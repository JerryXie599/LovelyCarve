<template>
  <div class="git-changes-panel">
    <!-- Sandbox Control Card -->
    <section class="ui-card gch-sandbox-card">
      <div class="gch-sb-info">
        <div class="gch-sb-title">
          <Terminal :size="15" />
          <strong>{{ tr("交互式 Git 沙盒工作区") }}</strong>
          <span class="ui-badge sm" :class="overview.sandbox.initialized ? 'success' : ''">
            {{ overview.sandbox.initialized ? tr("已初始化") : tr("未初始化（将在首次操作时自动创建）") }}
          </span>
        </div>
        <div class="gch-sb-path mono">{{ overview.sandbox.path }}</div>
        <div v-if="!overview.sandbox.gitAvailable" class="ui-alert warn">
          {{ tr("未检测到系统 git 命令行工具，仅可进行文件系统读写，Git 提交/回滚/分支操作不可用。") }}
        </div>
      </div>

      <div class="gch-sb-actions">
        <button
          v-if="!overview.sandbox.initialized"
          type="button"
          class="ui-btn sm primary"
          :disabled="busy"
          @click="runGit('init')"
        >
          <Play :size="13" />
          <span>{{ tr("立即初始化沙盒") }}</span>
        </button>

        <button
          type="button"
          class="ui-btn sm"
          :disabled="busy"
          @click="revealSandbox"
        >
          <FolderOpen :size="13" />
          <span>{{ tr("在资源管理器打开") }}</span>
        </button>

        <button
          v-if="overview.sandbox.initialized"
          type="button"
          class="ui-btn sm danger-soft"
          :disabled="busy"
          @click="resetSandboxToEvidence"
        >
          <RotateCcw :size="13" />
          <span>{{ tr("重置为镜像原始状态") }}</span>
        </button>
      </div>
    </section>

    <div class="gch-grid">
      <!-- Branch Manager -->
      <section class="ui-card gch-col">
        <header class="gch-col-head">
          <h3><GitBranch :size="14" /> {{ tr("分支管理") }}</h3>
          <span class="ui-badge sm">{{ overview.branches.length }}</span>
        </header>

        <form class="gch-branch-create" @submit.prevent="createBranch">
          <input
            v-model="newBranch"
            class="ui-input sm mono"
            :placeholder="tr('新分支名称…')"
          />
          <button type="submit" class="ui-btn sm" :disabled="busy || !newBranch.trim()">
            <GitBranchPlus :size="13" />
            <span>{{ tr("新建分支") }}</span>
          </button>
        </form>

        <ul class="gch-branch-list">
          <li v-for="b in overview.branches" :key="b.fullRef" class="gch-branch-row">
            <div class="gch-b-info">
              <span class="gch-b-name mono">{{ b.name }}</span>
              <span v-if="b.isHead" class="ui-badge sm success">HEAD</span>
              <span v-if="b.isRemote" class="ui-badge sm">remote</span>
              <span class="gch-b-sha mono">{{ shortSha(b.sha) }}</span>
            </div>
            <div class="gch-b-ops">
              <button
                v-if="!b.isHead"
                type="button"
                class="ui-btn sm"
                :disabled="busy"
                @click="runGit('checkout', { targetRef: b.isRemote ? b.fullRef : b.name })"
              >
                {{ tr("切换") }}
              </button>
              <button
                v-if="!b.isHead && !b.isRemote"
                type="button"
                class="ui-icon-btn sm danger-soft"
                :title="tr('删除分支')"
                :disabled="busy"
                @click="deleteBranch(b.name)"
              >
                <Trash2 :size="12" />
              </button>
            </div>
          </li>
        </ul>
      </section>

      <!-- Working Tree Changes + Commit Workbench -->
      <section class="ui-card gch-col">
        <header class="gch-col-head">
          <h3><GitCommitHorizontal :size="14" /> {{ tr("变更暂存与提交") }}</h3>
          <div class="gch-head-ops">
            <button
              type="button"
              class="ui-btn sm"
              :disabled="busy || overview.statusEntries.length === 0"
              @click="runGit('stage', { filePaths: [] })"
            >
              <Check :size="12" />
              <span>{{ tr("全部暂存") }}</span>
            </button>
            <button
              type="button"
              class="ui-btn sm"
              :disabled="busy || stagedEntries.length === 0"
              @click="runGit('unstage', { filePaths: [] })"
            >
              <Minus :size="12" />
              <span>{{ tr("全部取消暂存") }}</span>
            </button>
          </div>
        </header>

        <div v-if="overview.statusEntries.length === 0" class="gch-empty">
          {{ tr("工作区干净，暂无未提交的文件修改") }}
        </div>

        <ul v-else class="gch-status-list">
          <li v-for="s in overview.statusEntries" :key="s.path" class="gch-status-row">
            <span class="ui-badge sm mono" :class="statusTone(s)">
              {{ s.staged ? tr("已暂存") : s.kind }}
            </span>
            <span class="gch-s-path mono">{{ s.path }}</span>
            <div class="gch-s-ops">
              <button
                v-if="!s.staged"
                type="button"
                class="ui-btn link sm"
                :disabled="busy"
                @click="runGit('stage', { filePaths: [s.path] })"
              >
                {{ tr("暂存") }}
              </button>
              <button
                v-else
                type="button"
                class="ui-btn link sm"
                :disabled="busy"
                @click="runGit('unstage', { filePaths: [s.path] })"
              >
                {{ tr("取消暂存") }}
              </button>
              <button
                type="button"
                class="ui-btn link sm danger-soft"
                :disabled="busy"
                @click="discardFile(s.path)"
              >
                {{ tr("放弃") }}
              </button>
            </div>
          </li>
        </ul>

        <form class="gch-commit-form" @submit.prevent="submitCommit">
          <textarea
            v-model="commitMsg"
            rows="3"
            class="ui-textarea gch-commit-input"
            :placeholder="tr('输入提交说明 (Commit Message)…')"
          />
          <button
            type="submit"
            class="ui-btn sm primary"
            :disabled="busy || !commitMsg.trim() || stagedEntries.length === 0"
          >
            <Loader2 v-if="busy" :size="13" class="spin" />
            <GitCommitHorizontal v-else :size="13" />
            <span>{{ tr("提交到沙盒仓库 ({0} 个已暂存)", { 0: stagedEntries.length }) }}</span>
          </button>
        </form>
      </section>
    </div>

    <!-- Live Worktree Diff against HEAD -->
    <section v-if="overview.statusEntries.length > 0" class="gch-diff-wrap">
      <GitDiffViewer :diff="worktreeDiff" :loading="diffLoading" :error="diffError" />
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import {
  Check,
  FolderOpen,
  GitBranch,
  GitBranchPlus,
  GitCommitHorizontal,
  Loader2,
  Minus,
  Play,
  RotateCcw,
  Terminal,
  Trash2,
} from 'lucide-vue-next';
import { confirm } from '../../../composables/useConfirm';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceDiff,
  gitWorkspaceSandboxFs,
  gitWorkspaceSandboxGit,
  type GitDiffResult,
  type GitRepoOverview,
  type GitSandboxAction,
  type GitStatusEntry,
} from '../../../lib/ipc/gitWorkspace';
import { shortSha } from '../model/pluginTools';
import GitDiffViewer from './GitDiffViewer.vue';

