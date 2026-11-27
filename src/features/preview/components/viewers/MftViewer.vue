<template>
  <div class="mft-viewer">
    <div v-if="digestLoading" class="state-row mono">
      <Loader2 :size="14" class="spin" />
      {{ tr("正在解析 NTFS $MFT 主文件表…") }}
    </div>
    <div v-else-if="digestError" class="state-row mono error">
      {{ tr("$MFT 解析失败：{0}", { 0: digestError }) }}
    </div>

    <template v-else-if="digest">
      <header class="mft-header">
        <div class="header-top">
          <div class="title-wrap">
            <Table2 :size="15" class="mft-icon" />
            <span class="mft-title mono">$MFT</span>
            <span v-if="digest.volumeName" class="ui-badge sm mono">{{ digest.volumeName }}</span>
            <span v-if="digest.ntfsVersion" class="ui-badge sm mono">NTFS {{ digest.ntfsVersion }}</span>
            <span class="meta-pill mono">{{ tr("记录大小 {0} B", { 0: digest.recordSize }) }}</span>
            <span class="meta-pill mono">{{ tr("总记录 {0} 条", { 0: digest.totalRecords.toLocaleString() }) }}</span>
            <span class="meta-pill mono">{{ formatBytes(digest.fileSize) }}</span>
            <span v-if="digest.indexBuilt" class="meta-pill accent mono">
              {{ tr("索引就绪 {0} 条", { 0: digest.indexedRecords.toLocaleString() }) }}
            </span>
          </div>

          <form class="jump-form" @submit.prevent="jumpToRecord">
            <input
              v-model="jumpInput"
              class="jump-input mono"
              :placeholder="tr('跳转记录 #')"
            />
            <button type="submit" class="ui-btn sm">{{ tr("定位") }}</button>
          </form>
        </div>

        <div v-if="digest.notes.length" class="notes-banner mono">
          <AlertTriangle :size="12" class="warn-icon" />
          <div class="notes-text">
            <div v-for="(note, idx) in digest.notes" :key="idx">{{ note }}</div>
          </div>
        </div>

        <div class="toolbar-row">
          <div class="filter-pills">
            <button
              v-for="pill in filterPills"
              :key="pill.key"
              type="button"
              class="filter-pill"
              :class="{ active: activeFilter === pill.key }"
              @click="selectFilter(pill.key)"
            >
              {{ pill.label }}
            </button>
          </div>

          <form class="search-form" @submit.prevent="applySearch">
            <div class="search-box">
              <Search :size="13" class="search-icon" />
              <input
                v-model="searchInput"
                class="search-input mono"
                :placeholder="tr('搜索文件名、完整路径或 #记录号（回车构建索引检索）…')"
              />
              <button v-if="searchInput" type="button" class="icon-clear" @click="clearSearch">
                <X :size="12" />
              </button>
            </div>
            <button type="submit" class="ui-btn sm">{{ tr("检索") }}</button>
          </form>
        </div>
      </header>

      <div class="mft-body">
        <div class="table-pane">
          <div v-if="listLoading" class="list-loading-bar mono">
            <Loader2 :size="13" class="spin" />
            {{ needsFullScan ? tr("正在扫描全表构建检索索引（大卷首次需数秒）…") : tr("正在读取 MFT 记录…") }}
          </div>
          <div v-if="listError" class="list-error mono">{{ listError }}</div>

          <div class="table-scroll">
            <table class="mft-table">
              <thead>
                <tr>
                  <th class="col-rec">{{ tr("记录号") }}</th>
                  <th class="col-state">{{ tr("状态") }}</th>
                  <th class="col-name">{{ tr("名称") }}</th>
                  <th class="col-path">{{ tr("路径") }}</th>
                  <th class="col-size">{{ tr("大小") }}</th>
                  <th class="col-time">{{ tr("$SI 创建") }}</th>
                  <th class="col-time">{{ tr("$SI 修改") }}</th>
                  <th class="col-time">{{ tr("$FN 创建") }}</th>
                  <th class="col-warn">{{ tr("异常") }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-if="!listLoading && items.length === 0">
                  <td colspan="9" class="empty-cell">{{ tr("无匹配的 MFT 记录") }}</td>
                </tr>
                <tr
                  v-for="row in items"
                  :key="row.recordNumber"
                  :class="{
                    selected: selectedRecord === row.recordNumber,
                    deleted: !row.inUse,
                    suspect: row.timestompSuspect,
                  }"
                  @click="selectRecord(row.recordNumber)"
                >
                  <td class="mono">#{{ row.recordNumber }}</td>
                  <td>
                    <span class="state-badge" :class="row.inUse ? 'live' : 'deleted'">
                      {{ row.inUse ? (row.isDir ? tr("目录") : tr("在用")) : tr("已删除") }}
                    </span>
                  </td>
                  <td class="mono name-cell" :title="row.name">
                    <span class="cell-ellipsis">{{ row.name || tr("（空槽/未命名）") }}</span>
                    <span v-if="row.adsCount > 0" class="ads-tag mono">ADS:{{ row.adsCount }}</span>
                  </td>
                  <td class="mono path-cell" :title="row.fullPath">
                    <span class="cell-ellipsis">{{ row.fullPath || '—' }}</span>
                  </td>
                  <td class="mono size-cell">
                    {{ row.isDir ? '—' : formatBytes(row.logicalSize) }}
                  </td>
                  <td class="mono time-cell">{{ formatIso(row.siCreated) }}</td>
                  <td class="mono time-cell">{{ formatIso(row.siModified) }}</td>
                  <td class="mono time-cell">{{ formatIso(row.fnCreated) }}</td>
                  <td>
                    <span
                      v-if="row.timestompSuspect"
                      class="suspect-badge"
                      :title="row.timestompReasons.join('\n')"
                    >
                      {{ tr("疑似篡改") }}
                    </span>
                    <span v-else class="muted">—</span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <footer class="pager-bar">
            <span class="pager-info mono">
              {{ tr("共 {0} 条 · 第 {1} / {2} 页", { 0: filteredTotal.toLocaleString(), 1: currentPage, 2: totalPages }) }}
            </span>
            <div class="pager-controls">
              <button class="ui-btn sm" type="button" :disabled="currentPage <= 1 || listLoading" @click="goPage(1)">
                {{ tr("首页") }}
              </button>
              <button class="ui-btn sm" type="button" :disabled="currentPage <= 1 || listLoading" @click="goPage(currentPage - 1)">
                {{ tr("上一页") }}
              </button>
              <button class="ui-btn sm" type="button" :disabled="currentPage >= totalPages || listLoading" @click="goPage(currentPage + 1)">
                {{ tr("下一页") }}
              </button>
            </div>
          </footer>
        </div>

        <aside class="detail-pane">
          <div v-if="selectedRecord === null" class="detail-empty">
            {{ tr("点击左侧 MFT 记录查看 $SI / $FN 时间线对照、属性列表与 Data Runs。") }}
          </div>
          <div v-else-if="detailLoading" class="state-row mono">
            <Loader2 :size="13" class="spin" />
            {{ tr("正在读取记录 #{0}…", { 0: selectedRecord }) }}
          </div>
          <div v-else-if="detailError" class="state-row mono error">
            {{ detailError }}
          </div>
          <div v-else-if="detail" class="detail-scroll">
            <div class="detail-head">
              <div class="detail-rec mono">
                #{{ detail.recordNumber }} (seq {{ detail.sequence }})
                <span class="state-badge" :class="detail.inUse ? 'live' : 'deleted'">
                  {{ detail.inUse ? tr("在用") : tr("已删除") }}
                </span>
                <span class="ui-badge sm">{{ detail.isDir ? tr("目录") : tr("文件") }}</span>
              </div>
              <div class="detail-path mono" :title="detail.fullPath">{{ detail.fullPath || '—' }}</div>
              <div class="detail-kv mono">
                <span>LSN: {{ detail.lsn }}</span>
                <span>{{ tr("硬链接: {0}", { 0: detail.hardLinkCount }) }}</span>
                <span>{{ tr("占用: {0}/{1} B", { 0: detail.usedSize, 1: detail.allocatedSize }) }}</span>
                <span v-if="detail.baseRecord > 0">{{ tr("基记录: #{0}", { 0: detail.baseRecord }) }}</span>
              </div>
            </div>

            <div v-if="detail.timestompWarnings.length" class="timestomp-alert">
              <AlertTriangle :size="14" />
              <div>
                <div class="alert-title">{{ tr("检测到 $SI 与 $FN 时间异常（疑似 Timestomping）") }}</div>
                <div v-for="(w, idx) in detail.timestompWarnings" :key="idx" class="alert-line">{{ w }}</div>
              </div>
            </div>

            <div class="section-block">
              <div class="section-title">{{ tr("$SI ($STANDARD_INFORMATION) 与主 $FN 时间对比") }}</div>
              <table class="ts-table">
                <thead>
                  <tr>
                    <th>{{ tr("时间维度") }}</th>
                    <th>$SI (0x10)</th>
                    <th>$FN (0x30)</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="row in timestampCompareRows" :key="row.label">
                    <td>{{ row.label }}</td>
                    <td class="mono" :title="row.siRaw">{{ row.siText }}</td>
                    <td class="mono" :title="row.fnRaw">{{ row.fnText }}</td>
                  </tr>
                </tbody>
              </table>
              <div v-if="detail.standardInfo" class="si-extras mono">
                <span v-if="detail.standardInfo.usn !== null">USN: {{ detail.standardInfo.usn }}</span>
                <span v-if="detail.standardInfo.securityId !== null">SecID: {{ detail.standardInfo.securityId }}</span>
                <span v-if="detail.standardInfo.ownerId !== null">OwnerID: {{ detail.standardInfo.ownerId }}</span>
                <span v-for="flag in detail.standardInfo.fileAttributeFlags" :key="flag" class="flag-tag">{{ flag }}</span>
              </div>
            </div>

            <div v-if="detail.filenames.length" class="section-block">
              <div class="section-title">{{ tr("$FILE_NAME (0x30) 命名空间列表") }}</div>
              <div v-for="(fn, idx) in detail.filenames" :key="idx" class="fn-card">
                <div class="fn-top mono">
                  <span class="ui-badge sm">{{ fn.namespace }}</span>
                  <span class="fn-name">{{ fn.name }}</span>
                  <span class="fn-parent">{{ tr("父记录 #{0}", { 0: fn.parentRecord }) }}</span>
                </div>
                <div class="fn-times mono">
                  <span>C: {{ formatIso(fn.created) }}</span>
                  <span>M: {{ formatIso(fn.modified) }}</span>
                  <span>E: {{ formatIso(fn.mftModified) }}</span>
                  <span>A: {{ formatIso(fn.accessed) }}</span>
                </div>
              </div>
            </div>

            <div class="section-block">
              <div class="section-title">{{ tr("MFT 属性与数据运行段 (Data Runs)") }}</div>
              <div v-for="attr in detail.attributes" :key="`${attr.typeCode}:${attr.instance}`" class="attr-card">
                <div class="attr-head mono">
                  <span class="attr-type">0x{{ attr.typeCode.toString(16).toUpperCase() }} {{ attr.typeName }}</span>
                  <span v-if="attr.name" class="ads-tag">:{{ attr.name }}</span>
                  <span class="ui-badge sm">{{ attr.resident ? tr("常驻") : tr("非常驻") }}</span>
                  <span class="attr-size">{{ formatBytes(attr.logicalSize) }}</span>
                </div>
                <div v-if="attr.dataRuns.length" class="runs-list mono">
                  <span v-for="(run, rIdx) in attr.dataRuns" :key="rIdx" class="run-pill">
                    VCN {{ run.vcnOffset }} · {{ run.clusters }}c → {{ run.lcn !== null ? `LCN ${run.lcn}` : tr("稀疏") }}
                  </span>
                </div>
                <pre v-if="attr.residentPreviewText" class="res-pre mono">{{ attr.residentPreviewText }}</pre>
                <pre v-if="attr.residentPreviewHex" class="res-hex mono">{{ attr.residentPreviewHex }}</pre>
              </div>
            </div>
          </div>
        </aside>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { AlertTriangle, Loader2, Search, Table2, X } from 'lucide-vue-next';
