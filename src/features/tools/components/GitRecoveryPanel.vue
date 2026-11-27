<template>
  <div class="git-recovery-panel">
    <aside class="grc-list ui-card">
      <div class="grc-filters">
        <div class="grc-chips">
          <button
            v-for="tab in filterTabs"
            :key="tab.id"
            type="button"
            class="ui-btn sm ghost"
            :class="{ active: kindFilter === tab.id }"
            @click="kindFilter = tab.id"
          >
            {{ tab.label }} ({{ tab.count }})
          </button>
        </div>
        <label class="ui-search grc-search">
          <Search :size="13" />
          <input v-model="query" :placeholder="tr('搜索残留或敏感关键字…')" />
          <button v-if="query" type="button" class="ui-icon-btn sm" @click="query = ''">
            <X :size="11" />
          </button>
        </label>
      </div>

      <div class="grc-scroll">
        <div v-if="filteredItems.length === 0" class="grc-empty">
          {{ tr("未发现匹配的残留或敏感恢复项") }}
        </div>
        <button
          v-for="item in filteredItems"
          :key="item.id"
          type="button"
          class="grc-row"
          :class="{ selected: activeItem?.id === item.id }"
          @click="selectedId = item.id"
        >
          <div class="grc-row-top">
            <span class="ui-badge sm" :class="kindTone(item.kind)">{{ kindLabel(item.kind) }}</span>
            <span v-if="item.objectSha" class="grc-sha mono">{{ shortSha(item.objectSha) }}</span>
          </div>
          <div class="grc-row-title">{{ item.title }}</div>
          <div class="grc-row-preview mono">{{ item.preview }}</div>
        </button>
      </div>
    </aside>

    <section class="grc-detail">
      <div v-if="!activeItem" class="ui-card grc-empty">
        {{ tr("选择左侧一条取证发现以查看完整恢复内容") }}
      </div>
      <template v-else>
        <div class="ui-card grc-card">
          <div class="grc-card-head">
            <span class="ui-badge sm" :class="kindTone(activeItem.kind)">
              {{ activeItem.findingKind }}
            </span>
            <span class="ui-badge sm mono">{{ formatBytes(activeItem.size) }}</span>
          </div>
          <h3 class="grc-card-title">{{ activeItem.title }}</h3>
          <div v-if="activeItem.objectSha" class="grc-meta-line">
            <span>{{ tr("对象 SHA:") }}</span>
            <code class="mono">{{ activeItem.objectSha }}</code>
            <span class="ui-badge sm">{{ activeItem.objectType }}</span>
          </div>
          <div class="grc-snippet mono">{{ activeItem.preview }}</div>

          <div class="grc-actions">
            <button type="button" class="ui-btn sm" @click="copyPreview">
              <Copy :size="13" />
              <span>{{ tr("复制发现内容") }}</span>
            </button>
            <button
              v-if="activeItem.kind === 'dangling_commit' && activeItem.objectSha"
              type="button"
              class="ui-btn sm primary"
              @click="emit('browseCommit', activeItem.objectSha)"
            >
              <FolderOpen :size="13" />
              <span>{{ tr("浏览该提交文件") }}</span>
            </button>
            <button
              v-if="activeItem.kind === 'dangling_commit' && activeItem.objectSha"
              type="button"
              class="ui-btn sm"
              :disabled="busy"
              @click="rescueCommitAsBranch(activeItem.objectSha)"
            >
              <GitBranchPlus :size="13" />
              <span>{{ tr("抢救为新分支") }}</span>
            </button>
            <button
              v-if="activeItem.kind === 'dangling_commit' && activeItem.objectSha"
              type="button"
              class="ui-btn sm"
              @click="emit('inspectCommitDiff', activeItem.objectSha)"
            >
              <GitCommitHorizontal :size="13" />
              <span>{{ tr("在提交历史查看 Diff") }}</span>
            </button>
          </div>
        </div>

        <div v-if="loadingDetail" class="ui-card grc-empty">
          <Loader2 :size="16" class="spin" />
          <span>{{ tr("正在加载对象内容…") }}</span>
        </div>

        <div v-else-if="activeItem.kind === 'secret_blob' && blobContent" class="ui-card grc-blob-card">
          <header class="grc-blob-head">
            <span class="mono">{{ tr("完整 Blob 内容") }} · {{ shortSha(activeItem.objectSha) }}</span>
            <button
              v-if="blobContent.text"
              type="button"
              class="ui-btn sm"
              @click="copyFullBlob"
            >
              <Copy :size="12" />
              <span>{{ tr("复制全文") }}</span>
            </button>
          </header>
          <pre class="grc-blob-text mono">{{ blobContent.text ?? blobContent.hexPreview ?? '' }}</pre>
        </div>

        <GitDiffViewer
          v-else-if="activeItem.kind === 'dangling_commit'"
          :diff="commitDiff"
          :loading="false"
          :error="detailError"
        />
      </template>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import {
  Copy,
  FolderOpen,
  GitBranchPlus,
  GitCommitHorizontal,
  Loader2,
  Search,
  X,
} from 'lucide-vue-next';
import { copyText } from '../../../composables/clipboard';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceDiff,
  gitWorkspaceFileRead,
  gitWorkspaceSandboxGit,
  type GitDiffResult,
  type GitFileContent,
  type GitRecoveryItem,
  type GitRepoOverview,
} from '../../../lib/ipc/gitWorkspace';
import { formatBytes } from '../../../utils/format';
import { shortSha } from '../model/pluginTools';
import GitDiffViewer from './GitDiffViewer.vue';

