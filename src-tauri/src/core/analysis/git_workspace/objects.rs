use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read as _;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::{DateTime, FixedOffset, Utc};
use flate2::read::ZlibDecoder;
use sha1::{Digest as _, Sha1};

use super::types::{
    GitBranchRef, GitCommitInfo, GitRecoveryItem, GitReflogInfo, GitRemoteInfo, GitStatusEntry,
    GitTagRef,
};
use crate::core::extensions::plugin::HostView;

pub const MAX_OBJECT_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PACK_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DELTA_DEPTH: usize = 64;
const MAX_WORKTREE_ENTRIES: usize = 10_000;
const MAX_WORKTREE_HASH_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct GitFsNode {
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub deleted: bool,
}

pub trait GitFsBackend {
    fn list(&self, rel_path: &str) -> Result<Vec<GitFsNode>>;
    fn read(&self, rel_path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>>;
    fn stat(&self, rel_path: &str) -> Result<Option<GitFsNode>>;
}

pub struct EvidenceViewBackend<'a> {
    view: &'a HostView,
    base: String,
}

impl<'a> EvidenceViewBackend<'a> {
    pub fn new(view: &'a HostView, base: &str) -> Self {
        Self {
            view,
            base: base.trim_matches('/').replace('\\', "/"),
        }
    }

    fn full_path(&self, rel_path: &str) -> String {
        let rel = rel_path.trim_matches('/').replace('\\', "/");
        if rel.is_empty() {
            self.base.clone()
        } else if self.base.is_empty() {
            rel
        } else {
            format!("{}/{rel}", self.base)
        }
    }
}

impl GitFsBackend for EvidenceViewBackend<'_> {
    fn list(&self, rel_path: &str) -> Result<Vec<GitFsNode>> {
        let full = self.full_path(rel_path);
        match self.view.list(&full, 0, 50_000) {
            Ok(page) => Ok(page
                .nodes
                .into_iter()
                .map(|n| GitFsNode {
                    name: n.name,
                    kind: n.kind,
                    size: n.size,
                    deleted: n.deleted,
                })
                .collect()),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn read(&self, rel_path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let full = self.full_path(rel_path);
        let Some(stat) = self.view.stat(&full)? else {
            return Ok(None);
        };
        if stat.kind != "file" || stat.size > max_bytes {
            return Ok(None);
        }
        let len = usize::try_from(stat.size).unwrap_or(usize::MAX);
        Ok(Some(self.view.read(&full, 0, len)?))
    }

    fn stat(&self, rel_path: &str) -> Result<Option<GitFsNode>> {
        let full = self.full_path(rel_path);
        Ok(self.view.stat(&full)?.map(|n| GitFsNode {
            name: n.name,
            kind: n.kind,
            size: n.size,
            deleted: n.deleted,
        }))
    }
}

pub struct LocalFsBackend {
    root: PathBuf,
}

impl LocalFsBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn resolve_path(&self, rel_path: &str) -> Result<PathBuf> {
        let mut out = self.root.clone();
        for seg in rel_path.split(['/', '\\']) {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." || seg.contains('\0') {
                return Err(anyhow!("invalid path component: {seg}"));
            }
            out.push(seg);
        }
        Ok(out)
    }
}