import { tr } from '../../../../composables/useLocale';
import type { ImageId, NodeId } from '../../../../lib/ipc/ids';
import {
  fsMftPreview,
  fsMftRecordDetail,
  fsMftRecords,
  type MftDigest,
  type MftFilterKind,
  type MftRecordDetail,
  type MftRecordSummary,
} from '../../../../lib/ipc/mft';
import { formatPreviewBytes as formatBytes } from '../../model/preview';

const props = defineProps<{
  imageId: ImageId;
  nodeId: NodeId;
}>();

const PAGE_SIZE = 100;

const digestLoading = ref(false);
const digestError = ref<string | null>(null);
const digest = ref<MftDigest | null>(null);

const activeFilter = ref<MftFilterKind>('all');
const searchInput = ref('');
const appliedQuery = ref('');
const jumpInput = ref('');
const currentPage = ref(1);

const listLoading = ref(false);
const listError = ref<string | null>(null);
const filteredTotal = ref(0);
const items = ref<MftRecordSummary[]>([]);

const selectedRecord = ref<number | null>(null);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
const detail = ref<MftRecordDetail | null>(null);

let loadGen = 0;

const filterPills = computed<{ key: MftFilterKind; label: string }[]>(() => [
  { key: 'all', label: tr('全部记录') },
  { key: 'timestomped', label: tr('时间篡改疑点') },
  { key: 'deleted', label: tr('已删除') },
  { key: 'ads', label: tr('含 ADS 交换流') },
  { key: 'resident', label: tr('常驻小文件') },
  { key: 'directories', label: tr('目录') },
]);

