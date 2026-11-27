<template>
  <Teleport to="body">
    <Transition name="ui-modal">
      <div v-show="open" class="ui-modal-overlay" @click.self="emit('close')">
        <div
          class="ui-modal diff-modal"
          role="dialog"
          aria-modal="true"
          tabindex="-1"
          @keydown.esc.stop="event => { if (!event.isComposing) emit('close'); }"
        >
          <header class="ui-modal-header">
            <span class="ui-modal-icon"><GitCompareArrows :size="18" /></span>
            <div class="ui-modal-heading">
              <h2 class="ui-modal-title">
                {{ copy ? tr("卷影副本 #{0} 与当前分区对比", { 0: copy.index + 1 }) : tr("卷影副本差异对比") }}
              </h2>
              <div v-if="copy" class="modal-sub mono">
                {{ utcTime(copy) }} · {{ tr("分区 #{0}", { 0: partitionId }) }}
              </div>
            </div>
            <button class="ui-icon-btn" type="button" :title="tr('关闭')" @click="emit('close')">
              <X :size="16" />
            </button>
          </header>

          <div class="ui-modal-body diff-body">
            <div v-if="loading" class="state-box mono">
              <Loader2 :size="16" class="spin" />
              {{ tr("正在对比卷影副本与当前 NTFS 分区的 $MFT 记录…") }}
            </div>
            <div v-else-if="error" class="ui-alert danger" role="alert">
              {{ tr("卷影副本对比失败：{0}", { 0: error }) }}
            </div>
            <template v-else-if="report">
              <div class="summary-bar">
                <span class="stat-pill deleted mono">{{ tr("仅存于卷影（已删除）：{0}", { 0: report.deletedCount }) }}</span>
                <span class="stat-pill modified mono">{{ tr("内容/大小变更：{0}", { 0: report.modifiedCount }) }}</span>
                <span class="stat-pill added mono">{{ tr("当前卷新增：{0}", { 0: report.addedCount }) }}</span>
                <span v-if="report.truncated" class="stat-pill warn mono">{{ tr("已达结果上限截断") }}</span>
              </div>

              <div class="toolbar">
                <div class="pills">
                  <button
                    v-for="pill in kindPills"
                    :key="pill.key"
                    type="button"
                    class="kind-pill"
                    :class="{ active: kindFilter === pill.key }"
                    @click="setKindFilter(pill.key)"
                  >
                    {{ pill.label }}
                  </button>
                </div>

                <label class="artifact-toggle">
                  <input v-model="forensicOnly" type="checkbox" />
                  {{ tr("仅看高价值取证目标") }}
                </label>

                <form class="search-form" @submit.prevent="runDiff">
                  <input
                    v-model="queryInput"
                    class="ui-input sm mono"
                    :placeholder="tr('按文件名或路径过滤（回车）…')"
                  />
                  <button type="submit" class="ui-btn sm">{{ tr("筛选") }}</button>
                </form>
              </div>

              <div class="diff-table-wrap">
                <table class="diff-table">
                  <thead>
                    <tr>
                      <th class="col-kind">{{ tr("变化") }}</th>
                      <th class="col-path">{{ tr("文件路径") }}</th>
                      <th class="col-size">{{ tr("卷影大小") }}</th>
                      <th class="col-size">{{ tr("当前大小") }}</th>
                      <th class="col-time">{{ tr("卷影修改时间") }}</th>
                      <th class="col-time">{{ tr("当前修改时间") }}</th>
                      <th class="col-actions">{{ tr("操作") }}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-if="visibleItems.length === 0">
                      <td colspan="7" class="empty-cell">{{ tr("当前筛选条件下无差异项") }}</td>
                    </tr>
                    <tr
                      v-for="item in visibleItems"
                      :key="`${item.kind}:${item.path}`"
                      :class="{ selected: previewTarget?.item.path === item.path && previewTarget?.item.kind === item.kind }"
                    >
                      <td>
                        <span class="diff-badge" :class="item.kind">{{ kindLabel(item.kind) }}</span>
                      </td>
                      <td class="mono path-cell" :title="item.path">
                        <span v-if="item.isForensicArtifact" class="artifact-dot" :title="tr('重点取证痕迹')">★</span>
                        {{ item.path }}
                      </td>
                      <td class="mono">{{ item.shadowSize !== null ? formatBytes(item.shadowSize) : '—' }}</td>
                      <td class="mono">{{ item.liveSize !== null ? formatBytes(item.liveSize) : '—' }}</td>
                      <td class="mono">{{ formatIso(item.shadowModified) }}</td>
                      <td class="mono">{{ formatIso(item.liveModified) }}</td>
                      <td class="actions-cell">
                        <template v-if="!item.isDir && (item.shadowNodeId || item.liveNodeId)">
                          <button
                            type="button"
                            class="row-btn"
                            :title="item.shadowNodeId ? tr('直接预览卷影内旧版本文件') : tr('预览当前卷文件')"
                            @click="previewRow(item)"
                          >
                            <Eye :size="12" />
                            {{ tr("预览") }}
                          </button>
                          <button
                            v-if="item.shadowNodeId"
                            type="button"
                            class="row-btn"
                            :disabled="exportingPath === item.path"
                            :title="tr('直接从卷影副本导出此文件')"
                            @click="exportRow(item)"
                          >
                            <Loader2 v-if="exportingPath === item.path" :size="12" class="spin" />
                            <Download v-else :size="12" />
                            {{ tr("导出") }}
                          </button>
                        </template>
                        <span v-else class="muted">—</span>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>

              <div v-if="previewTarget" class="preview-drawer">
                <div class="preview-bar">
                  <span class="preview-title mono" :title="previewTarget.item.path">
                    {{ previewTarget.fromShadow ? tr("卷影预览") : tr("当前卷预览") }}：{{ previewTarget.item.path }}
                  </span>
                  <div class="preview-tabs">
                    <button
                      type="button"
                      class="ptab"
                      :class="{ active: previewMode === 'text' }"
                      @click="previewMode = 'text'"
                    >{{ tr("文本") }}</button>
                    <button
                      type="button"
                      class="ptab"
                      :class="{ active: previewMode === 'hex' }"
                      @click="previewMode = 'hex'"
                    >{{ tr("十六进制") }}</button>
                    <button
                      v-if="previewTarget.item.shadowNodeId"
                      type="button"
                      class="row-btn"
                      :disabled="exportingPath === previewTarget.item.path"
                      @click="exportRow(previewTarget.item)"
                    >
                      <Download :size="12" />
                      {{ tr("导出此文件") }}
                    </button>
                    <button type="button" class="ui-icon-btn" :title="tr('关闭预览')" @click="closePreview">
                      <X :size="13" />
                    </button>
                  </div>
                </div>
                <div v-if="previewLoading" class="preview-state mono">
                  <Loader2 :size="13" class="spin" />
                  {{ tr("正在读取文件内容…") }}
                </div>
                <div v-else-if="previewError" class="preview-state error mono">{{ previewError }}</div>
                <div v-else class="preview-content">
                  <img v-if="previewImageUrl" :src="previewImageUrl" class="preview-img" :alt="previewTarget.item.path" />
                  <pre v-else-if="previewMode === 'text'" class="preview-pre mono">{{ previewText }}</pre>
                  <pre v-else class="preview-pre mono">{{ previewHex }}</pre>
                </div>
              </div>
            </template>
          </div>

          <footer class="ui-modal-footer">
            <button v-if="copy" type="button" class="ui-btn primary" @click="emit('open-copy', copy)">
              <FolderOpen :size="14" />
              {{ copy.open_image_id ? tr("切换到该卷影副本") : tr("打开该卷影副本浏览与导出") }}
            </button>
            <button type="button" class="ui-btn" @click="emit('close')">{{ tr("关闭") }}</button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from 'vue';