impl GitFsBackend for LocalFsBackend {
    fn list(&self, rel_path: &str) -> Result<Vec<GitFsNode>> {
        let path = self.resolve_path(rel_path)?;
        if !path.is_dir() {
            return Ok(Vec::new());
        }
        let mut nodes = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(entry.path())?;
            let kind = if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                "dir"
            } else if meta.is_file() {
                "file"
            } else {
                "other"
            };
            nodes.push(GitFsNode {
                name,
                kind: kind.to_owned(),
                size: meta.len(),
                deleted: false,
            });
        }
        Ok(nodes)
    }

    fn read(&self, rel_path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let path = self.resolve_path(rel_path)?;
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return Ok(None);
        };
        if !meta.is_file() || meta.len() > max_bytes {
            return Ok(None);
        }
        Ok(Some(std::fs::read(&path)?))
    }

    fn stat(&self, rel_path: &str) -> Result<Option<GitFsNode>> {
        let path = self.resolve_path(rel_path)?;
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return Ok(None);
        };
        let kind = if meta.file_type().is_symlink() {
            "link"
        } else if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "other"
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Some(GitFsNode {
            name,
            kind: kind.to_owned(),
            size: meta.len(),
            deleted: false,
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitObjectKind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl GitObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Tree => "tree",
            Self::Blob => "blob",
            Self::Tag => "tag",
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitObject {
    pub kind: GitObjectKind,
    pub body: Vec<u8>,
}

struct PackArchive {
    shas: Vec<[u8; 20]>,
    offsets: Vec<u64>,
    offset_to_idx: HashMap<u64, usize>,
    pack_bytes: Vec<u8>,
}

impl PackArchive {
    fn from_bytes(idx_bytes: &[u8], pack_bytes: Vec<u8>) -> Option<Self> {
        if idx_bytes.len() < 1032
            || &idx_bytes[0..8] != b"\xFFtOc\x00\x00\x00\x02"
            || pack_bytes.len() < 20
            || &pack_bytes[0..4] != b"PACK"
        {
            return None;
        }
        let total = u32::from_be_bytes([
            idx_bytes[1028],
            idx_bytes[1029],
            idx_bytes[1030],
            idx_bytes[1031],
        ]) as usize;
        if total == 0 || total > 250_000 {
            return None;
        }
        let sha_off = 1032usize;
        let crc_off = sha_off.checked_add(total.checked_mul(20)?)?;
        let off32_off = crc_off.checked_add(total.checked_mul(4)?)?;
        let off64_off = off32_off.checked_add(total.checked_mul(4)?)?;
        if off64_off > idx_bytes.len() {
            return None;
        }

        let mut shas = Vec::with_capacity(total);
        let mut offsets = Vec::with_capacity(total);
        let mut offset_to_idx = HashMap::with_capacity(total);

        for i in 0..total {
            let s = sha_off + i * 20;
            let mut sha = [0u8; 20];
            sha.copy_from_slice(&idx_bytes[s..s + 20]);
            shas.push(sha);

            let o = off32_off + i * 4;
            let raw = u32::from_be_bytes([
                idx_bytes[o],
                idx_bytes[o + 1],
                idx_bytes[o + 2],
                idx_bytes[o + 3],
            ]);
            let pack_off = if (raw & 0x8000_0000) == 0 {
                u64::from(raw)
            } else {
                let large_idx = (raw & 0x7FFF_FFFF) as usize;
                let lo = off64_off.checked_add(large_idx.checked_mul(8)?)?;
                if lo + 8 > idx_bytes.len() {
                    return None;
                }
                u64::from_be_bytes([
                    idx_bytes[lo],
                    idx_bytes[lo + 1],
                    idx_bytes[lo + 2],
                    idx_bytes[lo + 3],
                    idx_bytes[lo + 4],
                    idx_bytes[lo + 5],
                    idx_bytes[lo + 6],
                    idx_bytes[lo + 7],
                ])
            };
            offset_to_idx.insert(pack_off, i);
            offsets.push(pack_off);
        }

        Some(Self {
            shas,
            offsets,
            offset_to_idx,
            pack_bytes,
        })
    }

    fn find_sha(&self, target: &[u8; 20]) -> Option<usize> {
        self.shas.binary_search(target).ok()
    }
}

pub struct GitObjectStore<'a> {
    fs: &'a dyn GitFsBackend,
    git_dir: String,
    loose_shas: BTreeSet<String>,
    packs: Vec<PackArchive>,
    cache: HashMap<String, GitObject>,
}

impl<'a> GitObjectStore<'a> {
    pub fn load(fs: &'a dyn GitFsBackend, git_dir: &str) -> Result<Self> {
        let git_dir = git_dir.trim_matches('/').to_owned();
        let mut loose_shas = BTreeSet::new();

        let objects_dir = join_rel(&git_dir, "objects");
        for sub in fs.list(&objects_dir)? {
            if sub.kind != "dir" || sub.deleted || sub.name.len() != 2 {
                continue;
            }
            if !sub.name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let prefix = sub.name.to_ascii_lowercase();
            let sub_dir = join_rel(&objects_dir, &sub.name);
            for obj in fs.list(&sub_dir)? {
                if obj.kind == "file"
                    && !obj.deleted
                    && obj.name.len() == 38
                    && obj.name.chars().all(|c| c.is_ascii_hexdigit())
                {
                    loose_shas.insert(format!("{prefix}{}", obj.name.to_ascii_lowercase()));
                }
            }
        }

        let mut packs = Vec::new();
        let pack_dir = join_rel(&objects_dir, "pack");
        for node in fs.list(&pack_dir)? {
            if node.kind != "file"
                || node.deleted
                || !node.name.to_ascii_lowercase().ends_with(".idx")
            {
                continue;
            }
            let stem = &node.name[..node.name.len() - 4];
            let idx_path = join_rel(&pack_dir, &node.name);
            let pack_path = join_rel(&pack_dir, &format!("{stem}.pack"));
            let (Some(idx_bytes), Some(pack_bytes)) = (
                fs.read(&idx_path, MAX_PACK_BYTES)?,
                fs.read(&pack_path, MAX_PACK_BYTES)?,
            ) else {
                continue;
            };
            if let Some(archive) = PackArchive::from_bytes(&idx_bytes, pack_bytes) {
                packs.push(archive);
            }
        }

        Ok(Self {
            fs,
            git_dir,
            loose_shas,
            packs,
            cache: HashMap::new(),
        })
    }

    pub fn all_shas(&self) -> Vec<String> {
        let mut all: BTreeSet<String> = self.loose_shas.clone();
        for pack in &self.packs {
            for raw in &pack.shas {
                all.insert(hex::encode(raw));
            }
        }
        all.into_iter().collect()
    }

    pub fn resolve_prefix(&self, prefix: &str) -> Option<String> {
        let norm = prefix.trim().to_ascii_lowercase();
        if norm.len() == 40 && norm.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(norm);
        }
        if norm.len() < 4 || !norm.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut matches = Vec::new();
        for sha in &self.loose_shas {
            if sha.starts_with(&norm) {
                matches.push(sha.clone());
                if matches.len() > 1 {
                    break;
                }
            }
        }
        if matches.len() <= 1 {
            for pack in &self.packs {
                for raw in &pack.shas {
                    let s = hex::encode(raw);
                    if s.starts_with(&norm) && !matches.contains(&s) {
                        matches.push(s);
                        if matches.len() > 1 {
                            break;
                        }
                    }
                }
                if matches.len() > 1 {
                    break;
                }
            }
        }
        (matches.len() == 1).then(|| matches.swap_remove(0))
    }

    pub fn get(&mut self, sha: &str) -> Option<GitObject> {
        let norm = sha.trim().to_ascii_lowercase();
        if let Some(cached) = self.cache.get(&norm) {
            return Some(cached.clone());
        }
        let obj = self.load_uncached(&norm, 0)?;
        self.cache.insert(norm, obj.clone());
        Some(obj)
    }

    fn load_uncached(&mut self, sha: &str, depth: usize) -> Option<GitObject> {
        if depth > MAX_DELTA_DEPTH || sha.len() != 40 {
            return None;
        }
        if let Some(obj) = self.read_loose_object(sha) {
            return Some(obj);
        }
        let bytes = hex::decode(sha).ok()?;
        if bytes.len() != 20 {
            return None;
        }
        let mut raw = [0u8; 20];
        raw.copy_from_slice(&bytes);

        for pack_idx in 0..self.packs.len() {
            if let Some(entry_idx) = self.packs[pack_idx].find_sha(&raw) {
                let offset = self.packs[pack_idx].offsets[entry_idx];
                if let Some(obj) = self.read_pack_entry(pack_idx, offset, depth) {
                    return Some(obj);
                }
            }
        }
        None
    }

    fn read_loose_object(&self, sha: &str) -> Option<GitObject> {
        let rel = join_rel(
            &self.git_dir,
            &format!("objects/{}/{}", &sha[0..2], &sha[2..40]),
        );
        let compressed = self.fs.read(&rel, MAX_OBJECT_BYTES).ok().flatten()?;
        let decompressed = decompress_zlib_limited(&compressed, MAX_OBJECT_BYTES as usize)?;
        let nul = decompressed.iter().position(|&b| b == 0)?;
        let header = std::str::from_utf8(&decompressed[..nul]).ok()?;
        let kind_str = header.split(' ').next()?;
        let kind = match kind_str {
            "commit" => GitObjectKind::Commit,
            "tree" => GitObjectKind::Tree,
            "blob" => GitObjectKind::Blob,
            "tag" => GitObjectKind::Tag,
            _ => return None,
        };
        Some(GitObject {
            kind,
            body: decompressed[nul + 1..].to_vec(),
        })
    }

    fn read_pack_entry(&mut self, pack_idx: usize, offset: u64, depth: usize) -> Option<GitObject> {
        if depth > MAX_DELTA_DEPTH {
            return None;
        }
        let pack_off = usize::try_from(offset).ok()?;
        let (obj_type, size, data_cur, base_info) = {
            let pack = &self.packs[pack_idx].pack_bytes;
            if pack_off >= pack.len() {
                return None;
            }
            let mut cur = pack_off;
            let first = *pack.get(cur)?;
            cur += 1;
            let obj_type = (first >> 4) & 0x07;
            let mut size = usize::from(first & 0x0F);
            let mut shift = 4usize;
            let mut prev = first;
            while (prev & 0x80) != 0 {
                prev = *pack.get(cur)?;
                cur += 1;
                size |= usize::from(prev & 0x7F) << shift;
                shift = shift.saturating_add(7);
            }

            let base_info = match obj_type {
                6 => {
                    let mut c = *pack.get(cur)?;
                    cur += 1;
                    let mut neg = u64::from(c & 0x7F);
                    while (c & 0x80) != 0 {
                        c = *pack.get(cur)?;
                        cur += 1;
                        neg = ((neg.checked_add(1)?) << 7) | u64::from(c & 0x7F);
                    }
                    let base_off = offset.checked_sub(neg)?;
                    Some((Some(base_off), None))
                }
                7 => {
                    let slice = pack.get(cur..cur + 20)?;
                    cur += 20;
                    Some((None, Some(hex::encode(slice))))
                }
                1..=4 => None,
                _ => return None,
            };
            (obj_type, size, cur, base_info)
        };

        let max_decomp = size.max(64).min(MAX_OBJECT_BYTES as usize);
        let decompressed =
            decompress_zlib_limited(&self.packs[pack_idx].pack_bytes[data_cur..], max_decomp)?;

        match obj_type {
            1 => Some(GitObject {
                kind: GitObjectKind::Commit,
                body: decompressed,
            }),
            2 => Some(GitObject {
                kind: GitObjectKind::Tree,
                body: decompressed,
            }),
            3 => Some(GitObject {
                kind: GitObjectKind::Blob,
                body: decompressed,
            }),
            4 => Some(GitObject {
                kind: GitObjectKind::Tag,
                body: decompressed,
            }),
            6 => {
                let (base_off, _) = base_info?;
                let base_off = base_off?;
                let base_sha = self.packs[pack_idx]
                    .offset_to_idx
                    .get(&base_off)
                    .map(|&idx| hex::encode(self.packs[pack_idx].shas[idx]));
                let base_obj = if let Some(ref bsha) = base_sha {
                    if let Some(cached) = self.cache.get(bsha) {
                        cached.clone()
                    } else {
                        let loaded = self.read_pack_entry(pack_idx, base_off, depth + 1)?;
                        self.cache.insert(bsha.clone(), loaded.clone());
                        loaded
                    }
                } else {
                    self.read_pack_entry(pack_idx, base_off, depth + 1)?
                };
                let patched = apply_git_delta(&base_obj.body, &decompressed)?;
                Some(GitObject {
                    kind: base_obj.kind,
                    body: patched,
                })
            }
            7 => {
                let (_, base_sha) = base_info?;
                let base_sha = base_sha?;
                let base_obj = if let Some(cached) = self.cache.get(&base_sha) {
                    cached.clone()
                } else {
                    let loaded = self.load_uncached(&base_sha, depth + 1)?;
                    self.cache.insert(base_sha, loaded.clone());
                    loaded
                };
                let patched = apply_git_delta(&base_obj.body, &decompressed)?;
                Some(GitObject {
                    kind: base_obj.kind,
                    body: patched,
                })
            }
            _ => None,
        }
    }

    pub fn peel_to_commit(&mut self, rev_sha: &str) -> Option<String> {
        let mut cur = rev_sha.to_ascii_lowercase();
        for _ in 0..8 {
            let obj = self.get(&cur)?;
            match obj.kind {
                GitObjectKind::Commit => return Some(cur),
                GitObjectKind::Tag => {
                    cur = parse_tag_target(&obj.body)?;
                }
                _ => return None,
            }
        }
        None
    }

    pub fn read_tree_entries(&mut self, tree_sha: &str) -> Vec<ParsedTreeEntry> {
        let Some(obj) = self.get(tree_sha) else {
            return Vec::new();
        };
        if obj.kind != GitObjectKind::Tree {
            return Vec::new();
        }
        parse_tree_object(&obj.body)
    }

    pub fn resolve_dir_tree(&mut self, root_tree_sha: &str, dir_path: &str) -> Option<String> {
        let mut cur_sha = root_tree_sha.to_owned();
        for seg in dir_path.split('/').filter(|s| !s.is_empty() && *s != ".") {
            let entries = self.read_tree_entries(&cur_sha);
            let child = entries
                .into_iter()
                .find(|e| e.kind == "dir" && e.name == seg)?;
            cur_sha = child.sha;
        }
        Some(cur_sha)
    }

    pub fn collect_tree_blobs(
        &mut self,
        root_tree_sha: &str,
    ) -> BTreeMap<String, (String, String)> {
        let mut out = BTreeMap::new();
        self.walk_tree_blobs(root_tree_sha, "", &mut out, 0);
        out
    }

    fn walk_tree_blobs(
        &mut self,
        tree_sha: &str,
        prefix: &str,
        out: &mut BTreeMap<String, (String, String)>,
        depth: usize,
    ) {
        if depth > 64 || out.len() >= MAX_WORKTREE_ENTRIES {
            return;
        }
        for entry in self.read_tree_entries(tree_sha) {
            let full = join_rel(prefix, &entry.name);
            if entry.kind == "dir" {
                self.walk_tree_blobs(&entry.sha, &full, out, depth + 1);
            } else if entry.kind != "submodule" {
                out.insert(full, (entry.mode, entry.sha));
            }
        }
    }
}