const needsFullScan = computed(() =>
  !digest.value?.indexBuilt && (activeFilter.value !== 'all' || appliedQuery.value.length > 0),
);

const totalPages = computed(() =>
  Math.max(1, Math.ceil(filteredTotal.value / PAGE_SIZE)),
);

const primaryFnDetail = computed(() => {
  const fns = detail.value?.filenames ?? [];
  if (!fns.length) return null;
  const weight = (ns: string) =>
    ns === 'Win32' ? 4 : ns === 'Win32&DOS' ? 3 : ns === 'POSIX' ? 2 : 1;
  return [...fns].sort((a, b) => weight(b.namespace) - weight(a.namespace))[0];
});

const timestampCompareRows = computed(() => {
  const si = detail.value?.standardInfo;
  const fn = primaryFnDetail.value;
  return [
    {
      label: tr('创建 (Created)'),
      siText: formatIso(si?.created ?? null),
      siRaw: si ? `RAW FILETIME: ${si.rawFiletimes[0]}` : '',
      fnText: formatIso(fn?.created ?? null),
      fnRaw: fn ? `RAW FILETIME: ${fn.rawFiletimes[0]}` : '',
    },
    {
      label: tr('内容修改 (Modified)'),
      siText: formatIso(si?.modified ?? null),
      siRaw: si ? `RAW FILETIME: ${si.rawFiletimes[1]}` : '',
      fnText: formatIso(fn?.modified ?? null),
      fnRaw: fn ? `RAW FILETIME: ${fn.rawFiletimes[1]}` : '',
    },
    {
      label: tr('MFT 变更 (Entry Mod)'),
      siText: formatIso(si?.mftModified ?? null),
      siRaw: si ? `RAW FILETIME: ${si.rawFiletimes[2]}` : '',
      fnText: formatIso(fn?.mftModified ?? null),
      fnRaw: fn ? `RAW FILETIME: ${fn.rawFiletimes[2]}` : '',
    },
    {
      label: tr('访问 (Accessed)'),
      siText: formatIso(si?.accessed ?? null),
      siRaw: si ? `RAW FILETIME: ${si.rawFiletimes[3]}` : '',
      fnText: formatIso(fn?.accessed ?? null),
      fnRaw: fn ? `RAW FILETIME: ${fn.rawFiletimes[3]}` : '',
    },
  ];
});

