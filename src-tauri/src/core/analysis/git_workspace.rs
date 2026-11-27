pub mod diff;
pub mod objects;
pub mod sandbox;
pub mod types;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use base64::Engine as _;

use crate::core::common::text_decode;
use crate::core::extensions::plugin::HostView;
use crate::core::filesystem::evidence_session::EvidenceSession;

use diff::{compute_unified_hunks, looks_binary};
use objects::{
    build_commit_and_recovery_lists, collect_reflogs, collect_repo_refs,
    compute_evidence_worktree_status, join_rel, parse_commit_object, parse_git_config,
    parse_git_index, read_text_opt, walk_worktree_files, GitFsBackend, GitObjectKind,
    GitObjectStore, MAX_OBJECT_BYTES,
};
use sandbox::{
    execute_sandbox_fs_action, execute_sandbox_git_action, export_from_backend,
    normalize_repo_path, resolve_rev_to_commit_sha, sandbox_git_status, sandbox_root_for,
    select_backend,
};
use types::{
    GitDiffFile, GitDiffResult, GitExportResult, GitFileContent, GitReadmePreview, GitRepoOverview,
    GitSandboxActionResult, GitSandboxFsResult, GitTreeEntry, GitTreeListing,
};

const MAX_READ_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_READ_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIFF_FILES: usize = 200;

fn evidence_view(image: &Arc<EvidenceSession>, image_id: &str) -> HostView {
    HostView::evidence(
        format!("git-workspace-{image_id}"),
        image_id.to_owned(),
        Arc::clone(image),
    )
}

fn repo_display_name(repo_norm: &str) -> String {
    repo_norm
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("repository")
        .to_owned()
}

pub fn repo_overview(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    use_sandbox: bool,
) -> Result<GitRepoOverview> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    let (fs, git_dir, source_mode, sandbox) =
        select_backend(&view, case_dir, image_id, &repo_norm, use_sandbox)?;

    let config_text =
        read_text_opt(fs.as_ref(), &join_rel(&git_dir, "config"), 512 * 1024).unwrap_or_default();
    let cfg = parse_git_config(&config_text);

    let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
    let refs = collect_repo_refs(fs.as_ref(), &git_dir, &mut store)?;
    let reflogs = collect_reflogs(fs.as_ref(), &git_dir)?;
    let (commits, recoveries) =
        build_commit_and_recovery_lists(&mut store, &refs, &reflogs, cfg.leaks);

    let index_count = fs
        .read(&join_rel(&git_dir, "index"), MAX_OBJECT_BYTES)?
        .map(|b| parse_git_index(&b).len())
        .unwrap_or(0);

    let status_entries = if source_mode == "sandbox" {
        let sb_dir = sandbox_root_for(case_dir, image_id, &repo_norm);
        sandbox_git_status(&sb_dir).unwrap_or_default()
    } else {
        compute_evidence_worktree_status(
            fs.as_ref(),
            &git_dir,
            &mut store,
            refs.head_sha.as_deref(),
        )
    };

    Ok(GitRepoOverview {
        repo_path: repo_norm.clone(),
        repo_name: repo_display_name(&repo_norm),
        source_mode,
        head_ref: refs.head_ref,
        head_sha: refs.head_sha,
        is_detached: refs.is_detached,
        remotes: cfg.remotes,
        user_name: cfg.user_name,
        user_email: cfg.user_email,
        branches: refs.branches,
        tags: refs.tags,
        commits,
        reflogs,
        recoveries,
        status_entries,
        index_count,
        sandbox,
    })
}

