<template>
  <div class="git-reflog-panel">
    <aside class="grp-list ui-card">
      <header class="grp-head">
        <label class="ui-check">
          <input v-model="onlyRollbacks" type="checkbox" />
          <span>{{ tr("仅显示回滚 / 重置 / 重写") }}</span>
        </label>
        <span class="ui-badge sm">{{ filteredEntries.length }}</span>
      </header>

      <div class="grp-scroll">
        <div v-if="filteredEntries.length === 0" class="grp-empty">
          {{ tr("暂无匹配的 Reflog 记录") }}
        </div>
        <button
          v-for="(entry, idx) in filteredEntries"
          :key="`${entry.refName}-${entry.oldSha}-${entry.newSha}-${idx}`"
          type="button"
          class="grp-row"
          :class="{ selected: selectedIndex === idx }"
          @click="selectedIndex = idx"
        >
          <div class="grp-row-top">
            <span class="ui-badge sm" :class="entry.isRollback ? 'warn' : 'info'">
              {{ entry.actionType }}
            </span>
            <span class="grp-ref mono">{{ entry.refName }}</span>
          </div>
          <div class="grp-shas mono">{{ entry.oldShort }} → {{ entry.newShort }}</div>
          <div class="grp-detail">{{ entry.detail || '—' }}</div>
          <div class="grp-meta">
            <span>{{ entry.actor || '—' }}</span>
            <span>{{ formatGitTimestamp(entry.timestamp) }}</span>
          </div>
        </button>
      </div>
    </aside>

    <section class="grp-preview">
      <div v-if="!activeEntry" class="ui-card grp-empty">
        {{ tr("选择一条 Reflog 记录以查看变更前后的差异并恢复") }}
      </div>
      <template v-else>
        <div class="ui-card grp-card">
          <div class="grp-card-head">
            <span class="ui-badge sm" :class="activeEntry.isRollback ? 'warn' : 'info'">
              {{ activeEntry.actionType }}
            </span>
            <span class="grp-card-title mono">
              {{ activeEntry.oldShort }} → {{ activeEntry.newShort }}
            </span>
          </div>
          <div class="grp-card-desc">{{ activeEntry.detail }}</div>
          <div class="grp-card-meta">
            <span>{{ tr("引用:") }} <code class="mono">{{ activeEntry.refName }}</code></span>
            <span>{{ tr("操作人:") }} {{ activeEntry.actor || '—' }}</span>
            <span>{{ tr("时间:") }} <span class="mono">{{ formatGitTimestamp(activeEntry.timestamp) }}</span></span>
          </div>
          <div class="grp-actions">
            <button
              v-if="hasValidSha(activeEntry.oldSha)"
              type="button"
              class="ui-btn sm primary"
              @click="emit('browseCommit', activeEntry.oldSha)"
            >
              <FolderOpen :size="13" />
              <span>{{ tr("浏览变更前文件") }}</span>
            </button>
            <button
              v-if="hasValidSha(activeEntry.oldSha)"
              type="button"
              class="ui-btn sm"
              :disabled="busy"
              @click="restoreOldSha(activeEntry.oldSha)"
            >
              <RotateCcw :size="13" />
              <span>{{ tr("恢复至变更前 (Reset --hard)") }}</span>
            </button>
            <button
              v-if="hasValidSha(activeEntry.newSha)"
              type="button"
              class="ui-btn sm"
              @click="emit('browseCommit', activeEntry.newSha)"
            >
              <FolderOpen :size="13" />
              <span>{{ tr("浏览变更后文件") }}</span>
            </button>
          </div>
        </div>

        <GitDiffViewer :diff="diffResult" :loading="diffLoading" :error="diffError" />
      </template>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import { FolderOpen, RotateCcw } from 'lucide-vue-next';
import { confirm } from '../../../composables/useConfirm';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceDiff,
  gitWorkspaceSandboxGit,
  type GitDiffResult,
  type GitReflogInfo,
  type GitRepoOverview,
} from '../../../lib/ipc/gitWorkspace';
import { formatGitTimestamp, shortSha } from '../model/pluginTools';
import GitDiffViewer from './GitDiffViewer.vue';

const ZERO_SHA = '0000000000000000000000000000000000000000';

const props = defineProps<{
  imageId: ImageId;
  overview: GitRepoOverview;
  useSandbox: boolean;
}>();