function formatIso(iso: string | null): string {
  if (!iso) return '—';
  return iso.replace('T', ' ').replace(/\+00:00$|Z$/, ' UTC');
}

async function initMft() {
  const gen = ++loadGen;
  digestLoading.value = true;
  digestError.value = null;
  digest.value = null;
  selectedRecord.value = null;
  detail.value = null;
  activeFilter.value = 'all';
  searchInput.value = '';
  appliedQuery.value = '';
  currentPage.value = 1;

  try {
    const res = await fsMftPreview({ imageId: props.imageId, nodeId: props.nodeId });
    if (gen !== loadGen) return;
    digest.value = res;
    await fetchPage(gen);
  } catch (err) {
    if (gen !== loadGen) return;
    digestError.value = String(err);
  } finally {
    if (gen === loadGen) digestLoading.value = false;
  }
}

async function fetchPage(gen = loadGen) {
  listLoading.value = true;
  listError.value = null;
  try {
    const offset = (currentPage.value - 1) * PAGE_SIZE;
    const page = await fsMftRecords({
      imageId: props.imageId,
      nodeId: props.nodeId,
      query: appliedQuery.value || null,
      filter: activeFilter.value,
      offset,
      limit: PAGE_SIZE,
    });
    if (gen !== loadGen) return;
    items.value = page.items;
    filteredTotal.value = page.filteredTotal;
    if (digest.value && page.indexed && !digest.value.indexBuilt) {
      digest.value = await fsMftPreview({ imageId: props.imageId, nodeId: props.nodeId });
    }
  } catch (err) {
    if (gen !== loadGen) return;
    listError.value = String(err);
  } finally {
    if (gen === loadGen) listLoading.value = false;
  }
}