pub fn repo_tree(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    rev: &str,
    dir_path: &str,
    use_sandbox: bool,
) -> Result<GitTreeListing> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    let (fs, git_dir, source_mode, _) =
        select_backend(&view, case_dir, image_id, &repo_norm, use_sandbox)?;
    let clean_dir = clean_subpath(dir_path);
    let rev_norm = if rev.trim().is_empty() {
        "WORKTREE"
    } else {
        rev.trim()
    };

    if rev_norm.eq_ignore_ascii_case("WORKTREE") {
        let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
        let refs = collect_repo_refs(fs.as_ref(), &git_dir, &mut store)?;
        let status_entries = if source_mode == "sandbox" {
            let sb_dir = sandbox_root_for(case_dir, image_id, &repo_norm);
            sandbox_git_status(&sb_dir).unwrap_or_default()
        } else {
            compute_evidence_worktree_status(
                fs.as_ref(),
                &git_dir,
                &mut store,
                refs.head_sha.as_deref(),
            )
        };
        let status_map: BTreeMap<String, String> = status_entries
            .into_iter()
            .map(|s| (s.path, s.kind))
            .collect();

        let nodes = fs.list(&clean_dir)?;
        let mut entries = Vec::new();
        for node in nodes {
            if node.deleted || node.name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let rel = join_rel(&clean_dir, &node.name);
            let status = status_map.get(&rel).cloned();
            let mode = if node.kind == "dir" {
                "040000"
            } else if node.kind == "link" {
                "120000"
            } else {
                "100644"
            };
            entries.push(GitTreeEntry {
                name: node.name,
                path: rel,
                kind: if node.kind == "link" {
                    "symlink".to_owned()
                } else {
                    node.kind
                },
                mode: mode.to_owned(),
                sha: None,
                size: (mode != "040000").then_some(node.size),
                status,
            });
        }
        sort_tree_entries(&mut entries);
        let readme = find_readme_in_worktree(fs.as_ref(), &entries);
        return Ok(GitTreeListing {
            repo_path: repo_norm,
            source_mode,
            rev: "WORKTREE".to_owned(),
            resolved_sha: refs.head_sha,
            dir_path: clean_dir,
            entries,
            readme,
        });
    }

    let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
    if rev_norm.eq_ignore_ascii_case("INDEX") {
        let idx_bytes = fs
            .read(&join_rel(&git_dir, "index"), MAX_OBJECT_BYTES)?
            .unwrap_or_default();
        let idx_entries = parse_git_index(&idx_bytes);
        let prefix = if clean_dir.is_empty() {
            String::new()
        } else {
            format!("{clean_dir}/")
        };
        let mut dirs_seen = BTreeSet::new();
        let mut entries = Vec::new();
        for ie in idx_entries {
            let Some(rest) = (if prefix.is_empty() {
                Some(ie.path.as_str())
            } else {
                ie.path.strip_prefix(&prefix)
            }) else {
                continue;
            };
            if let Some((dir_name, _)) = rest.split_once('/') {
                if dirs_seen.insert(dir_name.to_owned()) {
                    entries.push(GitTreeEntry {
                        name: dir_name.to_owned(),
                        path: join_rel(&clean_dir, dir_name),
                        kind: "dir".to_owned(),
                        mode: "040000".to_owned(),
                        sha: None,
                        size: None,
                        status: None,
                    });
                }
            } else if !rest.is_empty() {
                entries.push(GitTreeEntry {
                    name: rest.to_owned(),
                    path: ie.path,
                    kind: "file".to_owned(),
                    mode: format!("{:06o}", ie.mode),
                    sha: Some(ie.sha),
                    size: Some(u64::from(ie.size)),
                    status: None,
                });
            }
        }
        sort_tree_entries(&mut entries);
        let readme = find_readme_in_store(&mut store, &entries);
        return Ok(GitTreeListing {
            repo_path: repo_norm,
            source_mode,
            rev: "INDEX".to_owned(),
            resolved_sha: None,
            dir_path: clean_dir,
            entries,
            readme,
        });
    }

    let commit_sha = resolve_rev_to_commit_sha(fs.as_ref(), &git_dir, &mut store, rev_norm)?;
    let commit_obj = store
        .get(&commit_sha)
        .ok_or_else(|| anyhow!("找不到提交对象: {commit_sha}"))?;
    let parsed = parse_commit_object(&commit_obj.body)
        .ok_or_else(|| anyhow!("无效的提交对象: {commit_sha}"))?;
    let dir_tree_sha = store
        .resolve_dir_tree(&parsed.tree_sha, &clean_dir)
        .ok_or_else(|| anyhow!("该版本不存在目录: {clean_dir}"))?;

    let raw_entries = store.read_tree_entries(&dir_tree_sha);
    let measure_sizes = raw_entries.len() <= 500;
    let mut entries = Vec::with_capacity(raw_entries.len());
    for re in raw_entries {
        let size = if measure_sizes && re.kind == "file" {
            store.get(&re.sha).map(|o| o.body.len() as u64)
        } else {
            None
        };
        entries.push(GitTreeEntry {
            path: join_rel(&clean_dir, &re.name),
            name: re.name,
            kind: re.kind,
            mode: re.mode,
            sha: Some(re.sha),
            size,
            status: None,
        });
    }
    sort_tree_entries(&mut entries);
    let readme = find_readme_in_store(&mut store, &entries);

    Ok(GitTreeListing {
        repo_path: repo_norm,
        source_mode,
        rev: rev_norm.to_owned(),
        resolved_sha: Some(commit_sha),
        dir_path: clean_dir,
        entries,
        readme,
    })
}