fn decompress_zlib_limited(data: &[u8], max_out: usize) -> Option<Vec<u8>> {
    let decoder = ZlibDecoder::new(data);
    let mut limited = decoder.take((max_out as u64).saturating_add(1));
    let mut out = Vec::new();
    limited.read_to_end(&mut out).ok()?;
    if out.len() > max_out {
        return None;
    }
    Some(out)
}

fn read_delta_varint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let mut result = 0usize;
    let mut shift = 0usize;
    loop {
        let b = *data.get(*pos)?;
        *pos += 1;
        result |= usize::from(b & 0x7F) << shift;
        if (b & 0x80) == 0 {
            return Some(result);
        }
        shift = shift.saturating_add(7);
        if shift >= 63 {
            return None;
        }
    }
}

fn apply_git_delta(base: &[u8], delta: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0usize;
    let src_len = read_delta_varint(delta, &mut pos)?;
    let dst_len = read_delta_varint(delta, &mut pos)?;
    if src_len != base.len() || dst_len > MAX_OBJECT_BYTES as usize {
        return None;
    }
    let mut out = Vec::with_capacity(dst_len);
    while pos < delta.len() {
        let cmd = delta[pos];
        pos += 1;
        if (cmd & 0x80) != 0 {
            let mut copy_off = 0usize;
            let mut copy_len = 0usize;
            if (cmd & 0x01) != 0 {
                copy_off |= usize::from(*delta.get(pos)?);
                pos += 1;
            }
            if (cmd & 0x02) != 0 {
                copy_off |= usize::from(*delta.get(pos)?) << 8;
                pos += 1;
            }
            if (cmd & 0x04) != 0 {
                copy_off |= usize::from(*delta.get(pos)?) << 16;
                pos += 1;
            }
            if (cmd & 0x08) != 0 {
                copy_off |= usize::from(*delta.get(pos)?) << 24;
                pos += 1;
            }
            if (cmd & 0x10) != 0 {
                copy_len |= usize::from(*delta.get(pos)?);
                pos += 1;
            }
            if (cmd & 0x20) != 0 {
                copy_len |= usize::from(*delta.get(pos)?) << 8;
                pos += 1;
            }
            if (cmd & 0x40) != 0 {
                copy_len |= usize::from(*delta.get(pos)?) << 16;
                pos += 1;
            }
            if copy_len == 0 {
                copy_len = 0x10000;
            }
            let end = copy_off.checked_add(copy_len)?;
            let slice = base.get(copy_off..end)?;
            out.extend_from_slice(slice);
        } else if cmd > 0 {
            let len = usize::from(cmd);
            let end = pos.checked_add(len)?;
            let slice = delta.get(pos..end)?;
            out.extend_from_slice(slice);
            pos = end;
        } else {
            return None;
        }
    }
    (out.len() == dst_len).then_some(out)
}