const props = defineProps<{
  imageId: ImageId;
  overview: GitRepoOverview;
  useSandbox: boolean;
}>();

const emit = defineEmits<{
  sandboxMutated: [];
}>();

const busy = ref(false);
const newBranch = ref('');
const commitMsg = ref('');
const worktreeDiff = shallowRef<GitDiffResult | null>(null);
const diffLoading = ref(false);
const diffError = ref<string | null>(null);

const stagedEntries = computed(() => props.overview.statusEntries.filter(s => s.staged));

let diffSeq = 0;

watch(
  [() => props.overview.repoPath, () => props.useSandbox, () => props.overview.statusEntries.length],
  async ([repoPath, useSandbox, count]) => {
    if (count === 0) {
      worktreeDiff.value = null;
      return;
    }
    const seq = ++diffSeq;
    diffLoading.value = true;
    diffError.value = null;
    try {
      const res = await gitWorkspaceDiff({
        imageId: props.imageId,
        repoPath,
        targetRev: 'WORKTREE',
        baseRev: 'HEAD',
        useSandbox,
      });
      if (seq === diffSeq) worktreeDiff.value = res;
    } catch (err) {
      if (seq === diffSeq) diffError.value = String(err);
    } finally {
      if (seq === diffSeq) diffLoading.value = false;
    }
  },
  { immediate: true },
);