import { save } from '@tauri-apps/plugin-dialog';
import { Download, Eye, FolderOpen, GitCompareArrows, Loader2, X } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifyInfo } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import { fsPreview } from '../../../lib/ipc/fs/read';
import {
  shadowCopyDiff,
  shadowCopyFileExport,
  shadowCopyFilePreview,
  type ShadowCopyDiffItem,
  type ShadowCopyDiffReport,
  type ShadowCopyInfo,
} from '../../../lib/ipc/shadow';
import { formatBytes } from '../model/evidenceOverview';
import { formatEpochSeconds } from '../../../utils/format';

const PREVIEW_CAP = 256 * 1024;

const props = defineProps<{
  open: boolean;
  imageId: ImageId;
  partitionId: number;
  copy: ShadowCopyInfo | null;
}>();

const emit = defineEmits<{
  close: [];
  'open-copy': [copy: ShadowCopyInfo];
}>();

const loading = ref(false);
const error = ref<string | null>(null);
const report = ref<ShadowCopyDiffReport | null>(null);
const kindFilter = ref<'all' | 'deleted' | 'modified' | 'added'>('all');
const queryInput = ref('');
const forensicOnly = ref(false);

const previewTarget = ref<{ item: ShadowCopyDiffItem; fromShadow: boolean } | null>(null);
const previewMode = ref<'text' | 'hex'>('text');
const previewLoading = ref(false);
const previewError = ref<string | null>(null);
const previewBytes = ref<Uint8Array>(new Uint8Array());
const previewImageUrl = ref<string | null>(null);
const exportingPath = ref<string | null>(null);