#[derive(Clone, Debug)]
pub struct ParsedTreeEntry {
    pub mode: String,
    pub kind: String,
    pub name: String,
    pub sha: String,
}

pub fn parse_tree_object(body: &[u8]) -> Vec<ParsedTreeEntry> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    while pos < body.len() {
        let Some(space_rel) = body[pos..].iter().position(|&b| b == b' ') else {
            break;
        };
        let mode_raw = std::str::from_utf8(&body[pos..pos + space_rel]).unwrap_or("100644");
        let name_start = pos + space_rel + 1;
        let Some(nul_rel) = body[name_start..].iter().position(|&b| b == 0) else {
            break;
        };
        let name = String::from_utf8_lossy(&body[name_start..name_start + nul_rel]).into_owned();
        let sha_start = name_start + nul_rel + 1;
        if sha_start + 20 > body.len() {
            break;
        }
        let sha = hex::encode(&body[sha_start..sha_start + 20]);
        pos = sha_start + 20;

        let mode = if mode_raw.len() == 5 {
            format!("0{mode_raw}")
        } else {
            mode_raw.to_owned()
        };
        let kind = if mode == "040000" || mode_raw == "40000" {
            "dir"
        } else if mode == "160000" {
            "submodule"
        } else if mode == "120000" {
            "symlink"
        } else {
            "file"
        };
        entries.push(ParsedTreeEntry {
            mode,
            kind: kind.to_owned(),
            name,
            sha,
        });
    }
    entries
}

#[derive(Clone, Debug)]
pub struct ParsedCommit {
    pub tree_sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub committer: String,
    pub timestamp: Option<String>,
    pub timestamp_unix: i64,
    pub summary: String,
    pub message: String,
    pub raw_size: usize,
}

pub fn parse_commit_object(body: &[u8]) -> Option<ParsedCommit> {
    let text = String::from_utf8_lossy(body);
    let (headers, msg) = text.split_once("\n\n").unwrap_or((&text, ""));
    let mut tree_sha = String::new();
    let mut parents = Vec::new();
    let mut author = String::new();
    let mut committer = String::new();
    let mut timestamp = None;
    let mut timestamp_unix = 0i64;

    for line in headers.lines() {
        if let Some(t) = line.strip_prefix("tree ") {
            tree_sha = t.trim().to_ascii_lowercase();
        } else if let Some(p) = line.strip_prefix("parent ") {
            parents.push(p.trim().to_ascii_lowercase());
        } else if let Some(a) = line.strip_prefix("author ") {
            let (act, ts, unix) = parse_git_ident_and_time(a);
            author = act;
            if timestamp.is_none() {
                timestamp = ts;
                timestamp_unix = unix;
            }
        } else if let Some(c) = line.strip_prefix("committer ") {
            let (act, ts, unix) = parse_git_ident_and_time(c);
            committer = act;
            if ts.is_some() {
                timestamp = ts;
                timestamp_unix = unix;
            }
        }
    }

    if tree_sha.is_empty() && author.is_empty() {
        return None;
    }

    let message = msg.trim().to_owned();
    let summary = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("(no commit message)")
        .to_owned();

    Some(ParsedCommit {
        tree_sha,
        parents,
        author,
        committer,
        timestamp,
        timestamp_unix,
        summary,
        message,
        raw_size: body.len(),
    })
}