pub fn repo_file_read(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    rev: &str,
    file_path: &str,
    blob_sha: Option<&str>,
    use_sandbox: bool,
) -> Result<GitFileContent> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    let (fs, git_dir, _, _) = select_backend(&view, case_dir, image_id, &repo_norm, use_sandbox)?;
    let clean_path = clean_subpath(file_path);
    let rev_norm = if rev.trim().is_empty() {
        "WORKTREE"
    } else {
        rev.trim()
    };

    let (resolved_sha, bytes) = if let Some(bsha) =
        blob_sha.map(str::trim).filter(|s| !s.is_empty())
    {
        let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
        let full_sha = store
            .resolve_prefix(bsha)
            .ok_or_else(|| anyhow!("找不到 Blob SHA: {bsha}"))?;
        let obj = store
            .get(&full_sha)
            .ok_or_else(|| anyhow!("读取 Blob 对象失败: {full_sha}"))?;
        (Some(full_sha), obj.body)
    } else if rev_norm.eq_ignore_ascii_case("WORKTREE") {
        let data = fs
            .read(&clean_path, MAX_READ_IMAGE_BYTES as u64)?
            .ok_or_else(|| anyhow!("文件不存在或超过 8 MiB 读取上限: {clean_path}"))?;
        (None, data)
    } else if rev_norm.eq_ignore_ascii_case("INDEX") {
        let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
        let idx_bytes = fs
            .read(&join_rel(&git_dir, "index"), MAX_OBJECT_BYTES)?
            .ok_or_else(|| anyhow!("仓库不存在 .git/index"))?;
        let entry = parse_git_index(&idx_bytes)
            .into_iter()
            .find(|e| e.path == clean_path)
            .ok_or_else(|| anyhow!("暂存区找不到文件: {clean_path}"))?;
        let obj = store
            .get(&entry.sha)
            .ok_or_else(|| anyhow!("找不到暂存区对象: {}", entry.sha))?;
        (Some(entry.sha), obj.body)
    } else {
        let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;
        let commit_sha = resolve_rev_to_commit_sha(fs.as_ref(), &git_dir, &mut store, rev_norm)?;
        let commit_obj = store
            .get(&commit_sha)
            .ok_or_else(|| anyhow!("找不到提交对象: {commit_sha}"))?;
        let parsed = parse_commit_object(&commit_obj.body)
            .ok_or_else(|| anyhow!("无效提交对象: {commit_sha}"))?;
        let (parent_dir, file_name) = clean_path
            .rsplit_once('/')
            .unwrap_or(("", clean_path.as_str()));
        let dir_tree = store
            .resolve_dir_tree(&parsed.tree_sha, parent_dir)
            .ok_or_else(|| anyhow!("路径不存在: {clean_path}"))?;
        let target = store
            .read_tree_entries(&dir_tree)
            .into_iter()
            .find(|e| e.name == file_name)
            .ok_or_else(|| anyhow!("文件不存在: {clean_path}"))?;
        let obj = store
            .get(&target.sha)
            .ok_or_else(|| anyhow!("读取对象失败: {}", target.sha))?;
        (Some(target.sha), obj.body)
    };

    let name = clean_path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("blob")
        .to_owned();
    Ok(format_file_content(
        clean_path,
        name,
        rev_norm.to_owned(),
        resolved_sha,
        &bytes,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn repo_diff(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    target_rev: &str,
    base_rev: Option<&str>,
    use_sandbox: bool,
) -> Result<GitDiffResult> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    let (fs, git_dir, source_mode, _) =
        select_backend(&view, case_dir, image_id, &repo_norm, use_sandbox)?;
    let mut store = GitObjectStore::load(fs.as_ref(), &git_dir)?;

    let target_norm = if target_rev.trim().is_empty() {
        "WORKTREE"
    } else {
        target_rev.trim()
    };

    enum DiffTarget {
        Worktree,
        Index,
        Commit(String, BTreeMap<String, (String, String)>),
    }

    let (target_sha, target_state) = if target_norm.eq_ignore_ascii_case("WORKTREE") {
        (None, DiffTarget::Worktree)
    } else if target_norm.eq_ignore_ascii_case("INDEX") {
        (None, DiffTarget::Index)
    } else {
        let sha = resolve_rev_to_commit_sha(fs.as_ref(), &git_dir, &mut store, target_norm)?;
        let obj = store
            .get(&sha)
            .ok_or_else(|| anyhow!("找不到提交对象: {sha}"))?;
        let parsed =
            parse_commit_object(&obj.body).ok_or_else(|| anyhow!("无效提交对象: {sha}"))?;
        let blobs = store.collect_tree_blobs(&parsed.tree_sha);
        (
            Some(sha),
            DiffTarget::Commit(parsed.parents.first().cloned().unwrap_or_default(), blobs),
        )
    };

    let (base_label, base_sha, base_blobs) =
        if let Some(brev) = base_rev.map(str::trim).filter(|s| !s.is_empty()) {
            let bsha = resolve_rev_to_commit_sha(fs.as_ref(), &git_dir, &mut store, brev)?;
            let bobj = store
                .get(&bsha)
                .ok_or_else(|| anyhow!("找不到基线提交: {bsha}"))?;
            let bparsed =
                parse_commit_object(&bobj.body).ok_or_else(|| anyhow!("无效基线提交: {bsha}"))?;
            (
                brev.to_owned(),
                Some(bsha),
                store.collect_tree_blobs(&bparsed.tree_sha),
            )
        } else {
            match &target_state {
                DiffTarget::Worktree | DiffTarget::Index => {
                    let refs = collect_repo_refs(fs.as_ref(), &git_dir, &mut store)?;
                    if let Some(hsha) = refs.head_sha {
                        if let Some(hobj) = store.get(&hsha) {
                            if let Some(hparsed) = parse_commit_object(&hobj.body) {
                                (
                                    "HEAD".to_owned(),
                                    Some(hsha),
                                    store.collect_tree_blobs(&hparsed.tree_sha),
                                )
                            } else {
                                ("(empty)".to_owned(), None, BTreeMap::new())
                            }
                        } else {
                            ("(empty)".to_owned(), None, BTreeMap::new())
                        }
                    } else {
                        ("(empty)".to_owned(), None, BTreeMap::new())
                    }
                }
                DiffTarget::Commit(parent_sha, _) => {
                    if parent_sha.is_empty() {
                        ("(root)".to_owned(), None, BTreeMap::new())
                    } else if let Some(pobj) = store.get(parent_sha) {
                        if let Some(pparsed) = parse_commit_object(&pobj.body) {
                            (
                                parent_sha[..parent_sha.len().min(8)].to_owned(),
                                Some(parent_sha.clone()),
                                store.collect_tree_blobs(&pparsed.tree_sha),
                            )
                        } else {
                            ("(root)".to_owned(), None, BTreeMap::new())
                        }
                    } else {
                        ("(root)".to_owned(), None, BTreeMap::new())
                    }
                }
            }
        };

    let mut diff_files = Vec::new();
    let mut total_additions = 0usize;
    let mut total_deletions = 0usize;

    match target_state {
        DiffTarget::Worktree => {
            let work_files = walk_worktree_files(fs.as_ref(), &git_dir);
            let mut all_paths: BTreeSet<String> = base_blobs.keys().cloned().collect();
            all_paths.extend(work_files.keys().cloned());

            for path in all_paths {
                if diff_files.len() >= MAX_DIFF_FILES {
                    break;
                }
                let old_entry = base_blobs.get(&path);
                let in_work = work_files.contains_key(&path);
                let old_bytes = old_entry
                    .and_then(|(_, sha)| store.get(sha))
                    .map(|o| o.body);
                let new_bytes = if in_work {
                    fs.read(&path, MAX_READ_TEXT_BYTES as u64)?
                } else {
                    None
                };

                if let (Some(ob), Some(nb)) = (&old_bytes, &new_bytes) {
                    if ob == nb {
                        continue;
                    }
                }
                if old_entry.is_none() && !in_work {
                    continue;
                }

                let df = build_diff_file(
                    path,
                    old_entry.map(|(_, s)| s.clone()),
                    None,
                    old_bytes,
                    new_bytes,
                );
                total_additions += df.additions;
                total_deletions += df.deletions;
                diff_files.push(df);
            }
        }
        DiffTarget::Index => {
            let idx_bytes = fs
                .read(&join_rel(&git_dir, "index"), MAX_OBJECT_BYTES)?
                .unwrap_or_default();
            let new_blobs: BTreeMap<String, (String, String)> = parse_git_index(&idx_bytes)
                .into_iter()
                .map(|e| (e.path, (format!("{:06o}", e.mode), e.sha)))
                .collect();
            diff_blob_maps(
                &mut store,
                &base_blobs,
                &new_blobs,
                &mut diff_files,
                &mut total_additions,
                &mut total_deletions,
            );
        }
        DiffTarget::Commit(_, new_blobs) => {
            diff_blob_maps(
                &mut store,
                &base_blobs,
                &new_blobs,
                &mut diff_files,
                &mut total_additions,
                &mut total_deletions,
            );
        }
    }

    Ok(GitDiffResult {
        repo_path: repo_norm,
        source_mode,
        base_rev: base_label,
        target_rev: target_norm.to_owned(),
        base_sha,
        target_sha,
        total_additions,
        total_deletions,
        files: diff_files,
    })
}

pub fn repo_export(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    rev: &str,
    sub_path: Option<&str>,
    destination: &str,
    include_git_dir: bool,
    use_sandbox: bool,
) -> Result<GitExportResult> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    let (fs, git_dir, _, _) = select_backend(&view, case_dir, image_id, &repo_norm, use_sandbox)?;
    let rev_norm = if rev.trim().is_empty() {
        "WORKTREE"
    } else {
        rev.trim()
    };
    export_from_backend(
        fs.as_ref(),
        &git_dir,
        rev_norm,
        sub_path.unwrap_or(""),
        Path::new(destination),
        include_git_dir,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn sandbox_git_action(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    action: &str,
    target_ref: Option<&str>,
    branch_name: Option<&str>,
    commit_message: Option<&str>,
    file_paths: &[String],
    reset_from_evidence: bool,
) -> Result<GitSandboxActionResult> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    execute_sandbox_git_action(
        &view,
        case_dir,
        image_id,
        &repo_norm,
        action,
        target_ref,
        branch_name,
        commit_message,
        file_paths,
        reset_from_evidence,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn sandbox_fs_action(
    image: &Arc<EvidenceSession>,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    action: &str,
    target_path: &str,
    new_path: Option<&str>,
    content: Option<&str>,
) -> Result<GitSandboxFsResult> {
    let view = evidence_view(image, image_id);
    let repo_norm = normalize_repo_path(repo_path);
    execute_sandbox_fs_action(
        &view,
        case_dir,
        image_id,
        &repo_norm,
        action,
        target_path,
        new_path,
        content,
    )
}

fn diff_blob_maps(
    store: &mut GitObjectStore<'_>,
    base_blobs: &BTreeMap<String, (String, String)>,
    new_blobs: &BTreeMap<String, (String, String)>,
    out_files: &mut Vec<GitDiffFile>,
    total_add: &mut usize,
    total_del: &mut usize,
) {
    let mut all_paths: BTreeSet<String> = base_blobs.keys().cloned().collect();
    all_paths.extend(new_blobs.keys().cloned());

    for path in all_paths {
        if out_files.len() >= MAX_DIFF_FILES {
            break;
        }
        let old_sha = base_blobs.get(&path).map(|(_, s)| s.clone());
        let new_sha = new_blobs.get(&path).map(|(_, s)| s.clone());
        if old_sha == new_sha {
            continue;
        }
        let old_bytes = old_sha
            .as_deref()
            .and_then(|s| store.get(s))
            .map(|o| o.body);
        let new_bytes = new_sha
            .as_deref()
            .and_then(|s| store.get(s))
            .map(|o| o.body);
        let df = build_diff_file(path, old_sha, new_sha, old_bytes, new_bytes);
        *total_add += df.additions;
        *total_del += df.deletions;
        out_files.push(df);
    }
}

fn build_diff_file(
    path: String,
    old_sha: Option<String>,
    new_sha: Option<String>,
    old_bytes: Option<Vec<u8>>,
    new_bytes: Option<Vec<u8>>,
) -> GitDiffFile {
    let status = match (&old_bytes, &new_bytes) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        _ => "modified",
    };

    let old_bin = old_bytes.as_deref().is_some_and(looks_binary);
    let new_bin = new_bytes.as_deref().is_some_and(looks_binary);
    if old_bin || new_bin {
        return GitDiffFile {
            path,
            status: "binary".to_owned(),
            old_sha,
            new_sha,
            additions: 0,
            deletions: 0,
            is_binary: true,
            hunks: Vec::new(),
        };
    }

    let old_text = old_bytes
        .as_deref()
        .map(|b| text_decode::decode(b).text)
        .unwrap_or_default();
    let new_text = new_bytes
        .as_deref()
        .map(|b| text_decode::decode(b).text)
        .unwrap_or_default();

    let (hunks, additions, deletions) = compute_unified_hunks(&old_text, &new_text);
    GitDiffFile {
        path,
        status: status.to_owned(),
        old_sha,
        new_sha,
        additions,
        deletions,
        is_binary: false,
        hunks,
    }
}

fn format_file_content(
    path: String,
    name: String,
    rev: String,
    blob_sha: Option<String>,
    bytes: &[u8],
) -> GitFileContent {
    let size = bytes.len() as u64;
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();

    if let Some(mime) = image_mime_for_ext(&ext) {
        if bytes.len() <= MAX_READ_IMAGE_BYTES {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return GitFileContent {
                path,
                name,
                rev,
                blob_sha,
                size,
                kind: "image".to_owned(),
                mime: mime.to_owned(),
                encoding: None,
                text: None,
                image_data_url: Some(format!("data:{mime};base64,{b64}")),
                hex_preview: None,
                truncated: false,
            };
        }
    }

    if looks_binary(bytes) {
        let preview_len = bytes.len().min(4096);
        let hex_preview = format_hex_dump(&bytes[..preview_len]);
        return GitFileContent {
            path,
            name,
            rev,
            blob_sha,
            size,
            kind: "binary".to_owned(),
            mime: "application/octet-stream".to_owned(),
            encoding: None,
            text: None,
            image_data_url: None,
            hex_preview: Some(hex_preview),
            truncated: bytes.len() > preview_len,
        };
    }

    let truncated = bytes.len() > MAX_READ_TEXT_BYTES;
    let slice = &bytes[..bytes.len().min(MAX_READ_TEXT_BYTES)];
    let decoded = text_decode::decode(slice);
    GitFileContent {
        path,
        name,
        rev,
        blob_sha,
        size,
        kind: "text".to_owned(),
        mime: "text/plain".to_owned(),
        encoding: Some(decoded.encoding),
        text: Some(decoded.text),
        image_data_url: None,
        hex_preview: None,
        truncated,
    }
}

fn image_mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

fn format_hex_dump(bytes: &[u8]) -> String {
    let mut lines = Vec::with_capacity(bytes.len().div_ceil(16));
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let addr = row * 16;
        let mut hex_part = String::with_capacity(48);
        let mut ascii_part = String::with_capacity(16);
        for (i, &b) in chunk.iter().enumerate() {
            hex_part.push_str(&format!("{b:02x} "));
            if i == 7 {
                hex_part.push(' ');
            }
            ascii_part.push(if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            });
        }
        lines.push(format!("{addr:08x}  {hex_part:<49} |{ascii_part}|"));
    }
    lines.join("\n")
}