const props = defineProps<{
  imageId: ImageId;
  overview: GitRepoOverview;
  useSandbox: boolean;
}>();

const emit = defineEmits<{
  browseCommit: [sha: string];
  inspectCommitDiff: [sha: string];
  sandboxMutated: [];
}>();

const kindFilter = ref<'all' | 'config_leak' | 'dangling_commit' | 'secret_blob'>('all');
const query = ref('');
const selectedId = ref<string | null>(null);

const loadingDetail = ref(false);
const detailError = ref<string | null>(null);
const blobContent = shallowRef<GitFileContent | null>(null);
const commitDiff = shallowRef<GitDiffResult | null>(null);
const busy = ref(false);

const filterTabs = computed(() => [
  { id: 'all' as const, label: tr("全部"), count: props.overview.recoveries.length },
  {
    id: 'config_leak' as const,
    label: tr("凭据暴露"),
    count: props.overview.recoveries.filter(r => r.kind === 'config_leak').length,
  },
  {
    id: 'dangling_commit' as const,
    label: tr("悬空提交"),
    count: props.overview.recoveries.filter(r => r.kind === 'dangling_commit').length,
  },
  {
    id: 'secret_blob' as const,
    label: tr("敏感片段"),
    count: props.overview.recoveries.filter(r => r.kind === 'secret_blob').length,
  },
]);

const filteredItems = computed(() => {
  const q = query.value.trim().toLowerCase();
  return props.overview.recoveries.filter(item => {
    if (kindFilter.value !== 'all' && item.kind !== kindFilter.value) return false;
    if (!q) return true;
    return (
      item.title.toLowerCase().includes(q) ||
      item.preview.toLowerCase().includes(q) ||
      item.objectSha.toLowerCase().includes(q) ||
      item.findingKind.toLowerCase().includes(q)
    );
  });
});

const activeItem = computed<GitRecoveryItem | null>(() => {
  if (selectedId.value) {
    const exact = filteredItems.value.find(i => i.id === selectedId.value);
    if (exact) return exact;
  }
  return filteredItems.value[0] ?? null;
});

let loadSeq = 0;

watch(
  () => props.overview.repoPath,
  () => {
    selectedId.value = null;
  },
);