fn parse_tag_target(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    for line in text.lines() {
        if let Some(obj) = line.strip_prefix("object ") {
            let sha = obj.trim().to_ascii_lowercase();
            if sha.len() == 40 {
                return Some(sha);
            }
        }
    }
    None
}

pub fn parse_git_ident_and_time(raw: &str) -> (String, Option<String>, i64) {
    let Some(gt_pos) = raw.rfind('>') else {
        return (raw.trim().to_owned(), None, 0);
    };
    let actor = raw[..=gt_pos].trim().to_owned();
    let tail = raw[gt_pos + 1..].trim();
    let mut parts = tail.split_whitespace();
    let secs = parts.next().and_then(|s| s.parse::<i64>().ok());
    let tz = parts.next().unwrap_or("+0000");

    let Some(secs) = secs.filter(|s| (0..=4_102_444_800).contains(s)) else {
        return (actor, None, 0);
    };
    let Some(utc) = DateTime::<Utc>::from_timestamp(secs, 0) else {
        return (actor, None, 0);
    };
    let formatted = if let Some(offset) = parse_tz_offset(tz) {
        utc.with_timezone(&offset).to_rfc3339()
    } else {
        utc.to_rfc3339()
    };
    (actor, Some(formatted), secs)
}

fn parse_tz_offset(tz: &str) -> Option<FixedOffset> {
    let bytes = tz.as_bytes();
    if bytes.len() != 5 {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i32 = tz[1..3].parse().ok()?;
    let mins: i32 = tz[3..5].parse().ok()?;
    FixedOffset::east_opt(sign * (hours * 3600 + mins * 60))
}

#[derive(Default)]
pub struct ParsedGitConfig {
    pub remotes: Vec<GitRemoteInfo>,
    pub user_name: String,
    pub user_email: String,
    pub leaks: Vec<GitRecoveryItem>,
}

pub fn parse_git_config(text: &str) -> ParsedGitConfig {
    let mut out = ParsedGitConfig::default();
    let mut section = String::new();
    let mut remote_name: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let inner = line[1..line.len() - 1].trim();
            section = inner.to_ascii_lowercase();
            remote_name = None;
            if let Some(rest) = inner
                .strip_prefix("remote ")
                .or_else(|| inner.strip_prefix("Remote "))
            {
                remote_name = Some(rest.trim().trim_matches('"').to_owned());
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim().trim_matches('"').trim();
        if section.starts_with("remote") && key == "url" {
            let name = remote_name.clone().unwrap_or_else(|| "origin".to_owned());
            let github_repo = extract_github_owner_repo(val);
            let leak = detect_url_embedded_credential(val);
            if let Some(ref leak_msg) = leak {
                out.leaks.push(GitRecoveryItem {
                    id: format!("config-leak-{name}-{}", out.leaks.len()),
                    kind: "config_leak".to_owned(),
                    title: format!("远程 {name} 凭据暴露"),
                    object_sha: String::new(),
                    object_type: ".git/config".to_owned(),
                    finding_kind: "远程 URL 内嵌 Token / 凭据".to_owned(),
                    size: val.len() as u64,
                    preview: leak_msg.clone(),
                    source_path: Some(".git/config".to_owned()),
                });
            }
            out.remotes.push(GitRemoteInfo {
                name,
                url: val.to_owned(),
                github_repo,
                credential_leak: leak,
            });
        } else if section == "user" {
            if key == "name" {
                out.user_name = val.to_owned();
            } else if key == "email" {
                out.user_email = val.to_owned();
            }
        }
    }
    out
}

fn detect_url_embedded_credential(url: &str) -> Option<String> {
    if let Some(after_scheme) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        if let Some((userinfo, host_part)) = after_scheme.split_once('@') {
            if !userinfo.is_empty() && !userinfo.eq_ignore_ascii_case("git") {
                return Some(format!("远程 URL 内嵌凭据: {userinfo} @ {host_part}"));
            }
        }
    }
    if url.contains("ghp_") || url.contains("github_pat_") || url.contains("glpat-") {
        return Some(format!("发现 GitHub/GitLab Token: {url}"));
    }
    None
}

fn extract_github_owner_repo(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    for host in [
        "github.com/",
        "github.com:",
        "gitlab.com/",
        "gitlab.com:",
        "gitee.com/",
    ] {
        if let Some(pos) = lower.find(host) {
            let rest = &url[pos + host.len()..];
            let clean = rest.trim_end_matches('/').trim_end_matches(".git");
            if clean.contains('/') {
                return Some(clean.to_owned());
            }
        }
    }
    None
}