fn clean_subpath(raw: &str) -> String {
    raw.trim()
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect::<Vec<_>>()
        .join("/")
}

fn sort_tree_entries(entries: &mut [GitTreeEntry]) {
    entries.sort_by(|a, b| {
        let a_dir = a.kind == "dir";
        let b_dir = b.kind == "dir";
        b_dir
            .cmp(&a_dir)
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn is_readme_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "readme.md" | "readme.markdown" | "readme.txt" | "readme"
    )
}

fn find_readme_in_worktree(
    fs: &dyn GitFsBackend,
    entries: &[GitTreeEntry],
) -> Option<GitReadmePreview> {
    let entry = entries
        .iter()
        .find(|e| e.kind == "file" && is_readme_name(&e.name))?;
    let bytes = fs
        .read(&entry.path, MAX_READ_TEXT_BYTES as u64)
        .ok()
        .flatten()?;
    if looks_binary(&bytes) {
        return None;
    }
    Some(GitReadmePreview {
        path: entry.path.clone(),
        name: entry.name.clone(),
        content: text_decode::decode(&bytes).text,
    })
}

fn find_readme_in_store(
    store: &mut GitObjectStore<'_>,
    entries: &[GitTreeEntry],
) -> Option<GitReadmePreview> {
    let entry = entries
        .iter()
        .find(|e| e.kind == "file" && is_readme_name(&e.name))?;
    let sha = entry.sha.as_deref()?;
    let obj = store.get(sha)?;
    if obj.kind != GitObjectKind::Blob || looks_binary(&obj.body) {
        return None;
    }
    let slice = &obj.body[..obj.body.len().min(MAX_READ_TEXT_BYTES)];
    Some(GitReadmePreview {
        path: entry.path.clone(),
        name: entry.name.clone(),
        content: text_decode::decode(slice).text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::filesystem::evidence_session::EvidenceSession;

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git command runs");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn full_git_forensic_and_sandbox_workflow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let evidence_root = temp.path().join("evidence");
        let repo_dir = evidence_root.join("home").join("kali").join("demo-repo");
        let case_dir = temp.path().join("case");
        let export_dir = temp.path().join("exported");
        std::fs::create_dir_all(&repo_dir).expect("create repo dir");
        std::fs::create_dir_all(&case_dir).expect("create case dir");

        run_git(&repo_dir, &["init", "-b", "main"]);
        run_git(&repo_dir, &["config", "user.name", "Alice"]);
        run_git(&repo_dir, &["config", "user.email", "alice@example.com"]);
        run_git(
            &repo_dir,
            &[
                "remote",
                "add",
                "origin",
                "https://alice:ghp_SuperSecretToken123@github.com/alice/demo-repo.git",
            ],
        );

        std::fs::write(repo_dir.join("README.md"), "# Demo Repo\nInitial version\n").unwrap();
        std::fs::create_dir_all(repo_dir.join("src")).unwrap();
        std::fs::write(repo_dir.join("src/app.py"), "print('v1')\n").unwrap();
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-m", "Initial commit"]);
        let c1_sha = run_git(&repo_dir, &["rev-parse", "HEAD"]);

        // Second commit containing a secret that gets rolled back!
        std::fs::write(
            repo_dir.join("src/secret.env"),
            "SECRET_KEY=flag{git_rollback_recovered}\n",
        )
        .unwrap();
        std::fs::write(repo_dir.join("src/app.py"), "print('v2')\n").unwrap();
        run_git(&repo_dir, &["add", "."]);
        run_git(&repo_dir, &["commit", "-m", "Add secret config"]);
        let c2_sha = run_git(&repo_dir, &["rev-parse", "HEAD"]);

        // Rollback via reset --hard HEAD~1
        run_git(&repo_dir, &["reset", "--hard", "HEAD~1"]);
        // Pack reachable objects into .pack/.idx while preserving rolled-back loose commit
        run_git(&repo_dir, &["repack", "-a"]);

        let session =
            Arc::new(EvidenceSession::open(&evidence_root).expect("open folder evidence"));
        let image_id = "img-test";
        let repo_path = "root/home/kali/demo-repo";

        // 1. Overview in evidence mode finds commits (including rolled-back c2!), reflog, and secrets
        let ov = repo_overview(&session, &case_dir, image_id, repo_path, false).expect("overview");
        assert_eq!(ov.repo_name, "demo-repo");
        assert_eq!(ov.source_mode, "evidence");
        assert_eq!(ov.head_sha.as_deref(), Some(c1_sha.as_str()));
        assert!(!ov.remotes.is_empty());
        assert_eq!(
            ov.remotes[0].github_repo.as_deref(),
            Some("alice/demo-repo")
        );
        assert!(ov
            .commits
            .iter()
            .any(|c| c.sha == c1_sha && c.status == "reset_target"));
        assert!(ov
            .commits
            .iter()
            .any(|c| c.sha == c2_sha && c.status == "rolled_back"));
        assert!(ov
            .reflogs
            .iter()
            .any(|r| r.is_rollback && r.old_sha == c2_sha));
        assert!(ov
            .recoveries
            .iter()
            .any(|r| r.preview.contains("flag{git_rollback_recovered}")));

        // 2. Browse tree at rolled-back commit c2_sha
        let tree_c2 = repo_tree(
            &session, &case_dir, image_id, repo_path, &c2_sha, "src", false,
        )
        .expect("tree c2");
        assert!(tree_c2.entries.iter().any(|e| e.name == "secret.env"));

        // 3. Read secret.env from rolled-back commit c2_sha
        let file_c2 = repo_file_read(
            &session,
            &case_dir,
            image_id,
            repo_path,
            &c2_sha,
            "src/secret.env",
            None,
            false,
        )
        .expect("read c2 secret.env");
        assert!(file_c2
            .text
            .as_deref()
            .unwrap_or("")
            .contains("flag{git_rollback_recovered}"));

        // 4. Diff between c1 and c2
        let diff_res = repo_diff(
            &session,
            &case_dir,
            image_id,
            repo_path,
            &c2_sha,
            Some(&c1_sha),
            false,
        )
        .expect("diff c1..c2");
        assert!(diff_res
            .files
            .iter()
            .any(|f| f.path == "src/secret.env" && f.status == "added"));
        assert!(diff_res
            .files
            .iter()
            .any(|f| f.path == "src/app.py" && f.status == "modified"));

        // 5. Export snapshot of rolled-back commit c2
        let exp = repo_export(
            &session,
            &case_dir,
            image_id,
            repo_path,
            &c2_sha,
            None,
            export_dir.to_str().unwrap(),
            false,
            false,
        )
        .expect("export c2");
        assert!(exp.exported_files >= 3);
        assert!(export_dir.join("src/secret.env").is_file());

        // 6. Interactive Sandbox: rollback/restore to c2_sha & manage folders/files!
        let sb_res = sandbox_git_action(
            &session,
            &case_dir,
            image_id,
            repo_path,
            "reset_hard",
            Some(&c2_sha),
            None,
            None,
            &[],
            false,
        )
        .expect("sandbox reset_hard to c2");
        assert!(sb_res.ok, "stderr: {}", sb_res.stderr);
        assert!(sb_res.sandbox.initialized);

        let ov_sb = repo_overview(&session, &case_dir, image_id, repo_path, true)
            .expect("sandbox overview");
        assert_eq!(ov_sb.source_mode, "sandbox");
        assert_eq!(ov_sb.head_sha.as_deref(), Some(c2_sha.as_str()));

        // Create folder & file, stage & commit in sandbox
        sandbox_fs_action(
            &session, &case_dir, image_id, repo_path, "mkdir", "notes", None, None,
        )
        .expect("mkdir notes");
        sandbox_fs_action(
            &session,
            &case_dir,
            image_id,
            repo_path,
            "write_file",
            "notes/finding.txt",
            None,
            Some("Recovered flag!"),
        )
        .expect("write finding.txt");
        let commit_res = sandbox_git_action(
            &session,
            &case_dir,
            image_id,
            repo_path,
            "stage",
            None,
            None,
            None,
            &["notes/finding.txt".to_owned()],
            false,
        )
        .expect("stage");
        assert!(commit_res.ok);
        let committed = sandbox_git_action(
            &session,
            &case_dir,
            image_id,
            repo_path,
            "commit",
            None,
            None,
            Some("Document recovered flag"),
            &[],
            false,
        )
        .expect("commit");
        assert!(committed.ok, "commit stderr: {}", committed.stderr);

        // Original evidence repo remains completely untouched at c1_sha!
        let head_orig = run_git(&repo_dir, &["rev-parse", "HEAD"]);
        assert_eq!(head_orig, c1_sha);
    }
}