const kindPills = computed(() => [
  { key: 'all' as const, label: tr('全部') },
  { key: 'deleted' as const, label: tr('已删除（仅卷影有）') },
  { key: 'modified' as const, label: tr('已修改') },
  { key: 'added' as const, label: tr('当前新增') },
]);

const visibleItems = computed<ShadowCopyDiffItem[]>(() => {
  const list = report.value?.items ?? [];
  return forensicOnly.value ? list.filter((i) => i.isForensicArtifact) : list;
});

const previewText = computed(() => {
  const bytes = previewBytes.value;
  if (!bytes.length) return tr('（空文件）');
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
});

const previewHex = computed(() => {
  const bytes = previewBytes.value.subarray(0, 16 * 1024);
  if (!bytes.length) return tr('（空文件）');
  const lines: string[] = [];
  for (let off = 0; off < bytes.length; off += 16) {
    const chunk = bytes.subarray(off, off + 16);
    const hex = Array.from(chunk, (b) => b.toString(16).padStart(2, '0')).join(' ');
    const ascii = Array.from(chunk, (b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
    lines.push(`${off.toString(16).padStart(8, '0')}  ${hex.padEnd(47, ' ')}  |${ascii}|`);
  }
  return lines.join('\n');
});

function detectImageMime(bytes: Uint8Array): string | null {
  if (bytes.length >= 4 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47) return 'image/png';
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return 'image/jpeg';
  if (bytes.length >= 4 && bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x38) return 'image/gif';
  if (bytes.length >= 12 && bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x46 && bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50) return 'image/webp';
  return null;
}

function revokePreviewImage() {
  if (previewImageUrl.value) {
    URL.revokeObjectURL(previewImageUrl.value);
    previewImageUrl.value = null;
  }
}

function closePreview() {
  revokePreviewImage();
  previewTarget.value = null;
  previewBytes.value = new Uint8Array();
  previewError.value = null;
}

onScopeDispose(() => revokePreviewImage());

function kindLabel(kind: string): string {
  if (kind === 'deleted') return tr('已删除');
  if (kind === 'modified') return tr('已修改');
  return tr('新增');
}

function utcTime(copy: ShadowCopyInfo): string {
  const secs = copy.created_unix_ms == null ? null : copy.created_unix_ms / 1000;
  return formatEpochSeconds(secs, { timezone: 'utc', precision: 'seconds', suffix: true, fallback: tr('创建时间未知') });
}

function formatIso(iso: string | null): string {
  if (!iso) return '—';
  return iso.replace('T', ' ').replace(/\+00:00$|Z$/, ' UTC');
}

async function previewRow(item: ShadowCopyDiffItem) {
  if (!props.copy) return;
  const fromShadow = Boolean(item.shadowNodeId);
  const nodeId = item.shadowNodeId ?? item.liveNodeId;
  if (!nodeId) return;

  revokePreviewImage();
  previewTarget.value = { item, fromShadow };
  previewLoading.value = true;
  previewError.value = null;
  previewBytes.value = new Uint8Array();
  previewMode.value = 'text';

  try {
    const raw = fromShadow
      ? await shadowCopyFilePreview({
          imageId: props.imageId,
          partitionId: props.partitionId,
          index: props.copy.index,
          nodeId,
          maxBytes: PREVIEW_CAP,
        })
      : await fsPreview({ imageId: props.imageId, nodeId, maxBytes: PREVIEW_CAP });
    if (previewTarget.value?.item.path !== item.path) return;
    const arr = new Uint8Array(raw);
    previewBytes.value = arr;
    const imgMime = detectImageMime(arr);
    if (imgMime) {
      previewImageUrl.value = URL.createObjectURL(new Blob([arr], { type: imgMime }));
    }
  } catch (err) {
    if (previewTarget.value?.item.path === item.path) {
      previewError.value = String(err);
    }
  } finally {
    if (previewTarget.value?.item.path === item.path) previewLoading.value = false;
  }
}

async function exportRow(item: ShadowCopyDiffItem) {
  if (!props.copy || !item.shadowNodeId) return;
  const fileName = item.path.split(/[\\/]/).filter(Boolean).pop() || 'exported_file';
  const destination = await save({ defaultPath: fileName });
  if (!destination) return;
  exportingPath.value = item.path;
  try {
    const res = await shadowCopyFileExport({
      imageId: props.imageId,
      partitionId: props.partitionId,
      index: props.copy.index,
      nodeId: item.shadowNodeId,
      destination,
    });
    notifyInfo(
      tr('卷影文件导出成功'),
      tr('已导出 {0}（{1}）', { 0: fileName, 1: formatBytes(res.bytes_written) }),
    );
  } catch (err) {
    notifyError(tr('卷影文件导出失败'), String(err));
  } finally {
    exportingPath.value = null;
  }
}

async function runDiff() {
  if (!props.open || !props.copy) return;
  loading.value = true;
  error.value = null;
  closePreview();
  try {
    report.value = await shadowCopyDiff({
      imageId: props.imageId,
      partitionId: props.partitionId,
      index: props.copy.index,
      kindFilter: kindFilter.value,
      query: queryInput.value.trim() || null,
      limit: 1000,
    });
  } catch (err) {
    error.value = String(err);
    report.value = null;
  } finally {
    loading.value = false;
  }
}

function setKindFilter(key: 'all' | 'deleted' | 'modified' | 'added') {
  kindFilter.value = key;
  void runDiff();
}

watch(
  () => [props.open, props.imageId, props.partitionId, props.copy?.index],
  ([isOpen]) => {
    if (isOpen && props.copy) {
      kindFilter.value = 'all';
      queryInput.value = '';
      forensicOnly.value = false;
      void runDiff();
    } else {
      closePreview();
    }
  },
  { immediate: true },
);
</script>

<style scoped>
.diff-modal { width: min(1080px, 94vw); max-height: 88vh; display: flex; flex-direction: column; }
.modal-sub { font-size: 11px; color: var(--text-secondary); margin-top: 2px; }
.diff-body { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 10px; overflow: hidden; }
.state-box { display: flex; align-items: center; gap: 8px; padding: 24px; color: var(--text-secondary); }

.summary-bar { display: flex; flex-wrap: wrap; gap: 8px; }
.stat-pill { padding: 3px 8px; border-radius: var(--radius-sm); font-size: 11px; }
.stat-pill.deleted { background: var(--sev-critical-bg); color: var(--sev-critical-fg); }
.stat-pill.modified { background: var(--sev-warn-bg); color: var(--sev-warn-fg); }
.stat-pill.added { background: var(--sev-success-bg); color: var(--sev-success-fg); }
.stat-pill.warn { background: var(--surface-tertiary); color: var(--text-secondary); }

.toolbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.pills { display: flex; gap: 4px; flex-wrap: wrap; }
.kind-pill {
  padding: 3px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
}
.kind-pill.active { background: var(--primary-solid); border-color: var(--primary-solid); color: var(--accent-contrast); }
.artifact-toggle { display: flex; align-items: center; gap: 4px; font-size: 12px; color: var(--text-secondary); cursor: pointer; }
.search-form { display: flex; gap: 6px; margin-left: auto; }

.diff-table-wrap { flex: 1; min-height: 200px; overflow: auto; border: 1px solid var(--border-color); border-radius: var(--radius-sm); }
.diff-table { width: 100%; border-collapse: collapse; table-layout: fixed; font-size: 11px; }
.diff-table th {
  position: sticky;
  top: 0;
  padding: 6px 8px;
  text-align: left;
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-color);
  color: var(--text-secondary);
}
.diff-table td { padding: 5px 8px; border-bottom: 1px solid color-mix(in srgb, var(--border-color) 65%, transparent); }
.diff-table tbody tr.selected { background: color-mix(in srgb, var(--primary-color) 12%, transparent); }
.col-kind { width: 66px; }
.col-path { width: auto; }
.col-size { width: 84px; }
.col-time { width: 162px; }
.col-actions { width: 120px; }
.path-cell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.actions-cell { white-space: nowrap; }
.row-btn {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin-right: 4px;
  padding: 2px 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-primary);
  font-size: 11px;
  cursor: pointer;
}
.row-btn:hover { border-color: var(--primary-color); color: var(--primary-color); }
.row-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.muted { color: var(--text-secondary); }
.empty-cell { text-align: center; padding: 24px; color: var(--text-secondary); }