pub fn parse_reflog_text(text: &str, ref_name: &str) -> Vec<GitReflogInfo> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.len() < 85 {
            continue;
        }
        let (head_part, detail_part) = line.split_once('\t').unwrap_or((line, ""));
        let old_sha = head_part.get(0..40).unwrap_or("").to_ascii_lowercase();
        let new_sha = head_part.get(41..81).unwrap_or("").to_ascii_lowercase();
        if old_sha.len() != 40
            || new_sha.len() != 40
            || !old_sha.chars().all(|c| c.is_ascii_hexdigit())
            || !new_sha.chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        let meta = head_part.get(82..).unwrap_or("").trim();
        let (actor, timestamp, _) = parse_git_ident_and_time(meta);

        let detail_lower = detail_part.to_ascii_lowercase();
        let (action_type, is_rollback) = if detail_lower.starts_with("reset:") {
            ("版本回滚 (reset)".to_owned(), true)
        } else if detail_lower.starts_with("rebase") {
            ("变基重写 (rebase)".to_owned(), true)
        } else if detail_lower.starts_with("commit (amend)") {
            ("覆盖修改提交 (amend)".to_owned(), true)
        } else if detail_lower.starts_with("checkout:") {
            ("切换分支/游标 (checkout)".to_owned(), false)
        } else if detail_lower.starts_with("commit (initial)") {
            ("初始提交 (initial commit)".to_owned(), false)
        } else if detail_lower.starts_with("commit:") {
            ("常规提交 (commit)".to_owned(), false)
        } else if detail_lower.starts_with("pull") || detail_lower.starts_with("merge") {
            ("拉取合并 (pull/merge)".to_owned(), false)
        } else if detail_lower.starts_with("clone:") {
            ("克隆仓库 (clone)".to_owned(), false)
        } else {
            ("引用变更 (reflog)".to_owned(), false)
        };

        out.push(GitReflogInfo {
            ref_name: ref_name.to_owned(),
            old_short: old_sha[..old_sha.len().min(8)].to_owned(),
            new_short: new_sha[..new_sha.len().min(8)].to_owned(),
            old_sha,
            new_sha,
            action_type,
            is_rollback,
            actor,
            timestamp,
            detail: detail_part.to_owned(),
        });
    }
    out
}

#[derive(Clone, Debug)]
pub struct GitIndexEntry {
    pub path: String,
    pub sha: String,
    pub size: u32,
    pub mode: u32,
}

pub fn parse_git_index(bytes: &[u8]) -> Vec<GitIndexEntry> {
    let mut out = Vec::new();
    if bytes.len() < 12 || &bytes[0..4] != b"DIRC" {
        return out;
    }
    let version = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if !(2..=3).contains(&version) {
        return out;
    }
    let count = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let mut pos = 12usize;

    for _ in 0..count.min(20_000) {
        if pos + 62 > bytes.len() {
            break;
        }
        let entry_start = pos;
        let mode = u32::from_be_bytes([
            bytes[pos + 24],
            bytes[pos + 25],
            bytes[pos + 26],
            bytes[pos + 27],
        ]);
        let size = u32::from_be_bytes([
            bytes[pos + 36],
            bytes[pos + 37],
            bytes[pos + 38],
            bytes[pos + 39],
        ]);
        let sha = hex::encode(&bytes[pos + 40..pos + 60]);
        let flags = u16::from_be_bytes([bytes[pos + 60], bytes[pos + 61]]);
        let extended = (version >= 3) && ((flags & 0x4000) != 0);
        let name_start = entry_start + if extended { 64 } else { 62 };
        if name_start >= bytes.len() {
            break;
        }
        let nul_rel = bytes[name_start..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(0);
        let path = String::from_utf8_lossy(&bytes[name_start..name_start + nul_rel]).into_owned();
        let raw_len = (name_start - entry_start) + nul_rel + 1;
        let padded_len = (raw_len + 7) & !7;
        pos = entry_start + padded_len.max(64);
        if !path.is_empty() {
            out.push(GitIndexEntry {
                path,
                sha,
                size,
                mode,
            });
        }
    }
    out
}

pub fn scan_blob_for_secrets(body: &[u8]) -> Option<(String, String)> {
    if body.is_empty() || body.len() > 2 * 1024 * 1024 {
        return None;
    }
    if body.iter().take(512).any(|&b| b == 0) {
        return None;
    }
    let text = String::from_utf8_lossy(body);
    let patterns: &[(&str, &str)] = &[
        ("flag{", "疑似 CTF Flag (flag{...})"),
        ("CTF{", "疑似 CTF Flag (CTF{...})"),
        ("ghp_", "GitHub Personal Access Token (ghp_)"),
        ("github_pat_", "GitHub Fine-grained Token (github_pat_)"),
        ("glpat-", "GitLab Personal Access Token"),
        ("AKIA", "AWS Access Key ID (AKIA*)"),
        ("BEGIN RSA PRIVATE KEY", "RSA 私钥 (Private Key)"),
        ("BEGIN OPENSSH PRIVATE KEY", "OpenSSH 私钥 (Private Key)"),
        ("BEGIN PRIVATE KEY", "PEM 私钥 (Private Key)"),
        ("sk-", "API Secret Key (sk-*)"),
        ("DB_PASSWORD", "数据库密码配置 (DB_PASSWORD)"),
        ("SECRET_KEY", "应用密钥配置 (SECRET_KEY)"),
    ];
    for (needle, label) in patterns {
        if let Some(pos) = text.find(needle) {
            let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let end = text[pos..]
                .find('\n')
                .map(|i| pos + i)
                .unwrap_or(text.len())
                .min(start + 240);
            let snippet = text[start..end].trim().to_owned();
            return Some(((*label).to_owned(), snippet));
        }
    }
    None
}

pub fn git_blob_sha1(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", data.len()).as_bytes());
    h.update(data);
    hex::encode(h.finalize())
}

pub struct RepoRefState {
    pub head_ref: String,
    pub head_sha: Option<String>,
    pub is_detached: bool,
    pub branches: Vec<GitBranchRef>,
    pub tags: Vec<GitTagRef>,
    pub active_tips: BTreeSet<String>,
    pub sha_labels: HashMap<String, Vec<String>>,
}

pub fn collect_repo_refs(
    fs: &dyn GitFsBackend,
    git_dir: &str,
    store: &mut GitObjectStore<'_>,
) -> Result<RepoRefState> {
    let head_raw = read_text_opt(fs, &join_rel(git_dir, "HEAD"), 64 * 1024).unwrap_or_default();
    let head_trimmed = head_raw.trim();
    let (head_ref, is_detached) = if let Some(target) = head_trimmed.strip_prefix("ref: ") {
        (target.trim().to_owned(), false)
    } else {
        (head_trimmed.to_owned(), true)
    };

    let mut ref_map: BTreeMap<String, String> = BTreeMap::new();
    if let Some(packed) = read_text_opt(fs, &join_rel(git_dir, "packed-refs"), 2 * 1024 * 1024) {
        for line in packed.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some((sha, rname)) = line.split_once(' ') {
                let sha_l = sha.trim().to_ascii_lowercase();
                if sha_l.len() == 40 && sha_l.chars().all(|c| c.is_ascii_hexdigit()) {
                    ref_map.insert(rname.trim().to_owned(), sha_l);
                }
            }
        }
    }

    for (rel, content) in walk_text_files(fs, &join_rel(git_dir, "refs"), 5, 500)? {
        let sha_l = content.trim().to_ascii_lowercase();
        if sha_l.len() == 40 && sha_l.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Some(pos) = rel.find("refs/") {
                ref_map.insert(rel[pos..].to_owned(), sha_l);
            }
        }
    }

    let head_sha = if is_detached {
        (head_ref.len() == 40 && head_ref.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| head_ref.to_ascii_lowercase())
    } else {
        read_text_opt(fs, &join_rel(git_dir, &head_ref), 4096)
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
            .or_else(|| ref_map.get(&head_ref).cloned())
    };

    let mut branches = Vec::new();
    let mut tags = Vec::new();
    let mut active_tips = BTreeSet::new();
    let mut sha_labels: HashMap<String, Vec<String>> = HashMap::new();

    if let Some(ref hs) = head_sha {
        active_tips.insert(hs.clone());
        let head_badge = if is_detached {
            "HEAD (detached)".to_owned()
        } else {
            let short = head_ref.strip_prefix("refs/heads/").unwrap_or(&head_ref);
            format!("HEAD -> {short}")
        };
        sha_labels.entry(hs.clone()).or_default().push(head_badge);
    }

    for (full_ref, sha) in &ref_map {
        if let Some(name) = full_ref.strip_prefix("refs/heads/") {
            let is_head = !is_detached && head_ref == *full_ref;
            active_tips.insert(sha.clone());
            if !is_head {
                sha_labels
                    .entry(sha.clone())
                    .or_default()
                    .push(name.to_owned());
            }
            branches.push(GitBranchRef {
                name: name.to_owned(),
                full_ref: full_ref.clone(),
                sha: sha.clone(),
                is_head,
                is_remote: false,
            });
        } else if let Some(name) = full_ref.strip_prefix("refs/remotes/") {
            active_tips.insert(sha.clone());
            sha_labels
                .entry(sha.clone())
                .or_default()
                .push(name.to_owned());
            branches.push(GitBranchRef {
                name: name.to_owned(),
                full_ref: full_ref.clone(),
                sha: sha.clone(),
                is_head: false,
                is_remote: true,
            });
        } else if let Some(name) = full_ref.strip_prefix("refs/tags/") {
            let target_sha = store.peel_to_commit(sha).unwrap_or_else(|| sha.clone());
            active_tips.insert(target_sha.clone());
            sha_labels
                .entry(target_sha.clone())
                .or_default()
                .push(format!("tag: {name}"));
            tags.push(GitTagRef {
                name: name.to_owned(),
                sha: sha.clone(),
                target_sha,
            });
        }
    }

    Ok(RepoRefState {
        head_ref,
        head_sha,
        is_detached,
        branches,
        tags,
        active_tips,
        sha_labels,
    })
}