async function selectRecord(recordNumber: number) {
  selectedRecord.value = recordNumber;
  detailLoading.value = true;
  detailError.value = null;
  try {
    const res = await fsMftRecordDetail({
      imageId: props.imageId,
      nodeId: props.nodeId,
      recordNumber,
    });
    if (selectedRecord.value === recordNumber) {
      detail.value = res;
    }
  } catch (err) {
    if (selectedRecord.value === recordNumber) {
      detailError.value = String(err);
      detail.value = null;
    }
  } finally {
    if (selectedRecord.value === recordNumber) detailLoading.value = false;
  }
}

function selectFilter(key: MftFilterKind) {
  if (activeFilter.value === key) return;
  activeFilter.value = key;
  currentPage.value = 1;
  void fetchPage();
}

function applySearch() {
  appliedQuery.value = searchInput.value.trim();
  currentPage.value = 1;
  void fetchPage();
}

function clearSearch() {
  searchInput.value = '';
  if (appliedQuery.value !== '') {
    appliedQuery.value = '';
    currentPage.value = 1;
    void fetchPage();
  }
}

function goPage(page: number) {
  currentPage.value = Math.max(1, Math.min(page, totalPages.value));
  void fetchPage();
}

function jumpToRecord() {
  const num = Number.parseInt(jumpInput.value.replace(/^#/, '').trim(), 10);
  if (Number.isNaN(num) || num < 0) return;
  activeFilter.value = 'all';
  appliedQuery.value = '';
  searchInput.value = '';
  currentPage.value = Math.floor(num / PAGE_SIZE) + 1;
  void fetchPage().then(() => selectRecord(num));
}

onMounted(() => void initMft());
watch(() => [props.imageId, props.nodeId], () => void initMft());
</script>

<style scoped>
.mft-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-primary);
  color: var(--text-primary);
  font-size: 12px;
}

.state-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px;
  color: var(--text-secondary);
}
.state-row.error { color: var(--sev-critical-fg); }

.mft-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
}

.header-top,
.toolbar-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.title-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.mft-icon { color: var(--primary-color); }
.mft-title { font-weight: 700; font-size: 13px; }
.meta-pill {
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  font-size: 11px;
}
.meta-pill.accent {
  color: var(--primary-color);
  background: color-mix(in srgb, var(--primary-color) 12%, transparent);
}

.jump-form { display: flex; align-items: center; gap: 4px; }
.jump-input {
  width: 100px;
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-primary);
  font-size: 11px;
}

.notes-banner {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--sev-warn-bg-hover) 50%, transparent);
  color: var(--text-primary);
  font-size: 11px;
}

.filter-pills { display: flex; gap: 4px; flex-wrap: wrap; }
.filter-pill {
  padding: 3px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
}
.filter-pill.active {
  background: var(--primary-solid);
  border-color: var(--primary-solid);
  color: var(--accent-contrast);
}

.search-form { display: flex; align-items: center; gap: 6px; flex: 1; max-width: 440px; }
.search-box {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1;
}
.search-icon { position: absolute; left: 8px; color: var(--text-secondary); }
.search-input {
  width: 100%;
  height: 26px;
  padding: 0 24px 0 26px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-primary);
  font-size: 11px;
}
.icon-clear {
  position: absolute;
  right: 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.mft-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1.45fr) minmax(300px, 1fr);
}

.table-pane {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--border-color);
}
.list-loading-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  font-size: 11px;
}
.list-error { padding: 8px 10px; color: var(--sev-critical-fg); }

