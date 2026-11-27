<template>
  <div class="git-commits-panel">
    <!-- Left Commit List -->
    <aside class="gcp-list-pane ui-card">
      <div class="gcp-filters">
        <div class="gcp-chips">
          <button
            v-for="f in filterTabs"
            :key="f.id"
            type="button"
            class="ui-btn sm ghost"
            :class="{ active: statusFilter === f.id }"
            @click="statusFilter = f.id"
          >
            {{ f.label }} ({{ f.count }})
          </button>
        </div>
        <label class="ui-search gcp-search">
          <Search :size="13" />
          <input v-model="query" :placeholder="tr('搜索提交说明、作者或 SHA…')" />
          <button v-if="query" type="button" class="ui-icon-btn sm" @click="query = ''">
            <X :size="11" />
          </button>
        </label>
      </div>

      <div class="gcp-scroll">
        <div v-if="filteredCommits.length === 0" class="gcp-empty">
          {{ tr("没有匹配的提交记录") }}
        </div>
        <button
          v-for="c in filteredCommits"
          :key="c.sha"
          type="button"
          class="gcp-commit-row"
          :class="{ selected: selectedCommit?.sha === c.sha }"
          @click="selectedSha = c.sha"
        >
          <div class="gcp-row-top">
            <span class="gcp-row-sha mono">{{ c.shortSha }}</span>
            <span
              class="ui-badge sm"
              :class="commitStatusTone(c.status)"
            >
              {{ commitStatusShort(c.status) }}
            </span>
            <span
              v-for="r in c.refs"
              :key="r"
              class="ui-badge sm info"
            >
              {{ r }}
            </span>
          </div>
          <div class="gcp-row-msg">{{ c.summary }}</div>
          <div class="gcp-row-meta">
            <span class="gcp-row-author">{{ c.author }}</span>
            <span class="gcp-row-time">{{ formatGitTimestamp(c.timestamp) }}</span>
          </div>
        </button>
      </div>
    </aside>

    <!-- Right Detail + Actions + Diff -->
    <section class="gcp-detail-pane">
      <div v-if="!selectedCommit" class="gcp-empty-detail ui-card">
        {{ tr("在左侧选择一个提交以查看文件差异或执行回滚操作") }}
      </div>

      <template v-else>
        <div class="gcp-header-card ui-card">
          <div class="gcp-title-row">
            <h3 class="gcp-commit-title">{{ selectedCommit.summary }}</h3>
            <span class="ui-badge sm" :class="commitStatusTone(selectedCommit.status)">
              {{ selectedCommit.statusLabel }}
            </span>
          </div>
          <pre v-if="selectedCommit.message !== selectedCommit.summary" class="gcp-full-msg">{{ selectedCommit.message }}</pre>

          <div class="gcp-meta-grid">
            <div class="gcp-meta-item">
              <span class="gcp-k">{{ tr("提交 SHA:") }}</span>
              <code class="mono gcp-sha-code">{{ selectedCommit.sha }}</code>
              <button
                type="button"
                class="ui-icon-btn sm"
                :title="tr('复制提交 SHA')"
                @click="copyText(selectedCommit.sha, tr('提交 SHA'))"
              >
                <Copy :size="11" />
              </button>
            </div>
            <div class="gcp-meta-item">
              <span class="gcp-k">{{ tr("Tree SHA:") }}</span>
              <code class="mono gcp-sha-code">{{ selectedCommit.treeSha }}</code>
            </div>
            <div class="gcp-meta-item">
              <span class="gcp-k">{{ tr("作者:") }}</span>
              <span class="gcp-sha-code">{{ selectedCommit.author }}</span>
            </div>
            <div class="gcp-meta-item">
              <span class="gcp-k">{{ tr("时间:") }}</span>
              <span class="mono">{{ formatGitTimestamp(selectedCommit.timestamp) }}</span>
            </div>
            <div v-if="selectedCommit.parents.length" class="gcp-meta-item">
              <span class="gcp-k">{{ tr("父提交:") }}</span>
              <button
                v-for="p in selectedCommit.parents"
                :key="p"
                type="button"
                class="ui-btn link sm mono"
                @click="selectedSha = p"
              >
                {{ shortSha(p) }}
              </button>
            </div>
          </div>

          <!-- GitHub-Style Action Bar -->
          <div class="gcp-actions">
            <button
              type="button"
              class="ui-btn sm primary"
              @click="emit('browseCommit', selectedCommit.sha)"
            >
              <FolderOpen :size="13" />
              <span>{{ tr("浏览此版本文件") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="actionBusy"
              @click="runGitAction('reset_hard', selectedCommit.sha)"
            >
              <RotateCcw :size="13" />
              <span>{{ tr("回滚到此提交 (Reset --hard)") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="actionBusy"
              @click="runGitAction('reset_soft', selectedCommit.sha)"
            >
              <History :size="13" />
              <span>{{ tr("软回滚 (Reset --soft)") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="actionBusy"
              @click="runGitAction('revert', selectedCommit.sha)"
            >
              <Undo2 :size="13" />
              <span>{{ tr("反向撤销 (Revert)") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="actionBusy"
              @click="openBranchForm"
            >
              <GitBranchPlus :size="13" />
              <span>{{ tr("从该提交建分支") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="exporting"
              @click="exportSnapshot(selectedCommit.sha)"
            >
              <Download :size="13" />
              <span>{{ tr("导出此版本") }}</span>
            </button>
          </div>

          <form v-if="branchFormOpen" class="gcp-branch-form" @submit.prevent="submitCreateBranch">
            <input
              ref="branchInputRef"
              v-model="newBranchName"
              class="ui-input sm mono"
              :placeholder="tr('输入新分支名称，例如 recovery-branch')"
            />
            <button type="submit" class="ui-btn sm primary" :disabled="actionBusy">
              {{ tr("创建分支") }}
            </button>
            <button type="button" class="ui-btn sm ghost" @click="branchFormOpen = false">
              {{ tr("取消") }}
            </button>
          </form>
        </div>

        <GitDiffViewer :diff="diffResult" :loading="diffLoading" :error="diffError" />
      </template>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  Copy,
  Download,
  FolderOpen,
  GitBranchPlus,
  History,
  RotateCcw,
  Search,
  Undo2,
  X,
} from 'lucide-vue-next';
import { copyText } from '../../../composables/clipboard';
import { confirm } from '../../../composables/useConfirm';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceDiff,
  gitWorkspaceExport,
  gitWorkspaceSandboxGit,
  type GitDiffResult,
  type GitRepoOverview,
} from '../../../lib/ipc/gitWorkspace';
import { formatBytes } from '../../../utils/format';
import { formatGitTimestamp, shortSha } from '../model/pluginTools';
import GitDiffViewer from './GitDiffViewer.vue';

const props = defineProps<{
  imageId: ImageId;
  overview: GitRepoOverview;
  useSandbox: boolean;
  focusSha?: string | null;
}>();

const emit = defineEmits<{
  browseCommit: [sha: string];
  sandboxMutated: [];
}>();

const statusFilter = ref<'all' | 'reachable' | 'rolled_back'>('all');
const query = ref('');
const selectedSha = ref<string | null>(props.focusSha ?? null);

const diffResult = shallowRef<GitDiffResult | null>(null);
const diffLoading = ref(false);
const diffError = ref<string | null>(null);
const actionBusy = ref(false);
const exporting = ref(false);
const branchFormOpen = ref(false);
const newBranchName = ref('');
const branchInputRef = ref<HTMLInputElement | null>(null);

const rolledBackCount = computed(() =>
  props.overview.commits.filter(c => c.status === 'rolled_back' || c.status === 'dangling').length,
);

const reachableCount = computed(() =>
  props.overview.commits.length - rolledBackCount.value,
);

const filterTabs = computed(() => [
  { id: 'all' as const, label: tr("全部"), count: props.overview.commits.length },
  { id: 'reachable' as const, label: tr("可达"), count: reachableCount.value },
  { id: 'rolled_back' as const, label: tr("已回滚/悬空"), count: rolledBackCount.value },
]);

const filteredCommits = computed(() => {
  const q = query.value.trim().toLowerCase();
  return props.overview.commits.filter(c => {
    if (statusFilter.value === 'rolled_back' && c.status !== 'rolled_back' && c.status !== 'dangling') {
      return false;
    }
    if (statusFilter.value === 'reachable' && (c.status === 'rolled_back' || c.status === 'dangling')) {
      return false;
    }
    if (!q) return true;
    return (
      c.sha.toLowerCase().includes(q) ||
      c.summary.toLowerCase().includes(q) ||
      c.author.toLowerCase().includes(q) ||
      c.refs.some(r => r.toLowerCase().includes(q))
    );
  });
});

const selectedCommit = computed(() => {
  if (selectedSha.value) {
    const exact = props.overview.commits.find(c => c.sha === selectedSha.value);
    if (exact) return exact;
  }
  return filteredCommits.value[0] ?? null;
});

watch(
  () => props.overview.repoPath,
  () => {
    branchFormOpen.value = false;
  },
);

watch(
  () => props.focusSha,
  sha => {
    if (sha) {
      statusFilter.value = 'all';
      selectedSha.value = sha;
    }
  },
);

let diffReq = 0;

watch(
  [() => selectedCommit.value?.sha, () => props.overview.repoPath, () => props.useSandbox],
  async ([sha]) => {
    if (!sha) {
      diffResult.value = null;
      return;
    }
    const seq = ++diffReq;
    diffLoading.value = true;
    diffError.value = null;
    try {
      const res = await gitWorkspaceDiff({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        targetRev: sha,
        useSandbox: props.useSandbox,
      });
      if (seq === diffReq) diffResult.value = res;
    } catch (err) {
      if (seq === diffReq) diffError.value = String(err);
    } finally {
      if (seq === diffReq) diffLoading.value = false;
    }
  },
  { immediate: true },
);

async function runGitAction(
  action: 'reset_hard' | 'reset_soft' | 'revert',
  sha: string,
) {
  const titleMap = {
    reset_hard: tr("确认硬回滚 (Reset --hard)"),
    reset_soft: tr("确认软回滚 (Reset --soft)"),
    revert: tr("确认反向撤销 (Revert)"),
  };
  const ok = await confirm({
    title: titleMap[action],
    message: tr("将在交互沙盒中对提交 {0} 执行该 Git 操作（原始镜像只读不受影响），是否继续？", { 0: shortSha(sha) }),
    confirmText: tr("执行"),
    danger: action === 'reset_hard',
  });
  if (!ok) return;

  actionBusy.value = true;
  try {
    const res = await gitWorkspaceSandboxGit({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action,
      targetRef: sha,
    });
    if (!res.ok) {
      notifyError(tr("Git 操作失败"), res.stderr || res.stdout || tr("未知错误"));
      return;
    }
    notifySuccess(tr("Git 操作已完成"), res.stdout || shortSha(sha));
    emit('sandboxMutated');
  } catch (err) {
    notifyError(tr("Git 操作失败"), String(err));
  } finally {
    actionBusy.value = false;
  }
}

function openBranchForm() {
  if (!selectedCommit.value) return;
  newBranchName.value = `recover-${selectedCommit.value.shortSha}`;
  branchFormOpen.value = true;
  void nextTick(() => branchInputRef.value?.focus());
}

async function submitCreateBranch() {
  if (!selectedCommit.value || !newBranchName.value.trim()) return;
  actionBusy.value = true;
  try {
    const res = await gitWorkspaceSandboxGit({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'create_branch',
      targetRef: selectedCommit.value.sha,
      branchName: newBranchName.value.trim(),
    });
    if (!res.ok) {
      notifyError(tr("创建分支失败"), res.stderr || res.stdout);
      return;
    }
    branchFormOpen.value = false;
    notifySuccess(tr("分支已创建"), newBranchName.value.trim());
    emit('sandboxMutated');
  } catch (err) {
    notifyError(tr("创建分支失败"), String(err));
  } finally {
    actionBusy.value = false;
  }
}

async function exportSnapshot(sha: string) {
  const destination = await openDialog({
    directory: true,
    multiple: false,
    title: tr("选择版本快照导出目录"),
  });
  if (!destination || typeof destination !== 'string') return;
  exporting.value = true;
  try {
    const res = await gitWorkspaceExport({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      rev: sha,
      destination,
      useSandbox: props.useSandbox,
    });
    notifySuccess(
      tr("版本快照已导出"),
      tr("已导出 {0} 个文件（{1}）", { 0: res.exportedFiles, 1: formatBytes(res.exportedBytes) }),
    );
  } catch (err) {
    notifyError(tr("导出失败"), String(err));
  } finally {
    exporting.value = false;
  }
}

function commitStatusShort(status: string): string {
  switch (status) {
    case 'rolled_back': return tr("已回滚");
    case 'dangling': return tr("悬空");
    case 'reset_target': return tr("回滚目标");
    default: return tr("可达");
  }
}

function commitStatusTone(status: string): string {
  switch (status) {
    case 'rolled_back': return 'danger';
    case 'dangling': return 'warn';
    case 'reset_target': return 'info';
    default: return 'success';
  }
}
</script>

<style scoped>
.git-commits-panel {
  display: grid;
  grid-template-columns: minmax(300px, 360px) minmax(0, 1fr);
  gap: 12px;
  height: 100%;
  min-height: 0;
  flex: 1;
  overflow: hidden;
}

@media (max-width: 960px) {
  .git-commits-panel {
    grid-template-columns: 1fr;
    overflow-y: auto;
  }
}

.gcp-list-pane {
  display: flex;
  flex-direction: column;
  padding: 0;
  overflow: hidden;
  height: 100%;
  min-height: 0;
}

@media (max-width: 960px) {
  .gcp-list-pane {
    max-height: 320px;
  }
}

.gcp-filters {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
}

.gcp-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.gcp-search {
  width: 100%;
}

.gcp-scroll {
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}

.gcp-empty,
.gcp-empty-detail {
  padding: 28px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.gcp-commit-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  padding: 10px 12px;
  border: 0;
  border-bottom: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
  transition: background var(--transition-fast);
}

.gcp-commit-row:hover {
  background: var(--hover-bg);
}

.gcp-commit-row.selected {
  background: var(--sidebar-active-bg);
}

.gcp-row-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
}

.gcp-row-sha {
  font-size: 11px;
  font-weight: 600;
  color: var(--primary-color);
}

.gcp-row-msg {
  font-size: 12px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gcp-row-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: 11px;
  color: var(--text-secondary);
}

.gcp-row-author {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}

.gcp-row-time {
  flex-shrink: 0;
  white-space: nowrap;
}

.gcp-detail-pane {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.gcp-header-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex-shrink: 0;
  padding: 14px 16px;
}

.gcp-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.gcp-commit-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  min-width: 0;
  overflow-wrap: anywhere;
}

.gcp-full-msg {
  margin: 0;
  padding: 8px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-primary);
  font-size: 12px;
  white-space: pre-wrap;
}

.gcp-meta-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 6px 14px;
  font-size: 12px;
}

.gcp-meta-item {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 6px;
  min-width: 0;
}

.gcp-sha-code {
  overflow-wrap: anywhere;
  word-break: break-all;
}

.gcp-k {
  color: var(--text-secondary);
}

.gcp-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding-top: 4px;
}

.gcp-branch-form {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 4px;
}
</style>