async function runGit(
  action: GitSandboxAction,
  extra: { targetRef?: string; branchName?: string; commitMessage?: string; filePaths?: string[] } = {},
) {
  busy.value = true;
  try {
    const res = await gitWorkspaceSandboxGit({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action,
      ...extra,
    });
    if (!res.ok) {
      notifyError(tr("Git 操作失败"), res.stderr || res.stdout || tr("未知错误"));
      return false;
    }
    notifySuccess(tr("Git 操作成功"), res.stdout || action);
    emit('sandboxMutated');
    return true;
  } catch (err) {
    notifyError(tr("Git 操作失败"), String(err));
    return false;
  } finally {
    busy.value = false;
  }
}

async function createBranch() {
  const name = newBranch.value.trim();
  if (!name) return;
  const ok = await runGit('create_branch', { branchName: name });
  if (ok) newBranch.value = '';
}

async function deleteBranch(name: string) {
  const ok = await confirm({
    title: tr("删除分支"),
    message: tr("将在沙盒中强制删除分支「{0}」，是否继续？", { 0: name }),
    confirmText: tr("删除"),
    danger: true,
  });
  if (!ok) return;
  await runGit('delete_branch', { branchName: name });
}

async function discardFile(path: string) {
  const ok = await confirm({
    title: tr("放弃工作区修改"),
    message: tr("将丢弃沙盒中「{0}」的未提交修改，是否继续？", { 0: path }),
    confirmText: tr("放弃"),
    danger: true,
  });
  if (!ok) return;
  await runGit('discard', { filePaths: [path] });
}

async function submitCommit() {
  const msg = commitMsg.value.trim();
  if (!msg) return;
  const ok = await runGit('commit', { commitMessage: msg });
  if (ok) commitMsg.value = '';
}

async function revealSandbox() {
  try {
    await gitWorkspaceSandboxFs({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'reveal',
      targetPath: '',
    });
    emit('sandboxMutated');
  } catch (err) {
    notifyError(tr("打开目录失败"), String(err));
  }
}

async function resetSandboxToEvidence() {
  const ok = await confirm({
    title: tr("重置沙盒工作区"),
    message: tr("将清空当前沙盒中的所有修改和提交，重新从原始镜像提取干净仓库，是否继续？"),
    confirmText: tr("重置"),
    danger: true,
  });
  if (!ok) return;
  await runGit('reset_evidence');
}

function statusTone(s: GitStatusEntry): string {
  if (s.staged) return 'success';
  if (s.kind === 'deleted') return 'danger';
  if (s.kind === 'modified') return 'warn';
  return 'info';
}
</script>

<style scoped>
.git-changes-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.gch-sandbox-card {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
}

.gch-sb-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.gch-sb-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.gch-sb-path {
  font-size: 11px;
  color: var(--text-secondary);
  word-break: break-all;
}

.gch-sb-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.gch-grid {
  display: grid;
  grid-template-columns: 1fr 1.25fr;
  gap: 12px;
}

@media (max-width: 960px) {
  .gch-grid {
    grid-template-columns: 1fr;
  }
}

.gch-col {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 14px 16px;
}

.gch-col-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.gch-col-head h3 {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.gch-head-ops {
  display: flex;
  gap: 4px;
}

.gch-branch-create {
  display: flex;
  gap: 6px;
}

.gch-branch-create input {
  flex: 1;
}

.gch-branch-list,
.gch-status-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
  max-height: 260px;
  overflow-y: auto;
}

.gch-branch-row,
.gch-status-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-primary);
  font-size: 12px;
}

.gch-b-info {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.gch-b-name,
.gch-s-path {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gch-s-path {
  flex: 1;
  min-width: 0;
}

.gch-b-sha {
  font-size: 11px;
  color: var(--text-secondary);
}

.gch-b-ops,
.gch-s-ops {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.gch-empty {
  padding: 18px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.gch-commit-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: auto;
}

.gch-commit-input {
  width: 100%;
  resize: vertical;
  padding: 8px 10px;
  font-size: 12px;
}

.gch-diff-wrap {
  display: flex;
  flex-direction: column;
}
</style>