.table-scroll { flex: 1; min-height: 0; overflow: auto; }
.mft-table { width: 100%; border-collapse: collapse; table-layout: fixed; }
.mft-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  padding: 6px 8px;
  text-align: left;
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-secondary);
}
.mft-table td {
  padding: 5px 8px;
  border-bottom: 1px solid color-mix(in srgb, var(--border-color) 65%, transparent);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mft-table tbody tr { cursor: pointer; }
.mft-table tbody tr:hover { background: var(--surface-secondary); }
.mft-table tbody tr.selected { background: color-mix(in srgb, var(--primary-color) 14%, transparent); }
.mft-table tbody tr.deleted { opacity: 0.78; }
.mft-table tbody tr.suspect { background: var(--sev-warn-bg); }

.col-rec { width: 72px; }
.col-state { width: 64px; }
.col-name { width: 160px; }
.col-path { width: 210px; }
.col-size { width: 82px; }
.col-time { width: 164px; }
.col-warn { width: 76px; }

.cell-ellipsis { display: inline-block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; vertical-align: middle; }
.state-badge {
  display: inline-block;
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  font-size: 10px;
}
.state-badge.live { background: var(--sev-success-bg); color: var(--sev-success-fg); }
.state-badge.deleted { background: var(--sev-critical-bg); color: var(--sev-critical-fg); }
.ads-tag {
  margin-left: 4px;
  padding: 0 4px;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--primary-color) 16%, transparent);
  color: var(--primary-color);
  font-size: 10px;
}
.suspect-badge {
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  background: var(--sev-warn-bg);
  color: var(--sev-warn-fg);
  font-size: 10px;
  font-weight: 600;
}
.muted { color: var(--text-secondary); }

.pager-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-top: 1px solid var(--border-color);
  background: var(--surface-secondary);
}
.pager-info { color: var(--text-secondary); font-size: 11px; }
.pager-controls { display: flex; gap: 6px; }

.detail-pane { display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
.detail-empty { padding: 18px; color: var(--text-secondary); }
.detail-scroll { flex: 1; min-height: 0; overflow: auto; padding: 10px 12px; display: grid; gap: 12px; align-content: start; }
.detail-head { display: grid; gap: 4px; padding-bottom: 8px; border-bottom: 1px solid var(--border-color); }
.detail-rec { display: flex; align-items: center; gap: 6px; font-weight: 700; }
.detail-path { font-size: 11px; color: var(--text-secondary); overflow-wrap: anywhere; }
.detail-kv { display: flex; flex-wrap: wrap; gap: 10px; font-size: 11px; color: var(--text-secondary); }

.timestomp-alert {
  display: flex;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--sev-warn-border);
  background: var(--sev-warn-bg);
  color: var(--sev-warn-fg);
  font-size: 11px;
}
.alert-title { font-weight: 700; margin-bottom: 2px; }

.section-block { display: grid; gap: 6px; }
.section-title { font-weight: 600; color: var(--text-secondary); font-size: 11px; }

.ts-table { width: 100%; border-collapse: collapse; font-size: 11px; }
.ts-table th,
.ts-table td {
  padding: 4px 6px;
  border: 1px solid var(--border-color);
  text-align: left;
}
.ts-table th { background: var(--surface-secondary); color: var(--text-secondary); }

.si-extras { display: flex; flex-wrap: wrap; gap: 6px; font-size: 11px; color: var(--text-secondary); }
.flag-tag {
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  color: var(--text-primary);
}

.fn-card,
.attr-card {
  padding: 6px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-secondary);
  display: grid;
  gap: 4px;
}
.fn-top,
.attr-head { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: 11px; }
.fn-name,
.attr-type { font-weight: 600; }
.fn-parent,
.attr-size { margin-left: auto; color: var(--text-secondary); }
.fn-times { display: flex; flex-wrap: wrap; gap: 8px; color: var(--text-secondary); font-size: 10px; }

.runs-list { display: flex; flex-wrap: wrap; gap: 4px; }
.run-pill {
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  border: 1px solid var(--border-color);
  font-size: 10px;
}
.res-pre,
.res-hex {
  margin: 0;
  padding: 6px;
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  border: 1px solid var(--border-color);
  font-size: 10px;
  max-height: 120px;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.spin { animation: mft-spin 1s linear infinite; }
@keyframes mft-spin { to { transform: rotate(360deg); } }
</style>
