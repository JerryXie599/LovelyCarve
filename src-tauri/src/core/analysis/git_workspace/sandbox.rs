use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use sha1::{Digest as _, Sha1};

use super::objects::{
    join_rel, parse_commit_object, parse_git_index, walk_worktree_files, EvidenceViewBackend,
    GitFsBackend, GitObjectKind, GitObjectStore, LocalFsBackend, MAX_OBJECT_BYTES,
};
use super::types::{
    GitExportResult, GitSandboxActionResult, GitSandboxFsResult, GitSandboxState, GitStatusEntry,
};
use crate::core::extensions::plugin::HostView;

const MAX_EXPORT_FILES: usize = 25_000;
const MAX_SANDBOX_FILES: usize = 25_000;
const MAX_SANDBOX_BYTES: u64 = 512 * 1024 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static GIT_AVAILABLE: OnceLock<bool> = OnceLock::new();

pub fn is_git_cli_available() -> bool {
    *GIT_AVAILABLE.get_or_init(|| {
        let mut cmd = Command::new("git");
        cmd.arg("--version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
}

pub fn sandbox_root_for(case_dir: &Path, image_id: &str, repo_path: &str) -> PathBuf {
    let norm = normalize_repo_path(repo_path);
    let mut h = Sha1::new();
    h.update(image_id.as_bytes());
    h.update(b"::");
    h.update(norm.as_bytes());
    let hash12 = &hex::encode(h.finalize())[..12];

    let img_prefix: String = image_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(24)
        .collect();
    let dir_name = if img_prefix.is_empty() {
        format!("repo_{hash12}")
    } else {
        format!("{img_prefix}_{hash12}")
    };
    case_dir.join("git-sandboxes").join(dir_name)
}

pub fn normalize_repo_path(repo_path: &str) -> String {
    let norm = repo_path
        .trim()
        .trim_matches(['/', '\\'])
        .replace('\\', "/");
    norm.strip_suffix("/.git")
        .unwrap_or(&norm)
        .trim_end_matches('/')
        .to_owned()
}

pub fn resolve_git_dir(fs: &dyn GitFsBackend, repo_norm: &str) -> Result<String> {
    if let Some(dot_git) = fs.stat(".git")? {
        if dot_git.kind == "dir" {
            return Ok(".git".to_owned());
        }
        if dot_git.kind == "file" {
            if let Some(bytes) = fs.read(".git", 16 * 1024)? {
                let text = String::from_utf8_lossy(&bytes);
                if let Some(rest) = text.trim().strip_prefix("gitdir:") {
                    let target = rest.trim().replace('\\', "/");
                    if !target.is_empty() {
                        return Ok(target);
                    }
                }
            }
        }
    }
    if fs.stat("HEAD")?.is_some() && fs.stat("objects")?.is_some() {
        return Ok(String::new());
    }
    Err(anyhow!("未在路径下检测到 .git 仓库: {repo_norm}"))
}

pub fn is_sandbox_initialized(sandbox_dir: &Path) -> bool {
    sandbox_dir.join(".git").exists()
        || (sandbox_dir.join("HEAD").is_file() && sandbox_dir.join("objects").is_dir())
}

pub fn sandbox_state(sandbox_dir: &Path) -> GitSandboxState {
    let initialized = is_sandbox_initialized(sandbox_dir);
    let git_available = is_git_cli_available();
    let mut dirty_count = 0usize;
    let mut staged_count = 0usize;
    let mut untracked_count = 0usize;

    if initialized && git_available {
        if let Ok(entries) = sandbox_git_status(sandbox_dir) {
            dirty_count = entries.len();
            staged_count = entries.iter().filter(|e| e.staged).count();
            untracked_count = entries.iter().filter(|e| e.kind == "untracked").count();
        }
    }

    GitSandboxState {
        initialized,
        path: sandbox_dir.to_string_lossy().into_owned(),
        git_available,
        dirty_count,
        staged_count,
        untracked_count,
    }
}

pub fn sandbox_git_status(sandbox_dir: &Path) -> Result<Vec<GitStatusEntry>> {
    let (ok, stdout, _) = run_git_cmd(sandbox_dir, &["status", "--porcelain=v1", "-uall"], false)?;
    if !ok {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = (bytes[0] as char).to_string();
        let y = (bytes[1] as char).to_string();
        let raw_path = line[3..].trim();
        let path = if let Some((_, new_p)) = raw_path.split_once(" -> ") {
            unquote_git_path(new_p)
        } else {
            unquote_git_path(raw_path)
        };
        let staged = x != " " && x != "?";
        let kind = if x == "?" && y == "?" {
            "untracked"
        } else if x == "D" || y == "D" {
            "deleted"
        } else if x == "A" || y == "A" {
            "added"
        } else if x == "R" || y == "R" {
            "renamed"
        } else {
            "modified"
        };
        entries.push(GitStatusEntry {
            path,
            index_status: x,
            worktree_status: y,
            staged,
            kind: kind.to_owned(),
        });
    }
    Ok(entries)
}

fn unquote_git_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_owned()
    }
}

pub fn ensure_sandbox(
    view: &HostView,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    force_reset: bool,
) -> Result<PathBuf> {
    let sandbox_dir = sandbox_root_for(case_dir, image_id, repo_path);
    if !force_reset && is_sandbox_initialized(&sandbox_dir) {
        return Ok(sandbox_dir);
    }

    if sandbox_dir.exists() {
        let parent = case_dir.join("git-sandboxes");
        if !sandbox_dir.starts_with(&parent) {
            return Err(anyhow!("unsafe sandbox path"));
        }
        std::fs::remove_dir_all(&sandbox_dir)
            .with_context(|| format!("清理沙盒目录失败: {}", sandbox_dir.display()))?;
    }
    std::fs::create_dir_all(&sandbox_dir)?;

    let repo_norm = normalize_repo_path(repo_path);
    let backend = EvidenceViewBackend::new(view, &repo_norm);
    let _ = resolve_git_dir(&backend, &repo_norm)?;

    let mut files_copied = 0usize;
    let mut bytes_copied = 0u64;
    copy_view_tree_to_disk(
        &backend,
        "",
        &sandbox_dir,
        true,
        &mut files_copied,
        &mut bytes_copied,
    )?;

    // Disable any hooks inside extracted `.git/hooks`
    let hooks_dir = sandbox_dir.join(".git").join("hooks");
    if hooks_dir.exists() {
        let disabled = sandbox_dir.join(".git").join("hooks.disabled");
        if disabled.exists() {
            let _ = std::fs::remove_dir_all(&disabled);
        }
        let _ = std::fs::rename(&hooks_dir, &disabled);
    }

    Ok(sandbox_dir)
}

fn copy_view_tree_to_disk(
    fs: &dyn GitFsBackend,
    rel_dir: &str,
    dest_dir: &Path,
    disable_hooks: bool,
    files_copied: &mut usize,
    bytes_copied: &mut u64,
) -> Result<()> {
    let mut stack = vec![(rel_dir.to_owned(), 0usize)];
    let mut visited = BTreeSet::new();
    while let Some((cur, depth)) = stack.pop() {
        if depth > 48 || !visited.insert(cur.to_ascii_lowercase()) {
            continue;
        }
        for node in fs.list(&cur)? {
            if node.deleted || node.kind == "link" {
                continue;
            }
            if !is_safe_segment(&node.name) {
                continue;
            }
            let child_rel = join_rel(&cur, &node.name);
            let mapped_rel = if disable_hooks && child_rel.eq_ignore_ascii_case(".git/hooks") {
                ".git/hooks.disabled".to_owned()
            } else if disable_hooks && child_rel.to_ascii_lowercase().starts_with(".git/hooks/") {
                format!(".git/hooks.disabled/{}", &child_rel[".git/hooks/".len()..])
            } else {
                child_rel.clone()
            };

            let target = safe_join(dest_dir, &mapped_rel)?;
            if node.kind == "dir" {
                std::fs::create_dir_all(&target)?;
                stack.push((child_rel, depth + 1));
            } else if node.kind == "file" {
                if *files_copied >= MAX_SANDBOX_FILES {
                    return Err(anyhow!(
                        "仓库文件数量超过沙盒安全上限 ({MAX_SANDBOX_FILES})"
                    ));
                }
                if bytes_copied.saturating_add(node.size) > MAX_SANDBOX_BYTES {
                    return Err(anyhow!("仓库体积超过沙盒安全上限 (512 MiB)"));
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if let Some(data) = fs.read(&child_rel, MAX_SANDBOX_BYTES)? {
                    *bytes_copied = bytes_copied.saturating_add(data.len() as u64);
                    *files_copied += 1;
                    std::fs::write(&target, data)?;
                }
            }
        }
    }
    Ok(())
}

fn run_git_cmd(
    sandbox_dir: &Path,
    args: &[&str],
    needs_identity: bool,
) -> Result<(bool, String, String)> {
    if !is_git_cli_available() {
        return Err(anyhow!(
            "系统未安装或未找到 git 命令行工具，无法执行交互式 Git 操作"
        ));
    }
    let mut cmd = Command::new("git");
    cmd.current_dir(sandbox_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .arg("-c")
        .arg("core.hooksPath=.git/.lc-nohooks")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.sshCommand=false")
        .arg("-c")
        .arg("core.pager=cat")
        .arg("-c")
        .arg("safe.directory=*");

    if needs_identity && !has_git_identity(sandbox_dir) {
        cmd.arg("-c")
            .arg("user.name=LovelyCarve Examiner")
            .arg("-c")
            .arg("user.email=forensics@lovelycarve.local");
    }

    for arg in args {
        cmd.arg(arg);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().context("执行 git 命令失败")?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((output.status.success(), stdout, stderr))
}

fn has_git_identity(sandbox_dir: &Path) -> bool {
    let mut cmd_name = Command::new("git");
    cmd_name.current_dir(sandbox_dir).args([
        "-c",
        "safe.directory=*",
        "config",
        "--get",
        "user.name",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd_name.creation_flags(CREATE_NO_WINDOW);
    }
    let has_name = cmd_name
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);

    let mut cmd_email = Command::new("git");
    cmd_email.current_dir(sandbox_dir).args([
        "-c",
        "safe.directory=*",
        "config",
        "--get",
        "user.email",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd_email.creation_flags(CREATE_NO_WINDOW);
    }
    let has_email = cmd_email
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);

    has_name && has_email
}

#[allow(clippy::too_many_arguments)]
pub fn execute_sandbox_git_action(
    view: &HostView,
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
    let force_reset = reset_from_evidence || action == "reset_evidence";
    let sandbox_dir = ensure_sandbox(view, case_dir, image_id, repo_path, force_reset)?;

    if action == "init" || action == "reset_evidence" {
        let state = sandbox_state(&sandbox_dir);
        return Ok(GitSandboxActionResult {
            ok: true,
            action: action.to_owned(),
            stdout: if action == "reset_evidence" {
                "已从原始镜像重新提取沙盒工作区".to_owned()
            } else {
                "沙盒工作区已就绪".to_owned()
            },
            stderr: String::new(),
            sandbox: state,
        });
    }

    let (ok, stdout, stderr) = match action {
        "checkout" => {
            let target = require_non_empty(target_ref, "targetRef")?;
            run_git_cmd(&sandbox_dir, &["checkout", target], false)?
        }
        "reset_hard" => {
            let target = require_non_empty(target_ref, "targetRef")?;
            run_git_cmd(&sandbox_dir, &["reset", "--hard", target], false)?
        }
        "reset_mixed" => {
            let target = require_non_empty(target_ref, "targetRef")?;
            run_git_cmd(&sandbox_dir, &["reset", "--mixed", target], false)?
        }
        "reset_soft" => {
            let target = require_non_empty(target_ref, "targetRef")?;
            run_git_cmd(&sandbox_dir, &["reset", "--soft", target], false)?
        }
        "revert" => {
            let target = require_non_empty(target_ref, "targetRef")?;
            run_git_cmd(&sandbox_dir, &["revert", "--no-edit", target], true)?
        }
        "create_branch" => {
            let branch = require_non_empty(branch_name, "branchName")?;
            if let Some(target) = target_ref.map(str::trim).filter(|s| !s.is_empty()) {
                run_git_cmd(&sandbox_dir, &["branch", branch, target], false)?
            } else {
                run_git_cmd(&sandbox_dir, &["checkout", "-b", branch], false)?
            }
        }
        "delete_branch" => {
            let branch = require_non_empty(branch_name, "branchName")?;
            run_git_cmd(&sandbox_dir, &["branch", "-D", branch], false)?
        }
        "stage" => {
            if file_paths.is_empty() {
                run_git_cmd(&sandbox_dir, &["add", "-A"], false)?
            } else {
                let mut args = vec!["add", "--"];
                for p in file_paths {
                    validate_worktree_rel_path(p)?;
                    args.push(p.as_str());
                }
                run_git_cmd(&sandbox_dir, &args, false)?
            }
        }
        "unstage" => {
            if file_paths.is_empty() {
                run_git_cmd(&sandbox_dir, &["reset", "HEAD"], false)?
            } else {
                let mut args = vec!["reset", "HEAD", "--"];
                for p in file_paths {
                    validate_worktree_rel_path(p)?;
                    args.push(p.as_str());
                }
                run_git_cmd(&sandbox_dir, &args, false)?
            }
        }
        "discard" => {
            if file_paths.is_empty() {
                let (ok1, out1, err1) = run_git_cmd(&sandbox_dir, &["checkout", "--", "."], false)?;
                let (ok2, out2, err2) = run_git_cmd(&sandbox_dir, &["clean", "-fd"], false)?;
                (ok1 && ok2, format!("{out1}{out2}"), format!("{err1}{err2}"))
            } else {
                let mut clean_msgs = Vec::new();
                let mut tracked = Vec::new();
                for p in file_paths {
                    let clean = validate_worktree_rel_path(p)?;
                    let full = safe_join(&sandbox_dir, &clean)?;
                    let (t_ok, t_out, _) =
                        run_git_cmd(&sandbox_dir, &["ls-files", "--", p.as_str()], false)?;
                    if t_ok && !t_out.trim().is_empty() {
                        tracked.push(p.as_str());
                    } else if full.exists() {
                        if full.is_dir() {
                            std::fs::remove_dir_all(&full)?;
                        } else {
                            std::fs::remove_file(&full)?;
                        }
                        clean_msgs.push(format!("Removed untracked {clean}"));
                    }
                }
                if tracked.is_empty() {
                    (true, clean_msgs.join("\n"), String::new())
                } else {
                    let mut args = vec!["checkout", "--"];
                    args.extend(tracked);
                    let (ok, out, err) = run_git_cmd(&sandbox_dir, &args, false)?;
                    (ok, format!("{out}{}", clean_msgs.join("\n")), err)
                }
            }
        }
        "commit" => {
            let msg = require_non_empty(commit_message, "commitMessage")?;
            run_git_cmd(&sandbox_dir, &["commit", "-m", msg], true)?
        }
        _ => return Err(anyhow!("不支持的 Git 操作: {action}")),
    };

    let state = sandbox_state(&sandbox_dir);
    Ok(GitSandboxActionResult {
        ok,
        action: action.to_owned(),
        stdout,
        stderr,
        sandbox: state,
    })
}

pub fn execute_sandbox_fs_action(
    view: &HostView,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    action: &str,
    target_path: &str,
    new_path: Option<&str>,
    content: Option<&str>,
) -> Result<GitSandboxFsResult> {
    let sandbox_dir = ensure_sandbox(view, case_dir, image_id, repo_path, false)?;

    let resolved_target = if action == "reveal" && target_path.trim().is_empty() {
        String::new()
    } else {
        validate_worktree_rel_path(target_path)?
    };
    let abs_target = if resolved_target.is_empty() {
        sandbox_dir.clone()
    } else {
        safe_join(&sandbox_dir, &resolved_target)?
    };

    match action {
        "mkdir" => {
            std::fs::create_dir_all(&abs_target)?;
        }
        "write_file" => {
            if let Some(parent) = abs_target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&abs_target, content.unwrap_or("").as_bytes())?;
        }
        "rename" => {
            let dst_rel = validate_worktree_rel_path(require_non_empty(new_path, "newPath")?)?;
            let abs_dst = safe_join(&sandbox_dir, &dst_rel)?;
            if let Some(parent) = abs_dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&abs_target, &abs_dst)?;
        }
        "delete" => {
            if abs_target == sandbox_dir {
                return Err(anyhow!("不能删除沙盒根目录"));
            }
            if abs_target.is_dir() {
                std::fs::remove_dir_all(&abs_target)?;
            } else if abs_target.exists() {
                std::fs::remove_file(&abs_target)?;
            }
        }
        "reveal" => {
            reveal_in_os_explorer(&abs_target)?;
        }
        _ => return Err(anyhow!("不支持的文件系统操作: {action}")),
    }

    Ok(GitSandboxFsResult {
        ok: true,
        action: action.to_owned(),
        target_path: resolved_target,
        sandbox: sandbox_state(&sandbox_dir),
    })
}

fn reveal_in_os_explorer(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        if path.is_file() {
            Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .context("启动资源管理器失败")?;
        } else {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .context("启动资源管理器失败")?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(path).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open").arg(dir).spawn()?;
    }
    Ok(())
}

pub fn export_from_backend(
    fs: &dyn GitFsBackend,
    git_dir: &str,
    rev: &str,
    sub_path: &str,
    destination: &Path,
    include_git_dir: bool,
) -> Result<GitExportResult> {
    let clean_sub = sanitize_rel_path(sub_path)?;
    std::fs::create_dir_all(destination)?;

    let mut exported_files = 0usize;
    let mut exported_bytes = 0u64;

    if rev.eq_ignore_ascii_case("WORKTREE") {
        let work_files = walk_worktree_files(fs, git_dir);
        if !clean_sub.is_empty() && work_files.contains_key(&clean_sub) {
            let file_name = clean_sub.rsplit('/').next().unwrap_or(&clean_sub);
            if let Some(bytes) = fs.read(&clean_sub, MAX_SANDBOX_BYTES)? {
                let target = safe_join(destination, file_name)?;
                exported_bytes += bytes.len() as u64;
                exported_files += 1;
                std::fs::write(target, bytes)?;
            }
        } else {
            let prefix = if clean_sub.is_empty() {
                String::new()
            } else {
                format!("{clean_sub}/")
            };
            for (rel, _) in work_files {
                let rel_out = if prefix.is_empty() {
                    rel.as_str()
                } else if let Some(stripped) = rel.strip_prefix(&prefix) {
                    stripped
                } else {
                    continue;
                };
                if exported_files >= MAX_EXPORT_FILES {
                    break;
                }
                if let Some(bytes) = fs.read(&rel, MAX_SANDBOX_BYTES)? {
                    let target = safe_join(destination, rel_out)?;
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    exported_bytes += bytes.len() as u64;
                    exported_files += 1;
                    std::fs::write(target, bytes)?;
                }
            }
            if include_git_dir && clean_sub.is_empty() && !git_dir.is_empty() {
                copy_view_tree_to_disk(
                    fs,
                    git_dir,
                    &destination.join(".git"),
                    true,
                    &mut exported_files,
                    &mut exported_bytes,
                )?;
            }
        }
        return Ok(GitExportResult {
            destination: destination.to_string_lossy().into_owned(),
            exported_files,
            exported_bytes,
        });
    }

    let mut store = GitObjectStore::load(fs, git_dir)?;
    if rev.eq_ignore_ascii_case("INDEX") {
        let index_bytes = fs
            .read(&join_rel(git_dir, "index"), MAX_OBJECT_BYTES)?
            .ok_or_else(|| anyhow!("仓库中不存在 .git/index"))?;
        let entries = parse_git_index(&index_bytes);
        let prefix = if clean_sub.is_empty() {
            String::new()
        } else {
            format!("{clean_sub}/")
        };
        for entry in entries {
            let rel_out = if clean_sub == entry.path {
                entry.path.rsplit('/').next().unwrap_or(&entry.path)
            } else if prefix.is_empty() {
                entry.path.as_str()
            } else if let Some(stripped) = entry.path.strip_prefix(&prefix) {
                stripped
            } else {
                continue;
            };
            if exported_files >= MAX_EXPORT_FILES {
                break;
            }
            if let Some(obj) = store.get(&entry.sha) {
                if obj.kind == GitObjectKind::Blob {
                    let target = safe_join(destination, rel_out)?;
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    exported_bytes += obj.body.len() as u64;
                    exported_files += 1;
                    std::fs::write(target, obj.body)?;
                }
            }
        }
        return Ok(GitExportResult {
            destination: destination.to_string_lossy().into_owned(),
            exported_files,
            exported_bytes,
        });
    }

    // Export commit / branch / tag snapshot
    let commit_sha = resolve_rev_to_commit_sha(fs, git_dir, &mut store, rev)?;
    let commit_obj = store
        .get(&commit_sha)
        .ok_or_else(|| anyhow!("找不到提交对象: {commit_sha}"))?;
    let parsed = parse_commit_object(&commit_obj.body)
        .ok_or_else(|| anyhow!("无效的提交对象: {commit_sha}"))?;
    let blobs = store.collect_tree_blobs(&parsed.tree_sha);
    let prefix = if clean_sub.is_empty() {
        String::new()
    } else {
        format!("{clean_sub}/")
    };

    for (path, (_, blob_sha)) in blobs {
        let rel_out = if clean_sub == path {
            path.rsplit('/').next().unwrap_or(&path)
        } else if prefix.is_empty() {
            path.as_str()
        } else if let Some(stripped) = path.strip_prefix(&prefix) {
            stripped
        } else {
            continue;
        };
        if exported_files >= MAX_EXPORT_FILES {
            break;
        }
        if let Some(obj) = store.get(&blob_sha) {
            if obj.kind == GitObjectKind::Blob {
                let target = safe_join(destination, rel_out)?;
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                exported_bytes += obj.body.len() as u64;
                exported_files += 1;
                std::fs::write(target, obj.body)?;
            }
        }
    }

    if include_git_dir && clean_sub.is_empty() && !git_dir.is_empty() {
        copy_view_tree_to_disk(
            fs,
            git_dir,
            &destination.join(".git"),
            true,
            &mut exported_files,
            &mut exported_bytes,
        )?;
    }

    Ok(GitExportResult {
        destination: destination.to_string_lossy().into_owned(),
        exported_files,
        exported_bytes,
    })
}

pub fn resolve_rev_to_commit_sha(
    fs: &dyn GitFsBackend,
    git_dir: &str,
    store: &mut GitObjectStore<'_>,
    rev: &str,
) -> Result<String> {
    let trimmed = rev.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("HEAD") {
        let refs = super::objects::collect_repo_refs(fs, git_dir, store)?;
        return refs
            .head_sha
            .ok_or_else(|| anyhow!("当前仓库 HEAD 未指向有效提交"));
    }

    let refs = super::objects::collect_repo_refs(fs, git_dir, store)?;
    if let Some(b) = refs
        .branches
        .iter()
        .find(|b| b.name == trimmed || b.full_ref == trimmed)
    {
        return Ok(b.sha.clone());
    }
    if let Some(t) = refs
        .tags
        .iter()
        .find(|t| t.name == trimmed || format!("refs/tags/{}", t.name) == trimmed)
    {
        return Ok(t.target_sha.clone());
    }

    if let Some(full) = store.resolve_prefix(trimmed) {
        if let Some(commit_sha) = store.peel_to_commit(&full) {
            return Ok(commit_sha);
        }
        return Ok(full);
    }

    Err(anyhow!("无法解析引用或提交: {rev}"))
}

pub fn select_backend<'a>(
    view: &'a HostView,
    case_dir: &Path,
    image_id: &str,
    repo_path: &str,
    use_sandbox: bool,
) -> Result<(Box<dyn GitFsBackend + 'a>, String, String, GitSandboxState)> {
    let sb_dir = sandbox_root_for(case_dir, image_id, repo_path);
    let sb_state = sandbox_state(&sb_dir);
    if use_sandbox {
        let sb_dir = ensure_sandbox(view, case_dir, image_id, repo_path, false)?;
        let sb_state = sandbox_state(&sb_dir);
        let backend = LocalFsBackend::new(sb_dir);
        let git_dir = resolve_git_dir(&backend, repo_path)?;
        return Ok((Box::new(backend), git_dir, "sandbox".to_owned(), sb_state));
    }
    let repo_norm = normalize_repo_path(repo_path);
    let backend = EvidenceViewBackend::new(view, &repo_norm);
    let git_dir = resolve_git_dir(&backend, &repo_norm)?;
    Ok((Box::new(backend), git_dir, "evidence".to_owned(), sb_state))
}

fn require_non_empty<'a>(val: Option<&'a str>, field: &str) -> Result<&'a str> {
    val.map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("参数 {field} 不能为空"))
}

fn sanitize_rel_path(raw: &str) -> Result<String> {
    let mut parts = Vec::new();
    let normalized = raw.replace('\\', "/");
    for seg in normalized.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if !is_safe_segment(seg) {
            return Err(anyhow!("非法路径段: {seg}"));
        }
        parts.push(seg);
    }
    Ok(parts.join("/"))
}

pub fn validate_worktree_rel_path(raw: &str) -> Result<String> {
    let clean = sanitize_rel_path(raw)?;
    if clean.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if clean.split('/').any(|seg| seg.eq_ignore_ascii_case(".git")) {
        return Err(anyhow!("不允许直接修改 .git 内部目录"));
    }
    Ok(clean)
}

fn is_safe_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg.contains('\0')
        && !seg.contains(':')
        && !seg.contains('/')
        && !seg.contains('\\')
}

fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return Err(anyhow!("非法相对路径: {rel}")),
        }
    }
    Ok(out)
}