const emit = defineEmits<{
  browseCommit: [sha: string];
  sandboxMutated: [];
}>();

const onlyRollbacks = ref(false);
const selectedIndex = ref(0);
const diffResult = shallowRef<GitDiffResult | null>(null);
const diffLoading = ref(false);
const diffError = ref<string | null>(null);
const busy = ref(false);

const filteredEntries = computed(() =>
  onlyRollbacks.value
    ? props.overview.reflogs.filter(e => e.isRollback)
    : props.overview.reflogs,
);

const activeEntry = computed<GitReflogInfo | null>(
  () => filteredEntries.value[selectedIndex.value] ?? filteredEntries.value[0] ?? null,
);

function hasValidSha(sha: string): boolean {
  return Boolean(sha && sha !== ZERO_SHA);
}

let diffSeq = 0;

watch(
  [() => props.overview.repoPath, onlyRollbacks],
  () => {
    selectedIndex.value = 0;
  },
);

watch(
  [activeEntry, () => props.overview.repoPath, () => props.useSandbox],
  async ([entry]) => {
    if (!entry || !hasValidSha(entry.newSha)) {
      diffResult.value = null;
      diffError.value = null;
      return;
    }
    const seq = ++diffSeq;
    diffLoading.value = true;
    diffError.value = null;
    try {
      const res = await gitWorkspaceDiff({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        targetRev: entry.newSha,
        baseRev: hasValidSha(entry.oldSha) ? entry.oldSha : undefined,
        useSandbox: props.useSandbox,
      });
      if (seq === diffSeq) diffResult.value = res;
    } catch (err) {
      if (seq === diffSeq) diffError.value = String(err);
    } finally {
      if (seq === diffSeq) diffLoading.value = false;
    }
  },
  { immediate: true },
);

async function restoreOldSha(oldSha: string) {
  const ok = await confirm({
    title: tr("恢复至回滚前版本"),
    message: tr("将在交互沙盒中执行 Reset --hard 恢复到 {0}（原始镜像保持只读不变），是否继续？", { 0: shortSha(oldSha) }),
    confirmText: tr("恢复"),
    danger: true,
  });
  if (!ok) return;

  busy.value = true;
  try {
    const res = await gitWorkspaceSandboxGit({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'reset_hard',
      targetRef: oldSha,
    });
    if (!res.ok) {
      notifyError(tr("恢复失败"), res.stderr || res.stdout);
      return;
    }
    notifySuccess(tr("已恢复到变更前提交"), shortSha(oldSha));
    emit('sandboxMutated');
  } catch (err) {
    notifyError(tr("恢复失败"), String(err));
  } finally {
    busy.value = false;
  }
}
</script>

<style scoped>
.git-reflog-panel {
  display: grid;
  grid-template-columns: minmax(300px, 360px) minmax(0, 1fr);
  gap: 12px;
  height: 100%;
  min-height: 0;
  flex: 1;
  overflow: hidden;
}

@media (max-width: 960px) {
  .git-reflog-panel {
    grid-template-columns: 1fr;
    overflow-y: auto;
  }
}

.grp-list {
  display: flex;
  flex-direction: column;
  padding: 0;
  overflow: hidden;
  height: 100%;
  min-height: 0;
}

@media (max-width: 960px) {
  .grp-list {
    max-height: 320px;
  }
}

.grp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
  font-size: 12px;
}

.grp-scroll {
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}

.grp-empty {
  padding: 28px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.grp-row {
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

.grp-row:hover {
  background: var(--hover-bg);
}

.grp-row.selected {
  background: var(--sidebar-active-bg);
}

.grp-row-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}

.grp-ref {
  font-size: 11px;
  color: var(--text-secondary);
}

.grp-shas {
  font-size: 12px;
  font-weight: 600;
  color: var(--primary-color);
}

.grp-detail {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.grp-meta {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-secondary);
}

.grp-preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.grp-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex-shrink: 0;
  padding: 14px 16px;
}

.grp-card-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.grp-card-title {
  font-size: 13px;
  font-weight: 600;
}

.grp-card-desc {
  font-size: 13px;
  color: var(--text-primary);
}

.grp-card-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  font-size: 12px;
  color: var(--text-secondary);
}

.grp-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
</style>