.diff-badge { display: inline-block; padding: 1px 5px; border-radius: var(--radius-sm); font-size: 10px; font-weight: 600; }
.diff-badge.deleted { background: var(--sev-critical-bg); color: var(--sev-critical-fg); }
.diff-badge.modified { background: var(--sev-warn-bg); color: var(--sev-warn-fg); }
.diff-badge.added { background: var(--sev-success-bg); color: var(--sev-success-fg); }
.artifact-dot { color: var(--sev-warn-fg); margin-right: 4px; }

.preview-drawer {
  display: flex;
  flex-direction: column;
  max-height: 240px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-secondary);
}
.preview-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-color);
}
.preview-title { font-size: 11px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.preview-tabs { display: flex; align-items: center; gap: 4px; }
.ptab {
  padding: 2px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
}
.ptab.active { background: var(--primary-solid); border-color: var(--primary-solid); color: var(--accent-contrast); }
.preview-state { display: flex; align-items: center; gap: 6px; padding: 12px; font-size: 11px; color: var(--text-secondary); }
.preview-state.error { color: var(--sev-critical-fg); }
.preview-content { flex: 1; min-height: 0; overflow: auto; padding: 8px 10px; background: var(--surface-primary); }
.preview-pre { margin: 0; font-size: 11px; line-height: 1.5; white-space: pre-wrap; overflow-wrap: anywhere; }
.preview-img { max-width: 100%; max-height: 180px; border-radius: var(--radius-sm); }

.spin { animation: diff-spin 1s linear infinite; }
@keyframes diff-spin { to { transform: rotate(360deg); } }
</style>
