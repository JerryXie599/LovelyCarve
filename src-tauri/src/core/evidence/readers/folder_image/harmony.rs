//! HarmonyOS NEXT (原生鸿蒙) backup manifest parser & O(1) seekable virtual tree reader.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::harmony_crypto::{
    aes256_gcm_decrypt_and_verify, compute_j0, derive_module_key_and_iv, unwrap_backup_key,
};
use super::harmony_tar::{
    directory_entry, ensure_parent_dirs, host_file_entry, index_tar_into_entries,
    insert_harmony_file, insert_host_if_absent, sanitize_rel_path, unix_time, HarmonyBacking,
};
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonyBackupInfo {
    pub locked: bool,
    pub encryption_type: u32,
    pub declared_encrypted: bool,
    pub backup_version: Option<u64>,
    pub device_model: Option<String>,
    pub product_model: Option<String>,
    pub device_id: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
    pub backup_time_ms: Option<i64>,
    pub backup_time_utc8: Option<String>,
    pub select_data_size: Option<u64>,
    pub backup_reminder: Option<String>,
    pub module_count: usize,
    pub clone_modules: Vec<String>,
    pub file_count: Option<usize>,
    pub special_bundles: Vec<String>,
}

#[derive(Debug)]
pub(super) struct HarmonyBackup {
    pub info: HarmonyBackupInfo,
    files: Vec<HarmonyBacking>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawInfoJson {
    #[serde(rename = "BackupFileHeaderInfo")]
    pub header: Option<RawHeaderInfo>,
    #[serde(rename = "BackupFileVersionInfo")]
    pub version: Option<RawVersionInfo>,
    #[serde(rename = "BackupFileTypeInfo")]
    pub backup_type: Option<RawTypeInfo>,
    #[serde(rename = "BackupFileModuleInfo", default)]
    pub modules: Vec<RawModuleInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawHeaderInfo {
    pub backup_version: Option<u64>,
    pub device_model: Option<String>,
    pub product_model: Option<String>,
    pub device_id: Option<String>,
    pub select_data_size: Option<u64>,
    pub backup_time: Option<i64>,
    #[serde(default)]
    pub special_bundles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawVersionInfo {
    pub version_code: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawTypeInfo {
    #[serde(default)]
    pub encryption_type: u32,
    #[serde(default)]
    pub backup_key: String,
    #[serde(default)]
    pub backup_salt: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub backup_iv: String,
    #[serde(default)]
    pub backup_reminder: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawModuleInfo {
    #[serde(default)]
    pub module_name: String,
    #[serde(rename = "module_name", default)]
    pub qualified_name: String,
    #[serde(rename = "module_type", default)]
    pub module_type: String,
    #[serde(default)]
    pub encryption_salt: String,
    #[serde(default)]
    pub encryption_iv: String,
    #[serde(default)]
    pub app_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManageEntry {
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    is_big_file: bool,
    information: Option<ManageEntryInfo>,
}

#[derive(Debug, Deserialize)]
struct ManageEntryInfo {
    #[serde(default)]
    path: String,
    stat: Option<ManageStat>,
}

#[derive(Debug, Deserialize)]
struct ManageStat {
    st_size: Option<u64>,
    st_mode: Option<u32>,
    st_mtim: Option<ManageTime>,
    st_atim: Option<ManageTime>,
}

#[derive(Debug, Deserialize)]
struct ManageTime {
    tv_sec: Option<i64>,
}

/// Parses `.info.json` bytes into [`HarmonyBackupInfo`] when the file matches HarmonyOS NEXT backup schema.
pub fn probe_info_json(bytes: &[u8]) -> Option<HarmonyBackupInfo> {
    let raw: RawInfoJson = serde_json::from_slice(bytes).ok()?;
    let header = raw.header.as_ref()?;
    let backup_type = raw.backup_type.as_ref()?;
    Some(build_info(
        &raw,
        header,
        backup_type,
        backup_type.encryption_type != 0,
        None,
    ))
}

fn build_info(
    raw: &RawInfoJson,
    header: &RawHeaderInfo,
    backup_type: &RawTypeInfo,
    locked: bool,
    file_count: Option<usize>,
) -> HarmonyBackupInfo {
    let clone_modules = raw
        .modules
        .iter()
        .filter(|m| m.app_index > 0 || m.qualified_name.contains(':'))
        .map(|m| {
            if !m.qualified_name.is_empty() {
                m.qualified_name.clone()
            } else {
                format!("{}:{}", m.module_name, m.app_index)
            }
        })
        .collect();
    HarmonyBackupInfo {
        locked,
        encryption_type: backup_type.encryption_type,
        declared_encrypted: backup_type.encryption_type != 0,
        backup_version: header.backup_version,
        device_model: header.device_model.clone(),
        product_model: header.product_model.clone(),
        device_id: header.device_id.clone(),
        version_name: raw.version.as_ref().and_then(|v| v.version_name.clone()),
        version_code: raw.version.as_ref().and_then(|v| v.version_code.clone()),
        backup_time_ms: header.backup_time,
        backup_time_utc8: format_utc8_time(header.backup_time),
        select_data_size: header.select_data_size,
        backup_reminder: (!backup_type.backup_reminder.is_empty())
            .then(|| backup_type.backup_reminder.clone()),
        module_count: raw.modules.len(),
        clone_modules,
        file_count,
        special_bundles: header.special_bundles.clone(),
    }
}

fn format_utc8_time(millis: Option<i64>) -> Option<String> {
    let ms = millis?;
    let utc = chrono::DateTime::from_timestamp_millis(ms)?;
    let offset = chrono::FixedOffset::east_opt(8 * 3600)?;
    Some(
        utc.with_timezone(&offset)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    )
}

fn read_info_json(root: &Path) -> Result<Option<(PathBuf, RawInfoJson)>> {
    let info_path = root.join(".info.json");
    if !info_path.is_file() {
        return Ok(None);
    }
    let meta = fs::metadata(&info_path)?;
    if meta.len() > 16 * 1024 * 1024 {
        return Ok(None);
    }
    let bytes = fs::read(&info_path)?;
    let Ok(raw) = serde_json::from_slice::<RawInfoJson>(&bytes) else {
        return Ok(None);
    };
    if raw.header.is_none() || raw.backup_type.is_none() {
        return Ok(None);
    }
    Ok(Some((info_path, raw)))
}

impl FolderImage {
    /// Detects a HarmonyOS NEXT backup directory (`.info.json` + `files/`).
    pub(super) fn detect_harmony(root: &Path) -> Result<Option<Self>> {
        let Some((_, raw)) = read_info_json(root)? else {
            return Ok(None);
        };
        let header = raw
            .header
            .as_ref()
            .context("missing BackupFileHeaderInfo")?;
        let backup_type = raw
            .backup_type
            .as_ref()
            .context("missing BackupFileTypeInfo")?;
        if backup_type.encryption_type == 0 {
            return Self::open_harmony(root, None, &CancellationToken::new()).map(Some);
        }
        let info = build_info(&raw, header, backup_type, true, None);
        let mut image = Self::open_host(root)?;
        image.harmony_backup = Some(HarmonyBackup {
            info,
            files: Vec::new(),
        });
        Ok(Some(image))
    }

    /// Unlocks (when `password` is `Some`) or opens (when unencrypted) a HarmonyOS NEXT backup.
    pub fn open_harmony(
        root: &Path,
        password: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        if cancel.is_cancelled() {
            bail!("HarmonyOS backup unlock cancelled");
        }
        let (info_path, raw) = read_info_json(root)?
            .context("not a valid HarmonyOS NEXT backup (.info.json missing)")?;
        let header = raw
            .header
            .as_ref()
            .context("missing BackupFileHeaderInfo")?;
        let backup_type = raw
            .backup_type
            .as_ref()
            .context("missing BackupFileTypeInfo")?;

        let bkey_raw = if backup_type.encryption_type != 0 {
            let pw = password.context("password required for encrypted HarmonyOS backup")?;
            unwrap_backup_key(
                pw.as_bytes(),
                &backup_type.backup_salt,
                &backup_type.backup_key,
            )?
        } else {
            Vec::new()
        };

        let mut entries = BTreeMap::new();
        entries.insert(String::new(), directory_entry(String::new()));
        entries.insert("__backup".into(), directory_entry("__backup".into()));
        let info_meta = fs::metadata(&info_path)?;
        entries.insert(
            "__backup/.info.json".into(),
            host_file_entry("__backup/.info.json".into(), &info_path, &info_meta),
        );

        let mut backing_files: Vec<HarmonyBacking> = Vec::new();
        let mut file_count = 0usize;
        let mut total_size = info_meta.len();
        let encrypted = backup_type.encryption_type != 0;

        for module in &raw.modules {
            if cancel.is_cancelled() {
                bail!("HarmonyOS backup unlock cancelled");
            }
            let Some((dir_label, mdir)) = resolve_module_dir(root, module) else {
                continue;
            };
            let mod_cipher = if encrypted {
                match derive_module_key_and_iv(
                    &bkey_raw,
                    &module.encryption_salt,
                    &module.encryption_iv,
                )
                .and_then(|(key, iv)| compute_j0(&key, &iv).map(|j0| (key, iv, j0)))
                {
                    Ok(c) => Some(c),
                    Err(err) => {
                        tracing::warn!(module = %dir_label, "skip module key derivation: {err:#}");
                        None
                    }
                }
            } else {
                None
            };

            let mod_prefix = format!("modules/{dir_label}");
            ensure_parent_dirs(&mut entries, &format!("{mod_prefix}/.keep"))?;

            let mut managed_disk_names = HashSet::new();
            let mj_path = mdir.join("manage.json");
            if mj_path.is_file() {
                managed_disk_names.insert("manage.json".to_string());
                let raw_mj = fs::read(&mj_path)?;
                let plain_mj = if let Some((ref key, ref iv, _)) = mod_cipher {
                    aes256_gcm_decrypt_and_verify(key, iv, &raw_mj)
                        .with_context(|| format!("decrypt manage.json for {dir_label}"))?
                } else {
                    raw_mj
                };
                let mj_entries: Vec<ManageEntry> = serde_json::from_slice(&plain_mj)
                    .with_context(|| format!("parse manage.json for {dir_label}"))?;

                let mj_rel = format!("{mod_prefix}/manage.json");
                let mj_idx = backing_files.len();
                let mj_len = plain_mj.len() as u64;
                backing_files.push(HarmonyBacking::Memory(plain_mj));
                insert_harmony_file(
                    &mut entries,
                    &mut total_size,
                    &mut file_count,
                    mj_rel,
                    mj_len,
                    None,
                    None,
                    None,
                    FolderEntrySource::Harmony(mj_idx),
                )?;

                for item in &mj_entries {
                    if cancel.is_cancelled() {
                        bail!("HarmonyOS backup unlock cancelled");
                    }
                    if item.file_name.is_empty() {
                        continue;
                    }
                    managed_disk_names.insert(item.file_name.clone());
                    let disk_path = mdir.join(&item.file_name);
                    if !disk_path.is_file() {
                        continue;
                    }
                    let disk_len = fs::metadata(&disk_path)?.len();
                    let is_tar = !item.is_big_file
                        && (item.file_name.ends_with(".tar")
                            || item
                                .information
                                .as_ref()
                                .is_some_and(|i| i.path.ends_with(".tar")));
                    if is_tar {
                        index_tar_into_entries(
                            &disk_path,
                            disk_len,
                            mod_cipher.as_ref().map(|(k, _, j0)| (*k, *j0)),
                            &mod_prefix,
                            &mut entries,
                            &mut backing_files,
                            &mut total_size,
                            &mut file_count,
                            cancel,
                        )?;
                    } else if let Some(info) = item.information.as_ref() {
                        let Some(rel_inner) = sanitize_rel_path(&info.path) else {
                            continue;
                        };
                        let plain_len = if let Some((key, _, j0)) = mod_cipher {
                            let max_plain = disk_len.saturating_sub(16);
                            let len = info
                                .stat
                                .as_ref()
                                .and_then(|s| s.st_size)
                                .map(|sz| sz.min(max_plain))
                                .unwrap_or(max_plain);
                            let idx = backing_files.len();
                            backing_files.push(HarmonyBacking::EncryptedRange {
                                path: disk_path,
                                key,
                                j0,
                                base_plain_offset: 0,
                                len,
                            });
                            idx
                        } else {
                            let len = info
                                .stat
                                .as_ref()
                                .and_then(|s| s.st_size)
                                .map(|sz| sz.min(disk_len))
                                .unwrap_or(disk_len);
                            let idx = backing_files.len();
                            backing_files.push(HarmonyBacking::PlainRange {
                                path: disk_path,
                                offset: 0,
                                len,
                            });
                            idx
                        };
                        let size = backing_files[plain_len].size();
                        let mtime = info
                            .stat
                            .as_ref()
                            .and_then(|s| s.st_mtim.as_ref())
                            .and_then(|t| t.tv_sec)
                            .and_then(|s| unix_time(Some(s)));
                        let atime = info
                            .stat
                            .as_ref()
                            .and_then(|s| s.st_atim.as_ref())
                            .and_then(|t| t.tv_sec)
                            .and_then(|s| unix_time(Some(s)));
                        let mode = info.stat.as_ref().and_then(|s| s.st_mode);
                        let rel = format!("{mod_prefix}/{rel_inner}");
                        insert_harmony_file(
                            &mut entries,
                            &mut total_size,
                            &mut file_count,
                            rel,
                            size,
                            mtime,
                            atime,
                            mode,
                            FolderEntrySource::Harmony(plain_len),
                        )?;
                    }
                }
            }

            // Index remaining unmanaged files in module dir (.png icons, part.*.tar without manage.json,
            // and SYSTEM *_sa.data blobs).
            if let Ok(rd) = fs::read_dir(&mdir) {
                for dir_ent in rd.flatten() {
                    let fname = dir_ent.file_name().to_string_lossy().into_owned();
                    if managed_disk_names.contains(&fname) {
                        continue;
                    }
                    let fpath = dir_ent.path();
                    if !fpath.is_file() {
                        continue;
                    }
                    let meta = fs::metadata(&fpath)?;
                    if fname.ends_with(".tar") {
                        index_tar_into_entries(
                            &fpath,
                            meta.len(),
                            mod_cipher.as_ref().map(|(k, _, j0)| (*k, *j0)),
                            &mod_prefix,
                            &mut entries,
                            &mut backing_files,
                            &mut total_size,
                            &mut file_count,
                            cancel,
                        )?;
                        continue;
                    }
                    let rel = format!("{mod_prefix}/{fname}");
                    if fname.ends_with(".png") || meta.len() < 16 {
                        insert_host_if_absent(
                            &mut entries,
                            &mut total_size,
                            &mut file_count,
                            rel,
                            &fpath,
                            &meta,
                        )?;
                        continue;
                    }
                    let Some((key, iv, j0)) = mod_cipher.as_ref() else {
                        insert_host_if_absent(
                            &mut entries,
                            &mut total_size,
                            &mut file_count,
                            rel,
                            &fpath,
                            &meta,
                        )?;
                        continue;
                    };
                    // Loose encrypted SYSTEM metadata (*_sa.data) vs plaintext JSON fallback (`3203_sa.data`).
                    let probe_verified = meta.len() <= 16 * 1024 * 1024
                        && fs::read(&fpath).ok().is_some_and(|raw| {
                            aes256_gcm_decrypt_and_verify(key, iv, &raw).is_ok()
                        });
                    if probe_verified {
                        let len = meta.len().saturating_sub(16);
                        let idx = backing_files.len();
                        backing_files.push(HarmonyBacking::EncryptedRange {
                            path: fpath,
                            key: *key,
                            j0: *j0,
                            base_plain_offset: 0,
                            len,
                        });
                        insert_harmony_file(
                            &mut entries,
                            &mut total_size,
                            &mut file_count,
                            rel,
                            len,
                            meta.modified().ok(),
                            meta.accessed().ok(),
                            None,
                            FolderEntrySource::Harmony(idx),
                        )?;
                    } else {
                        insert_host_if_absent(
                            &mut entries,
                            &mut total_size,
                            &mut file_count,
                            rel,
                            &fpath,
                            &meta,
                        )?;
                    }
                }
            }
        }

        let info = build_info(&raw, header, backup_type, false, Some(file_count));
        let label = info
            .device_model
            .clone()
            .or_else(|| info.product_model.clone())
            .unwrap_or_else(|| "HarmonyOS NEXT 备份".into());

        Ok(Self {
            label,
            root: root.to_path_buf(),
            entries,
            total_size,
            bak_archives: Vec::new(),
            ios_backup: None,
            harmony_backup: Some(HarmonyBackup {
                info,
                files: backing_files,
            }),
        })
    }

    pub fn harmony_info(&self) -> Option<&HarmonyBackupInfo> {
        self.harmony_backup.as_ref().map(|b| &b.info)
    }

    pub fn read_harmony_range(
        &self,
        rel: &str,
        offset: u64,
        length: usize,
    ) -> Result<Option<Vec<u8>>> {
        let entry = self.lookup(rel).context("folder entry not found")?;
        let Some(FolderEntrySource::Harmony(index)) = entry.source.as_ref() else {
            return Ok(None);
        };
        let backing = self
            .harmony_backup
            .as_ref()
            .and_then(|b| b.files.get(*index))
            .context("HarmonyOS file index out of range")?;
        Ok(Some(backing.read_range(offset, length)?))
    }

    pub(super) fn read_harmony(&self, index: usize, max_bytes: usize) -> Result<Vec<u8>> {
        let backing = self
            .harmony_backup
            .as_ref()
            .and_then(|b| b.files.get(index))
            .context("HarmonyOS file index out of range")?;
        backing.read_range(0, max_bytes)
    }

    pub(super) fn stream_harmony<F>(
        &self,
        index: usize,
        chunk_size: usize,
        mut visitor: F,
    ) -> Result<u64>
    where
        F: FnMut(&[u8]) -> Result<()>,
    {
        let backing = self
            .harmony_backup
            .as_ref()
            .and_then(|b| b.files.get(index))
            .context("HarmonyOS file index out of range")?;
        let step = chunk_size.max(64 * 1024);
        let total = backing.size();
        let mut offset = 0u64;
        while offset < total {
            let chunk = backing.read_range(offset, step)?;
            if chunk.is_empty() {
                break;
            }
            visitor(&chunk)?;
            offset += chunk.len() as u64;
        }
        Ok(offset)
    }
}

fn resolve_module_dir(root: &Path, module: &RawModuleInfo) -> Option<(String, PathBuf)> {
    let files_dir = root.join("files");
    if !files_dir.is_dir() {
        return None;
    }
    let mut candidates = Vec::new();
    let colon_replaced = module.qualified_name.replace(':', "_");
    if module.app_index > 0 {
        if !module.module_name.is_empty() {
            candidates.push(format!("{}_{}", module.module_name, module.app_index));
        }
        if !colon_replaced.is_empty() && !candidates.contains(&colon_replaced) {
            candidates.push(colon_replaced.clone());
        }
    }
    if !module.module_name.is_empty() && !candidates.contains(&module.module_name) {
        candidates.push(module.module_name.clone());
    }
    if !colon_replaced.is_empty() && !candidates.contains(&colon_replaced) {
        candidates.push(colon_replaced);
    }

    let mut sub_dirs = Vec::new();
    let typed = match module.module_type.as_str() {
        "NEXT_APP" | "APPLICATION" => Some("APPLICATION"),
        "BASIC" => Some("BASIC"),
        "SYSTEM" => Some("SYSTEM"),
        other if !other.is_empty() => Some(other),
        _ => None,
    };
    if let Some(t) = typed {
        sub_dirs.push(files_dir.join(t));
    }
    for std_sub in ["APPLICATION", "BASIC", "SYSTEM"] {
        let p = files_dir.join(std_sub);
        if !sub_dirs.contains(&p) {
            sub_dirs.push(p);
        }
    }
    if let Ok(rd) = fs::read_dir(&files_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() && !sub_dirs.contains(&p) {
                sub_dirs.push(p);
            }
        }
    }
    for sub in sub_dirs {
        for cand in &candidates {
            if cand.contains(['/', '\\']) || matches!(cand.as_str(), "." | "..") {
                continue;
            }
            let p = sub.join(cand);
            if p.is_dir() {
                return Some((cand.clone(), p));
            }
        }
    }
    None
}