pub fn build_commit_and_recovery_lists(
    store: &mut GitObjectStore<'_>,
    refs: &RepoRefState,
    reflogs: &[GitReflogInfo],
    config_leaks: Vec<GitRecoveryItem>,
) -> (Vec<GitCommitInfo>, Vec<GitRecoveryItem>) {
    let mut rolled_back_shas = BTreeSet::new();
    let mut reset_target_shas = BTreeSet::new();
    let zero_sha = "0000000000000000000000000000000000000000";
    for rl in reflogs {
        if rl.is_rollback {
            if rl.old_sha != zero_sha {
                rolled_back_shas.insert(rl.old_sha.clone());
            }
            if rl.new_sha != zero_sha {
                reset_target_shas.insert(rl.new_sha.clone());
            }
        }
    }

    let mut parsed_commits: HashMap<String, ParsedCommit> = HashMap::new();
    let mut secret_blobs: Vec<GitRecoveryItem> = Vec::new();
    let mut seen_secrets = BTreeSet::new();

    for sha in store.all_shas() {
        let Some(obj) = store.get(&sha) else {
            continue;
        };
        match obj.kind {
            GitObjectKind::Commit => {
                if let Some(c) = parse_commit_object(&obj.body) {
                    parsed_commits.insert(sha, c);
                }
            }
            GitObjectKind::Blob if seen_secrets.insert(sha.clone()) => {
                if let Some((finding_kind, preview)) = scan_blob_for_secrets(&obj.body) {
                    secret_blobs.push(GitRecoveryItem {
                        id: format!("blob-{sha}"),
                        kind: "secret_blob".to_owned(),
                        title: finding_kind.clone(),
                        object_sha: sha,
                        object_type: "blob".to_owned(),
                        finding_kind,
                        size: obj.body.len() as u64,
                        preview,
                        source_path: None,
                    });
                }
            }
            _ => {}
        }
    }

    let mut reachable = BTreeSet::new();
    let mut queue: Vec<String> = refs.active_tips.iter().cloned().collect();
    while let Some(curr) = queue.pop() {
        if !reachable.insert(curr.clone()) {
            continue;
        }
        if let Some(c) = parsed_commits.get(&curr) {
            for p in &c.parents {
                queue.push(p.clone());
            }
        }
    }

    let mut commits = Vec::with_capacity(parsed_commits.len());
    let mut recoveries = config_leaks;

    for (sha, c) in parsed_commits {
        let is_reach = reachable.contains(&sha);
        let is_rb = rolled_back_shas.contains(&sha);
        let is_reset_target = reset_target_shas.contains(&sha);
        let (status, status_label) = if !is_reach && is_rb {
            ("rolled_back", "已回滚丢弃 (Rolled-back)")
        } else if !is_reach {
            ("dangling", "悬空/不可达 (Dangling)")
        } else if is_reset_target || is_rb {
            ("reset_target", "曾被回滚目标 (Reset Target)")
        } else {
            ("reachable", "正常可达 (Reachable)")
        };

        if !is_reach {
            recoveries.push(GitRecoveryItem {
                id: format!("commit-{sha}"),
                kind: "dangling_commit".to_owned(),
                title: c.summary.clone(),
                object_sha: sha.clone(),
                object_type: "commit".to_owned(),
                finding_kind: if is_rb {
                    "Reset/Rebase 回滚后未 GC 的提交".to_owned()
                } else {
                    "悬空未引用提交 (Dangling Commit)".to_owned()
                },
                size: c.raw_size as u64,
                preview: format!("{} · {}", c.author, c.message),
                source_path: None,
            });
        }

        let short_sha = sha[..sha.len().min(8)].to_owned();
        let ref_badges = refs.sha_labels.get(&sha).cloned().unwrap_or_default();
        commits.push(GitCommitInfo {
            sha,
            short_sha,
            parents: c.parents,
            tree_sha: c.tree_sha,
            status: status.to_owned(),
            status_label: status_label.to_owned(),
            author: c.author,
            committer: c.committer,
            timestamp: c.timestamp,
            timestamp_unix: c.timestamp_unix,
            summary: c.summary,
            message: c.message,
            refs: ref_badges,
        });
    }

    commits.sort_by(|a, b| {
        b.timestamp_unix
            .cmp(&a.timestamp_unix)
            .then_with(|| a.sha.cmp(&b.sha))
    });
    recoveries.extend(secret_blobs);

    (commits, recoveries)
}

