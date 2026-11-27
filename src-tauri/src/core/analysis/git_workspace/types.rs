use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteInfo {
    pub name: String,
    pub url: String,
    pub github_repo: Option<String>,
    pub credential_leak: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchRef {
    pub name: String,
    pub full_ref: String,
    pub sha: String,
    pub is_head: bool,
    pub is_remote: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTagRef {
    pub name: String,
    pub sha: String,
    pub target_sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub parents: Vec<String>,
    pub tree_sha: String,
    /// `reachable`, `rolled_back`, `dangling`, or `reset_target`
    pub status: String,
    pub status_label: String,
    pub author: String,
    pub committer: String,
    pub timestamp: Option<String>,
    pub timestamp_unix: i64,
    pub summary: String,
    pub message: String,
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitReflogInfo {
    pub ref_name: String,
    pub old_sha: String,
    pub new_sha: String,
    pub old_short: String,
    pub new_short: String,
    pub action_type: String,
    pub is_rollback: bool,
    pub actor: String,
    pub timestamp: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRecoveryItem {
    pub id: String,
    /// `dangling_commit`, `secret_blob`, or `config_leak`
    pub kind: String,
    pub title: String,
    pub object_sha: String,
    pub object_type: String,
    pub finding_kind: String,
    pub size: u64,
    pub preview: String,
    pub source_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    pub staged: bool,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSandboxState {
    pub initialized: bool,
    pub path: String,
    pub git_available: bool,
    pub dirty_count: usize,
    pub staged_count: usize,
    pub untracked_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoOverview {
    pub repo_path: String,
    pub repo_name: String,
    /// `evidence` or `sandbox`
    pub source_mode: String,
    pub head_ref: String,
    pub head_sha: Option<String>,
    pub is_detached: bool,
    pub remotes: Vec<GitRemoteInfo>,
    pub user_name: String,
    pub user_email: String,
    pub branches: Vec<GitBranchRef>,
    pub tags: Vec<GitTagRef>,
    pub commits: Vec<GitCommitInfo>,
    pub reflogs: Vec<GitReflogInfo>,
    pub recoveries: Vec<GitRecoveryItem>,
    pub status_entries: Vec<GitStatusEntry>,
    pub index_count: usize,
    pub sandbox: GitSandboxState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTreeEntry {
    pub name: String,
    pub path: String,
    /// `dir`, `file`, `symlink`, or `submodule`
    pub kind: String,
    pub mode: String,
    pub sha: Option<String>,
    pub size: Option<u64>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitReadmePreview {
    pub path: String,
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTreeListing {
    pub repo_path: String,
    pub source_mode: String,
    pub rev: String,
    pub resolved_sha: Option<String>,
    pub dir_path: String,
    pub entries: Vec<GitTreeEntry>,
    pub readme: Option<GitReadmePreview>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileContent {
    pub path: String,
    pub name: String,
    pub rev: String,
    pub blob_sha: Option<String>,
    pub size: u64,
    /// `text`, `image`, or `binary`
    pub kind: String,
    pub mime: String,
    pub encoding: Option<String>,
    pub text: Option<String>,
    pub image_data_url: Option<String>,
    pub hex_preview: Option<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffLine {
    /// `context`, `add`, or `delete`
    pub kind: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub header: String,
    pub lines: Vec<GitDiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffFile {
    pub path: String,
    /// `added`, `modified`, `deleted`, or `binary`
    pub status: String,
    pub old_sha: Option<String>,
    pub new_sha: Option<String>,
    pub additions: usize,
    pub deletions: usize,
    pub is_binary: bool,
    pub hunks: Vec<GitDiffHunk>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffResult {
    pub repo_path: String,
    pub source_mode: String,
    pub base_rev: String,
    pub target_rev: String,
    pub base_sha: Option<String>,
    pub target_sha: Option<String>,
    pub total_additions: usize,
    pub total_deletions: usize,
    pub files: Vec<GitDiffFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitExportResult {
    pub destination: String,
    pub exported_files: usize,
    pub exported_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSandboxActionResult {
    pub ok: bool,
    pub action: String,
    pub stdout: String,
    pub stderr: String,
    pub sandbox: GitSandboxState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSandboxFsResult {
    pub ok: bool,
    pub action: String,
    pub target_path: String,
    pub sandbox: GitSandboxState,
}
