// Git / GitHub repository interactive workspace IPC wrappers.
import { invokeCmd } from './client';
import type { ImageId } from './ids';

export interface GitRemoteInfo {
  name: string;
  url: string;
  githubRepo: string | null;
  credentialLeak: string | null;
}

export interface GitBranchRef {
  name: string;
  fullRef: string;
  sha: string;
  isHead: boolean;
  isRemote: boolean;
}

export interface GitTagRef {
  name: string;
  sha: string;
  targetSha: string;
}

export interface GitCommitInfo {
  sha: string;
  shortSha: string;
  parents: string[];
  treeSha: string;
  status: 'reachable' | 'rolled_back' | 'dangling' | 'reset_target' | string;
  statusLabel: string;
  author: string;
  committer: string;
  timestamp: string | null;
  timestampUnix: number;
  summary: string;
  message: string;
  refs: string[];
}

export interface GitReflogInfo {
  refName: string;
  oldSha: string;
  newSha: string;
  oldShort: string;
  newShort: string;
  actionType: string;
  isRollback: boolean;
  actor: string;
  timestamp: string | null;
  detail: string;
}

export interface GitRecoveryItem {
  id: string;
  kind: 'dangling_commit' | 'secret_blob' | 'config_leak' | string;
  title: string;
  objectSha: string;
  objectType: string;
  findingKind: string;
  size: number;
  preview: string;
  sourcePath: string | null;
}

export interface GitStatusEntry {
  path: string;
  indexStatus: string;
  worktreeStatus: string;
  staged: boolean;
  kind: 'modified' | 'added' | 'deleted' | 'untracked' | 'renamed' | string;
}

export interface GitSandboxState {
  initialized: boolean;
  path: string;
  gitAvailable: boolean;
  dirtyCount: number;
  stagedCount: number;
  untrackedCount: number;
}

export interface GitRepoOverview {
  repoPath: string;
  repoName: string;
  sourceMode: 'evidence' | 'sandbox' | string;
  headRef: string;
  headSha: string | null;
  isDetached: boolean;
  remotes: GitRemoteInfo[];
  userName: string;
  userEmail: string;
  branches: GitBranchRef[];
  tags: GitTagRef[];
  commits: GitCommitInfo[];
  reflogs: GitReflogInfo[];
  recoveries: GitRecoveryItem[];
  statusEntries: GitStatusEntry[];
  indexCount: number;
  sandbox: GitSandboxState;
}

export interface GitTreeEntry {
  name: string;
  path: string;
  kind: 'dir' | 'file' | 'symlink' | 'submodule' | string;
  mode: string;
  sha: string | null;
  size: number | null;
  status: string | null;
}

export interface GitReadmePreview {
  path: string;
  name: string;
  content: string;
}

export interface GitTreeListing {
  repoPath: string;
  sourceMode: string;
  rev: string;
  resolvedSha: string | null;
  dirPath: string;
  entries: GitTreeEntry[];
  readme: GitReadmePreview | null;
}

export interface GitFileContent {
  path: string;
  name: string;
  rev: string;
  blobSha: string | null;
  size: number;
  kind: 'text' | 'image' | 'binary' | string;
  mime: string;
  encoding: string | null;
  text: string | null;
  imageDataUrl: string | null;
  hexPreview: string | null;
  truncated: boolean;
}

export interface GitDiffLine {
  kind: 'context' | 'add' | 'delete' | string;
  oldLine: number | null;
  newLine: number | null;
  content: string;
}

export interface GitDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  header: string;
  lines: GitDiffLine[];
}

export interface GitDiffFile {
  path: string;
  status: 'added' | 'modified' | 'deleted' | 'binary' | string;
  oldSha: string | null;
  newSha: string | null;
  additions: number;
  deletions: number;
  isBinary: boolean;
  hunks: GitDiffHunk[];
}

export interface GitDiffResult {
  repoPath: string;
  sourceMode: string;
  baseRev: string;
  targetRev: string;
  baseSha: string | null;
  targetSha: string | null;
  totalAdditions: number;
  totalDeletions: number;
  files: GitDiffFile[];
}

export interface GitExportResult {
  destination: string;
  exportedFiles: number;
  exportedBytes: number;
}

export type GitSandboxAction =
  | 'init'
  | 'reset_evidence'
  | 'checkout'
  | 'reset_hard'
  | 'reset_mixed'
  | 'reset_soft'
  | 'revert'
  | 'create_branch'
  | 'delete_branch'
  | 'stage'
  | 'unstage'
  | 'discard'
  | 'commit';

export interface GitSandboxActionResult {
  ok: boolean;
  action: string;
  stdout: string;
  stderr: string;
  sandbox: GitSandboxState;
}

export type GitSandboxFsAction = 'mkdir' | 'write_file' | 'rename' | 'delete' | 'reveal';

export interface GitSandboxFsResult {
  ok: boolean;
  action: string;
  targetPath: string;
  sandbox: GitSandboxState;
}

export function gitWorkspaceOverview(args: {
  imageId: ImageId;
  repoPath: string;
  useSandbox?: boolean;
}): Promise<GitRepoOverview> {
  return invokeCmd<GitRepoOverview>('git_workspace_overview', args);
}

export function gitWorkspaceTree(args: {
  imageId: ImageId;
  repoPath: string;
  rev?: string;
  dirPath?: string;
  useSandbox?: boolean;
}): Promise<GitTreeListing> {
  return invokeCmd<GitTreeListing>('git_workspace_tree', args);
}

export function gitWorkspaceFileRead(args: {
  imageId: ImageId;
  repoPath: string;
  rev?: string;
  filePath?: string;
  blobSha?: string | null;
  useSandbox?: boolean;
}): Promise<GitFileContent> {
  return invokeCmd<GitFileContent>('git_workspace_file_read', args);
}

export function gitWorkspaceDiff(args: {
  imageId: ImageId;
  repoPath: string;
  targetRev: string;
  baseRev?: string | null;
  useSandbox?: boolean;
}): Promise<GitDiffResult> {
  return invokeCmd<GitDiffResult>('git_workspace_diff', args);
}

export function gitWorkspaceExport(args: {
  imageId: ImageId;
  repoPath: string;
  rev?: string;
  subPath?: string;
  destination: string;
  includeGitDir?: boolean;
  useSandbox?: boolean;
}): Promise<GitExportResult> {
  return invokeCmd<GitExportResult>('git_workspace_export', args);
}

export function gitWorkspaceSandboxGit(args: {
  imageId: ImageId;
  repoPath: string;
  action: GitSandboxAction;
  targetRef?: string;
  branchName?: string;
  commitMessage?: string;
  filePaths?: string[];
  resetFromEvidence?: boolean;
}): Promise<GitSandboxActionResult> {
  return invokeCmd<GitSandboxActionResult>('git_workspace_sandbox_git', args);
}

export function gitWorkspaceSandboxFs(args: {
  imageId: ImageId;
  repoPath: string;
  action: GitSandboxFsAction;
  targetPath: string;
  newPath?: string;
  content?: string;
}): Promise<GitSandboxFsResult> {
  return invokeCmd<GitSandboxFsResult>('git_workspace_sandbox_fs', args);
}