pub fn collect_reflogs(fs: &dyn GitFsBackend, git_dir: &str) -> Result<Vec<GitReflogInfo>> {
    let mut entries = Vec::new();
    if let Some(head_log) = read_text_opt(fs, &join_rel(git_dir, "logs/HEAD"), 4 * 1024 * 1024) {
        entries.extend(parse_reflog_text(&head_log, "HEAD"));
    }
    for (rel, txt) in walk_text_files(fs, &join_rel(git_dir, "logs/refs"), 5, 200)? {
        let ref_label = rel.split("logs/").nth(1).unwrap_or(&rel).to_owned();
        entries.extend(parse_reflog_text(&txt, &ref_label));
    }
    entries.reverse();
    Ok(entries)
}

pub fn compute_evidence_worktree_status(
    fs: &dyn GitFsBackend,
    git_dir: &str,
    store: &mut GitObjectStore<'_>,
    head_sha: Option<&str>,
) -> Vec<GitStatusEntry> {
    let head_blobs = head_sha
        .and_then(|sha| store.get(sha))
        .and_then(|obj| parse_commit_object(&obj.body))
        .map(|c| store.collect_tree_blobs(&c.tree_sha))
        .unwrap_or_default();

    let work_files = walk_worktree_files(fs, git_dir);
    let mut out = Vec::new();

    for (path, size) in &work_files {
        match head_blobs.get(path) {
            None => out.push(GitStatusEntry {
                path: path.clone(),
                index_status: "?".to_owned(),
                worktree_status: "?".to_owned(),
                staged: false,
                kind: "untracked".to_owned(),
            }),
            Some((_, head_blob_sha)) => {
                if *size <= MAX_WORKTREE_HASH_BYTES {
                    if let Ok(Some(bytes)) = fs.read(path, MAX_WORKTREE_HASH_BYTES) {
                        let actual_sha = git_blob_sha1(&bytes);
                        if actual_sha != *head_blob_sha {
                            out.push(GitStatusEntry {
                                path: path.clone(),
                                index_status: " ".to_owned(),
                                worktree_status: "M".to_owned(),
                                staged: false,
                                kind: "modified".to_owned(),
                            });
                        }
                    }
                }
            }
        }
    }

    for path in head_blobs.keys() {
        if !work_files.contains_key(path) {
            out.push(GitStatusEntry {
                path: path.clone(),
                index_status: " ".to_owned(),
                worktree_status: "D".to_owned(),
                staged: false,
                kind: "deleted".to_owned(),
            });
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub fn walk_worktree_files(fs: &dyn GitFsBackend, git_dir: &str) -> BTreeMap<String, u64> {
    let mut files = BTreeMap::new();
    let mut stack = vec![(String::new(), 0usize)];
    let git_top = git_dir.trim_matches('/').to_ascii_lowercase();

    while let Some((dir, depth)) = stack.pop() {
        if depth > 32 || files.len() >= MAX_WORKTREE_ENTRIES {
            continue;
        }
        let Ok(nodes) = fs.list(&dir) else {
            continue;
        };
        for node in nodes {
            if node.deleted || node.name == ".git" || node.name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let rel = join_rel(&dir, &node.name);
            if !git_top.is_empty() && rel.eq_ignore_ascii_case(&git_top) {
                continue;
            }
            if node.kind == "dir" {
                stack.push((rel, depth + 1));
            } else if node.kind == "file" {
                files.insert(rel, node.size);
            }
        }
    }
    files
}

pub fn read_text_opt(fs: &dyn GitFsBackend, rel_path: &str, cap: u64) -> Option<String> {
    let bytes = fs.read(rel_path, cap).ok().flatten()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn walk_text_files(
    fs: &dyn GitFsBackend,
    root: &str,
    max_depth: usize,
    max_files: usize,
) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut queue = vec![(root.to_owned(), 0usize)];
    let mut seen = BTreeSet::new();
    while let Some((dir, depth)) = queue.pop() {
        if out.len() >= max_files || !seen.insert(dir.to_ascii_lowercase()) {
            continue;
        }
        for node in fs.list(&dir)? {
            if node.deleted || node.kind == "link" {
                continue;
            }
            let child = join_rel(&dir, &node.name);
            if node.kind == "dir" && depth < max_depth {
                queue.push((child, depth + 1));
            } else if node.kind == "file" {
                if let Some(txt) = read_text_opt(fs, &child, 2 * 1024 * 1024) {
                    out.push((child, txt));
                }
            }
        }
    }
    Ok(out)
}

pub fn join_rel(base: &str, child: &str) -> String {
    let b = base.trim_matches('/');
    let c = child.trim_matches('/');
    if b.is_empty() {
        c.to_owned()
    } else if c.is_empty() {
        b.to_owned()
    } else {
        format!("{b}/{c}")
    }
}