watch(
  [activeItem, () => props.overview.repoPath, () => props.useSandbox],
  async ([item]) => {
    blobContent.value = null;
    commitDiff.value = null;
    detailError.value = null;
    if (!item || !item.objectSha) return;

    const seq = ++loadSeq;
    loadingDetail.value = true;
    try {
      if (item.kind === 'secret_blob') {
        const res = await gitWorkspaceFileRead({
          imageId: props.imageId,
          repoPath: props.overview.repoPath,
          blobSha: item.objectSha,
          useSandbox: props.useSandbox,
        });
        if (seq === loadSeq) blobContent.value = res;
      } else if (item.kind === 'dangling_commit') {
        const res = await gitWorkspaceDiff({
          imageId: props.imageId,
          repoPath: props.overview.repoPath,
          targetRev: item.objectSha,
          useSandbox: props.useSandbox,
        });
        if (seq === loadSeq) commitDiff.value = res;
      }
    } catch (err) {
      if (seq === loadSeq) detailError.value = String(err);
    } finally {
      if (seq === loadSeq) loadingDetail.value = false;
    }
  },
  { immediate: true },
);

async function copyPreview() {
  if (!activeItem.value) return;
  await copyText(activeItem.value.preview, activeItem.value.title);
}

async function copyFullBlob() {
  if (!blobContent.value?.text) return;
  await copyText(blobContent.value.text, shortSha(activeItem.value?.objectSha) || tr("全文"));
}

async function rescueCommitAsBranch(sha: string) {
  busy.value = true;
  const branch = `rescued-${shortSha(sha)}`;
  try {
    const res = await gitWorkspaceSandboxGit({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'create_branch',
      targetRef: sha,
      branchName: branch,
    });
    if (!res.ok) {
      notifyError(tr("抢救分支失败"), res.stderr || res.stdout);
      return;
    }
    notifySuccess(tr("已创建抢救分支"), branch);
    emit('sandboxMutated');
  } catch (err) {
    notifyError(tr("抢救分支失败"), String(err));
  } finally {
    busy.value = false;
  }
}

function kindLabel(kind: string): string {
  switch (kind) {
    case 'config_leak': return tr("凭据暴露");
    case 'dangling_commit': return tr("悬空提交");
    default: return tr("敏感片段");
  }
}

function kindTone(kind: string): string {
  switch (kind) {
    case 'config_leak': return 'danger';
    case 'dangling_commit': return 'warn';
    default: return 'danger';
  }
}
</script>

<style scoped>
.git-recovery-panel {
  display: grid;
  grid-template-columns: minmax(300px, 360px) minmax(0, 1fr);
  gap: 12px;
  height: 100%;
  min-height: 0;
  flex: 1;
  overflow: hidden;
}

@media (max-width: 960px) {
  .git-recovery-panel {
    grid-template-columns: 1fr;
    overflow-y: auto;
  }
}

.grc-list {
  display: flex;
  flex-direction: column;
  padding: 0;
  overflow: hidden;
  height: 100%;
  min-height: 0;
}

@media (max-width: 960px) {
  .grc-list {
    max-height: 320px;
  }
}

.grc-filters {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
}

.grc-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.grc-search {
  width: 100%;
}

.grc-scroll {
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}

.grc-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 28px;
  font-size: 12px;
  color: var(--text-secondary);
}

.grc-row {
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
}

.grc-row:hover {
  background: var(--hover-bg);
}

.grc-row.selected {
  background: var(--sidebar-active-bg);
}

.grc-row-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}

.grc-sha {
  font-size: 11px;
  color: var(--primary-color);
}

.grc-row-title {
  font-size: 12px;
  font-weight: 600;
}

.grc-row-preview {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.grc-detail {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.grc-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex-shrink: 0;
  padding: 14px 16px;
}

.grc-card-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.grc-card-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.grc-meta-line {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 8px;
  font-size: 12px;
  color: var(--text-secondary);
  word-break: break-all;
}

.grc-snippet {
  padding: 10px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-primary);
  font-size: 12px;
  word-break: break-all;
}

.grc-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.grc-blob-card {
  padding: 0;
  overflow: hidden;
  flex-shrink: 0;
}

.grc-blob-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
  font-size: 12px;
}

.grc-blob-text {
  margin: 0;
  padding: 12px;
  max-height: 460px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
