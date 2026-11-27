<template>
  <div class="evtx-viewer">
    <!-- 加载中 / 错误态 -->
    <div v-if="loading" class="state-row mono">
      <Loader2 :size="14" class="spin" />
      {{ tr("正在解析 Windows 事件日志…") }}
    </div>
    <div v-else-if="error" class="state-row mono error">
      {{ tr("EVTX 解析失败：{0}", { 0: error }) }}
    </div>

    <template v-else-if="digest">
      <!-- 顶部概览栏 -->
      <header class="evtx-header">
        <div class="header-main">
          <div class="title-wrap">
            <ScrollText :size="16" class="evtx-icon" />
            <span class="channel-name mono">{{ digest.channel || tr("Windows 事件日志") }}</span>
            <span class="ui-badge sm mono">{{ digest.totalRecords.toLocaleString() }} {{ tr("条记录") }}</span>
            <span v-if="digest.computer" class="computer-pill mono" :title="tr('计算机')">
              <Laptop :size="12" /> {{ digest.computer }}
            </span>
          </div>

          <div class="time-range mono" v-if="digest.earliestTime || digest.latestTime">
            <Clock :size="12" />
            <span>{{ formatTime(digest.earliestTime) }}</span>
            <span class="range-arrow">→</span>
            <span>{{ formatTime(digest.latestTime) }}</span>
          </div>
        </div>

        <!-- 异常提示 (Dirty / 截断 / 警告) -->
        <div v-if="digest.notes.length" class="notes-banner">
          <AlertTriangle :size="13" class="warn-icon" />
          <div class="notes-text mono">
            <span v-for="(n, idx) in digest.notes" :key="idx">{{ n }} </span>
          </div>
        </div>

        <!-- 快捷级别与事件统计栏 -->
        <div class="stats-bar">
          <div class="level-pills">
            <button
              class="lvl-pill"
              :class="{ active: selectedLevel === null }"
              @click="toggleLevel(null)"
            >
              <span class="pill-dot all" />
              <span>{{ tr("全部") }}</span>
              <span class="pill-cnt mono">{{ digest.totalRecords }}</span>
            </button>
            <button
              v-for="lvl in digest.levelDistribution"
              :key="lvl.level"
              class="lvl-pill"
              :class="[levelClass(lvl.level), { active: selectedLevel === lvl.level }]"
              @click="toggleLevel(lvl.level)"
            >
              <span class="pill-dot" :class="levelClass(lvl.level)" />
              <span>{{ lvl.name.split(' ')[0] }}</span>
              <span class="pill-cnt mono">{{ lvl.count }}</span>
            </button>
          </div>

          <!-- 高频事件快速筛选 -->
          <div class="event-chips" v-if="digest.eventIdDistribution.length > 0">
            <span class="chips-label">{{ tr("常见事件：") }}</span>
            <button
              v-for="item in digest.eventIdDistribution.slice(0, 8)"
              :key="item.eventId"
              class="event-chip mono"
              :class="{ active: selectedEventId === item.eventId }"
              :title="item.title"
              @click="toggleEventId(item.eventId)"
            >
              <span class="chip-id">{{ item.eventId }}</span>
              <span class="chip-cnt">{{ item.count }}</span>
            </button>
          </div>
        </div>

        <!-- 过滤工具栏 -->
        <div class="filter-toolbar">
          <div class="search-box">
            <Search :size="13" class="search-icon" />
            <input
              v-model="searchInput"
              class="search-input mono"
              :placeholder="tr('搜索事件 ID、来源、进程、用户名、IP 等…')"
              @keydown.enter="applyFilters"
            />
            <button v-if="searchInput" class="clear-btn" @click="clearSearch">
              <X :size="12" />
            </button>
          </div>

          <div class="filter-actions">
            <button
              v-if="hasActiveFilter"
              class="ui-btn sm mono"
              @click="resetAllFilters"
            >
              <RotateCcw :size="12" />
              {{ tr("重置筛选") }}
            </button>

            <button
              class="ui-btn sm mono"
              :class="{ active: useUtc }"
              :title="useUtc ? tr('切换为本地时间') : tr('切换为 UTC 时间')"
              @click="useUtc = !useUtc"
            >
              UTC
            </button>
          </div>
        </div>
      </header>

      <!-- 主视图：左侧列表 + 右侧详情 -->
      <div class="evtx-body">
        <!-- 列表表格 -->
        <div class="table-pane">
          <div class="table-scroll">
            <table class="data-table evtx-table">
              <thead>
                <tr>
                  <th class="th-id mono">{{ tr("序号") }}</th>
                  <th class="th-time mono">{{ tr("时间") }}</th>
                  <th class="th-level mono">{{ tr("级别") }}</th>
                  <th class="th-eid mono">{{ tr("事件 ID") }}</th>
                  <th class="th-title">{{ tr("任务 / 名称") }}</th>
                  <th class="th-summary">{{ tr("事件摘要") }}</th>
                  <th class="th-provider">{{ tr("来源") }}</th>
                </tr>
                <tr class="col-filter-row">
                  <th>
                    <input
                      v-model="recordIdFilter"
                      class="col-filter-input mono"
                      :placeholder="tr('如 1-50')"
                    />
                  </th>
                  <th>
                    <input
                      v-model="timeFilter"
                      class="col-filter-input mono"
                      :placeholder="tr('筛选时间…')"
                    />
                  </th>
                  <th>
                    <select
                      :value="selectedLevel ?? ''"
                      class="col-filter-select mono"
                      @change="onLevelSelectChange"
                    >
                      <option value="">{{ tr("全部") }}</option>
                      <option :value="1">{{ tr("重大") }}</option>
                      <option :value="2">{{ tr("错误") }}</option>
                      <option :value="3">{{ tr("警告") }}</option>
                      <option :value="4">{{ tr("信息") }}</option>
                      <option :value="5">{{ tr("详细") }}</option>
                    </select>
                  </th>
                  <th>
                    <input
                      v-model="eventIdInput"
                      class="col-filter-input mono"
                      :placeholder="tr('如 4624')"
                      @keydown.enter="onEventIdFilterCommit"
                      @blur="onEventIdFilterCommit"
                    />
                  </th>
                  <th>
                    <input
                      v-model="titleFilter"
                      class="col-filter-input mono"
                      :placeholder="tr('筛选任务…')"
                    />
                  </th>
                  <th>
                    <input
                      v-model="summaryFilter"
                      class="col-filter-input mono"
                      :placeholder="tr('筛选摘要…')"
                    />
                  </th>
                  <th>
                    <input
                      v-model="providerFilter"
                      class="col-filter-input mono"
                      :placeholder="tr('筛选来源…')"
                    />
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="record in displayedRecords"
                  :key="record.recordId"
                  class="evtx-row"
                  :class="{ active: selectedRecord?.recordId === record.recordId }"
                  @click="selectRecord(record)"
                >
                  <td class="td-id mono">{{ record.recordId }}</td>
                  <td class="td-time mono" :title="record.timestamp">
                    {{ formatTime(record.timestamp) }}
                  </td>
                  <td class="td-level">
                    <span class="level-tag" :class="levelClass(record.level)">
                      {{ levelShort(record.level) }}
                    </span>
                  </td>
                  <td class="td-eid mono font-bold">{{ record.eventId }}</td>
                  <td class="td-title" :title="record.title">
                    <div class="cell-ellipsis">{{ record.title }}</div>
                  </td>
                  <td class="td-summary" :title="record.summary">
                    <div class="cell-ellipsis mono">{{ record.summary }}</div>
                  </td>
                  <td class="td-provider" :title="record.provider">
                    <div class="cell-ellipsis mono">{{ record.provider }}</div>
                  </td>
                </tr>
                <tr v-if="recordsLoading && !displayedRecords.length">
                  <td colspan="7" class="loading-td mono">
                    <Loader2 :size="14" class="spin" />
                    {{ tr("正在加载记录…") }}
                  </td>
                </tr>
                <tr v-else-if="!displayedRecords.length">
                  <td colspan="7" class="empty-td mono">
                    {{ tr("无匹配事件记录") }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <!-- 分页底栏 -->
          <footer class="table-pager">
            <div class="pager-info mono">
              <Loader2 v-if="recordsLoading" :size="12" class="spin" />
              <span>{{ rangeStart }}–{{ rangeEnd }} / {{ filteredTotal.toLocaleString() }}</span>
              <span v-if="filteredTotal !== digest.totalRecords" class="filtered-from">
                ({{ tr("筛选自") }} {{ digest.totalRecords.toLocaleString() }})
              </span>
            </div>

            <div class="pager-controls">
              <select v-model.number="pageSize" class="page-size-select mono" @change="onPageSizeChange">
                <option :value="50">50 {{ tr("条/页") }}</option>
                <option :value="100">100 {{ tr("条/页") }}</option>
                <option :value="200">200 {{ tr("条/页") }}</option>
                <option :value="500">500 {{ tr("条/页") }}</option>
              </select>

              <button
                class="ui-icon-btn sm bordered"
                :title="tr('上一页')"
                :disabled="pageIndex <= 0 || recordsLoading"
                @click="prevPage"
              >
                <ChevronLeft :size="14" />
              </button>

              <span class="pager-page mono">{{ pageIndex + 1 }} / {{ pageCount }}</span>

              <button
                class="ui-icon-btn sm bordered"
                :title="tr('下一页')"
                :disabled="pageIndex >= pageCount - 1 || recordsLoading"
                @click="nextPage"
              >
                <ChevronRight :size="14" />
              </button>
            </div>
          </footer>
        </div>

        <!-- 详细信息检查器面板 -->
        <aside class="inspector-pane" v-if="selectedRecord">
          <header class="inspector-header">
            <div class="rec-title-wrap">
              <span class="rec-id mono">#{{ selectedRecord.recordId }}</span>
              <span class="rec-name" :title="selectedRecord.title">{{ selectedRecord.title }}</span>
            </div>
            <div class="inspector-actions">
              <button
                class="ui-icon-btn sm"
                :title="tr('复制原始 XML')"
                @click="copyXml"
              >
                <Copy :size="13" />
              </button>
              <button
                class="ui-icon-btn sm"
                :title="tr('关闭详情')"
                @click="selectedRecord = null"
              >
                <X :size="13" />
              </button>
            </div>
          </header>

          <div class="inspector-tabs ui-seg">
            <button
              class="ui-seg-item"
              :class="{ active: inspectorTab === 'data' }"
              @click="inspectorTab = 'data'"
            >
              {{ tr("事件数据") }} ({{ selectedRecord.eventData.length }})
            </button>
            <button
              class="ui-seg-item"
              :class="{ active: inspectorTab === 'xml' }"
              @click="inspectorTab = 'xml'"
            >
              {{ tr("XML 源码") }}
            </button>
            <button
              class="ui-seg-item"
              :class="{ active: inspectorTab === 'meta' }"
              @click="inspectorTab = 'meta'"
            >
              {{ tr("元属性") }}
            </button>
          </div>

          <div class="inspector-body">
            <!-- 结构化参数表格 -->
            <div v-if="inspectorTab === 'data'" class="data-fields-wrap">
              <div v-if="!selectedRecord.eventData.length" class="empty-state mono">
                {{ tr("此事件未附带 EventData / UserData 参数") }}
              </div>
              <table v-else class="data-fields-table">
                <tbody>
                  <tr
                    v-for="(field, idx) in selectedRecord.eventData"
                    :key="idx"
                    class="field-row"
                    @click="copyField(field.value, field.name)"
                  >
                    <td class="field-name mono" :title="field.name">{{ field.name }}</td>
                    <td class="field-val mono" :title="field.value">{{ field.value || '—' }}</td>
                  </tr>
                </tbody>
              </table>
            </div>

            <!-- 原始 XML 视图 -->
            <div v-else-if="inspectorTab === 'xml'" class="xml-wrap">
              <pre class="xml-code mono"><code>{{ selectedRecord.xml }}</code></pre>
            </div>

            <!-- 元属性 -->
            <div v-else-if="inspectorTab === 'meta'" class="meta-fields-wrap">
              <table class="data-fields-table">
                <tbody>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("记录序号") }}</td>
                    <td class="field-val mono">{{ selectedRecord.recordId }}</td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("事件 ID") }}</td>
                    <td class="field-val mono font-bold">{{ selectedRecord.eventId }}</td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("严重级别") }}</td>
                    <td class="field-val mono">
                      <span class="level-tag" :class="levelClass(selectedRecord.level)">
                        {{ selectedRecord.levelName }}
                      </span>
                    </td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("生成时间") }}</td>
                    <td class="field-val mono">{{ selectedRecord.timestamp }}</td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("事件来源") }}</td>
                    <td class="field-val mono">{{ selectedRecord.provider || '—' }}</td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("日志通道") }}</td>
                    <td class="field-val mono">{{ selectedRecord.channel || '—' }}</td>
                  </tr>
                  <tr class="field-row">
                    <td class="field-name mono">{{ tr("计算机名称") }}</td>
                    <td class="field-val mono">{{ selectedRecord.computer || '—' }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </aside>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue';
import { tr } from '../../../../composables/useLocale';
import { copyText } from '../../../../composables/clipboard';
import type { ImageId, NodeId } from '../../../../lib/ipc/ids';
import {
  fsEvtxPreview,
  fsEvtxRecords,
  type EvtxDigest,
  type EvtxRecordItem,
} from '../../../../lib/ipc/evtx';
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  Clock,
  Copy,
  Laptop,
  Loader2,
  RotateCcw,
  ScrollText,
  Search,
  X,
} from 'lucide-vue-next';

const props = defineProps<{
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  query?: string;
}>();

const loading = ref(true);
const error = ref<string | null>(null);
const digest = ref<EvtxDigest | null>(null);

const recordsLoading = ref(false);
const records = ref<EvtxRecordItem[]>([]);
const filteredTotal = ref(0);
const selectedRecord = ref<EvtxRecordItem | null>(null);

const searchInput = ref(props.query || '');
const activeQuery = ref(props.query || '');
const selectedLevel = ref<number | null>(null);
const selectedEventId = ref<number | null>(null);
const eventIdInput = ref('');
const recordIdFilter = ref('');
const timeFilter = ref('');
const titleFilter = ref('');
const summaryFilter = ref('');
const providerFilter = ref('');
const pageIndex = ref(0);
const pageSize = ref(100);
const useUtc = ref(false);
const inspectorTab = ref<'data' | 'xml' | 'meta'>('data');

let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;
watch(searchInput, val => {
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    const trimmed = val.trim();
    if (trimmed !== activeQuery.value) {
      activeQuery.value = trimmed;
      pageIndex.value = 0;
      void loadRecords();
    }
  }, 200);
});

const hasActiveFilter = computed(() => {
  return (
    !!activeQuery.value ||
    selectedLevel.value !== null ||
    selectedEventId.value !== null ||
    !!eventIdInput.value.trim() ||
    !!recordIdFilter.value.trim() ||
    !!timeFilter.value.trim() ||
    !!titleFilter.value.trim() ||
    !!summaryFilter.value.trim() ||
    !!providerFilter.value.trim()
  );
});

const displayedRecords = computed(() => {
  const rf = recordIdFilter.value.trim();
  const tf = timeFilter.value.trim().toLowerCase();
  const titf = titleFilter.value.trim().toLowerCase();
  const sumf = summaryFilter.value.trim().toLowerCase();
  const pf = providerFilter.value.trim().toLowerCase();
  const eidsRaw = eventIdInput.value.trim();
  const multiEids =
    eidsRaw.includes(',') || eidsRaw.includes(' ')
      ? eidsRaw
          .split(/[,，\s]+/)
          .map(s => Number(s.trim()))
          .filter(n => !Number.isNaN(n) && n > 0)
      : [];

  let recMin: number | null = null;
  let recMax: number | null = null;
  if (rf) {
    const m = rf.match(/^(\d+)\s*-\s*(\d+)$/);
    if (m) {
      recMin = Number(m[1]);
      recMax = Number(m[2]);
    } else if (/^\d+$/.test(rf)) {
      recMin = Number(rf);
      recMax = Number(rf);
    }
  }

  return records.value.filter(rec => {
    if (recMin !== null && recMax !== null && (rec.recordId < recMin || rec.recordId > recMax)) {
      return false;
    }
    if (multiEids.length > 0 && !multiEids.includes(rec.eventId)) {
      return false;
    }
    if (tf) {
      const formatted = formatTime(rec.timestamp).toLowerCase();
      const rawTs = (rec.timestamp || '').toLowerCase();
      if (!formatted.includes(tf) && !rawTs.includes(tf)) return false;
    }
    if (titf && !(rec.title || '').toLowerCase().includes(titf)) return false;
    if (sumf && !(rec.summary || '').toLowerCase().includes(sumf)) return false;
    if (pf && !(rec.provider || '').toLowerCase().includes(pf)) return false;
    return true;
  });
});

const pageCount = computed(() => {
  if (filteredTotal.value <= 0) return 1;
  return Math.max(1, Math.ceil(filteredTotal.value / pageSize.value));
});

const rangeStart = computed(() => {
  if (filteredTotal.value === 0) return 0;
  return pageIndex.value * pageSize.value + 1;
});

const rangeEnd = computed(() => {
  return Math.min(filteredTotal.value, (pageIndex.value + 1) * pageSize.value);
});

async function loadDigest() {
  loading.value = true;
  error.value = null;
  try {
    const res = await fsEvtxPreview({
      imageId: props.imageId,
      nodeId: props.nodeId,
    });
    digest.value = res;
    pageIndex.value = 0;
    await loadRecords();
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

async function loadRecords() {
  recordsLoading.value = true;
  try {
    const page = await fsEvtxRecords({
      imageId: props.imageId,
      nodeId: props.nodeId,
      query: activeQuery.value || null,
      level: selectedLevel.value,
      eventId: selectedEventId.value,
      offset: pageIndex.value * pageSize.value,
      limit: pageSize.value,
    });
    records.value = page.records;
    filteredTotal.value = page.filteredTotal;
    if (page.records.length > 0 && !selectedRecord.value) {
      selectedRecord.value = page.records[0];
    }
  } catch (err) {
    error.value = String(err);
  } finally {
    recordsLoading.value = false;
  }
}

function selectRecord(rec: EvtxRecordItem) {
  selectedRecord.value = rec;
}

function toggleLevel(level: number | null) {
  selectedLevel.value = selectedLevel.value === level ? null : level;
  pageIndex.value = 0;
  void loadRecords();
}

function toggleEventId(eventId: number) {
  selectedEventId.value = selectedEventId.value === eventId ? null : eventId;
  eventIdInput.value = selectedEventId.value != null ? String(selectedEventId.value) : '';
  pageIndex.value = 0;
  void loadRecords();
}

function onLevelSelectChange(e: Event) {
  const raw = (e.target as HTMLSelectElement).value;
  selectedLevel.value = raw === '' ? null : Number(raw);
  pageIndex.value = 0;
  void loadRecords();
}

function onEventIdFilterCommit() {
  const raw = eventIdInput.value.trim();
  const single = /^\d+$/.test(raw) ? Number(raw) : null;
  if (single !== selectedEventId.value) {
    selectedEventId.value = single;
    pageIndex.value = 0;
    void loadRecords();
  }
}

function applyFilters() {
  activeQuery.value = searchInput.value.trim();
  onEventIdFilterCommit();
  pageIndex.value = 0;
  void loadRecords();
}

function clearSearch() {
  searchInput.value = '';
  activeQuery.value = '';
  pageIndex.value = 0;
  void loadRecords();
}

function resetAllFilters() {
  searchInput.value = '';
  activeQuery.value = '';
  selectedLevel.value = null;
  selectedEventId.value = null;
  eventIdInput.value = '';
  recordIdFilter.value = '';
  timeFilter.value = '';
  titleFilter.value = '';
  summaryFilter.value = '';
  providerFilter.value = '';
  pageIndex.value = 0;
  void loadRecords();
}

function prevPage() {
  if (pageIndex.value > 0) {
    pageIndex.value -= 1;
    void loadRecords();
  }
}

function nextPage() {
  if (pageIndex.value < pageCount.value - 1) {
    pageIndex.value += 1;
    void loadRecords();
  }
}

function onPageSizeChange() {
  pageIndex.value = 0;
  void loadRecords();
}

function formatTime(isoStr?: string | null): string {
  if (!isoStr) return '—';
  try {
    const date = new Date(isoStr);
    if (isNaN(date.getTime())) return isoStr;
    if (useUtc.value) {
      return date.toISOString().replace('T', ' ').replace('Z', ' UTC');
    }
    const y = date.getFullYear();
    const m = String(date.getMonth() + 1).padStart(2, '0');
    const d = String(date.getDate()).padStart(2, '0');
    const h = String(date.getHours()).padStart(2, '0');
    const min = String(date.getMinutes()).padStart(2, '0');
    const s = String(date.getSeconds()).padStart(2, '0');
    return `${y}-${m}-${d} ${h}:${min}:${s}`;
  } catch {
    return isoStr;
  }
}

function levelClass(level: number): string {
  switch (level) {
    case 1:
      return 'lvl-critical';
    case 2:
      return 'lvl-error';
    case 3:
      return 'lvl-warn';
    case 4:
      return 'lvl-info';
    case 5:
      return 'lvl-verbose';
    default:
      return 'lvl-normal';
  }
}

function levelShort(level: number): string {
  switch (level) {
    case 1:
      return tr('重大');
    case 2:
      return tr('错误');
    case 3:
      return tr('警告');
    case 4:
      return tr('信息');
    case 5:
      return tr('详细');
    default:
      return tr('常规');
  }
}

async function copyXml() {
  if (selectedRecord.value?.xml) {
    await copyText(selectedRecord.value.xml, tr("事件原始 XML"));
  }
}

async function copyField(value: string, name: string) {
  if (value) {
    await copyText(value, name);
  }
}

watch(
  () => props.nodeId,
  () => {
    selectedRecord.value = null;
    void loadDigest();
  }
);

watch(
  () => props.query,
  (newQ) => {
    if (newQ !== undefined && newQ !== activeQuery.value) {
      searchInput.value = newQ;
      activeQuery.value = newQ;
      pageIndex.value = 0;
      void loadRecords();
    }
  }
);

onMounted(() => {
  void loadDigest();
});
</script>

<style scoped>
.evtx-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  overflow: hidden;
  background: var(--surface-primary);
  color: var(--text-primary);
}

.state-row {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 32px;
  color: var(--text-secondary);
}

.state-row.error {
  color: var(--sev-critical-fg);
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* 顶部概览栏 */
.evtx-header {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
}

.header-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}

.evtx-icon {
  color: var(--primary-color);
}

.channel-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary);
}

.computer-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 1px 7px;
  background: var(--surface-tertiary);
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.time-range {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--text-secondary);
}

.range-arrow {
  opacity: 0.5;
}

.notes-banner {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  background: var(--sev-warn-bg);
  border: 1px solid var(--sev-warn-border);
  border-radius: var(--radius-sm);
  color: var(--sev-warn-fg);
  font-size: 11px;
}

.stats-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.level-pills {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.lvl-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 7px;
  font-size: 11px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: var(--text-secondary);
  transition: all 0.15s ease;
}

.lvl-pill:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.lvl-pill.active {
  background: var(--active-bg);
  border-color: var(--primary-color);
  color: var(--text-primary);
  font-weight: 500;
}

.pill-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.pill-dot.all { background: var(--text-secondary); }
.pill-dot.lvl-critical { background: var(--sev-critical-fg); }
.pill-dot.lvl-error { background: var(--sev-critical-fg); }
.pill-dot.lvl-warn { background: var(--sev-warn-fg); }
.pill-dot.lvl-info { background: var(--sev-info-fg); }
.pill-dot.lvl-verbose { background: var(--text-secondary); }

.pill-cnt {
  font-size: 10px;
  opacity: 0.8;
}

.event-chips {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
}

.chips-label {
  font-size: 11px;
  color: var(--text-secondary);
}

.event-chip {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 6px;
  font-size: 10px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  cursor: pointer;
  color: var(--text-secondary);
  transition: all 0.15s ease;
}

.event-chip:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.event-chip.active {
  background: var(--primary-solid);
  border-color: var(--primary-solid);
  color: var(--accent-contrast);
}

.chip-cnt {
  opacity: 0.7;
}

/* 过滤工具栏 */
.filter-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.search-box {
  position: relative;
  flex: 1;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 8px;
  color: var(--text-secondary);
  pointer-events: none;
}

.search-input {
  width: 100%;
  height: 26px;
  padding: 0 24px 0 26px;
  font-size: 12px;
  background: var(--surface-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  color: var(--text-primary);
  outline: none;
}

.search-input:focus {
  border-color: var(--primary-color);
}

.clear-btn {
  position: absolute;
  right: 6px;
  background: transparent;
  border: none;
  cursor: pointer;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  padding: 2px;
}

.filter-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

/* 主内容区 */
.evtx-body {
  flex: 1;
  display: flex;
  min-height: 0;
  overflow: hidden;
}

.table-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  border-right: 1px solid var(--border-color);
}

.table-scroll {
  flex: 1;
  overflow: auto;
}

.evtx-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.evtx-table th {
  position: sticky;
  top: 0;
  z-index: 2;
  background: var(--surface-secondary);
  padding: 6px 8px;
  text-align: left;
  font-weight: 500;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border-color);
  white-space: nowrap;
}

.col-filter-row th {
  top: 29px;
  padding: 4px 6px;
  background: var(--surface-primary);
}

.col-filter-input,
.col-filter-select {
  width: 100%;
  height: 22px;
  padding: 0 5px;
  font-size: 11px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-primary);
  outline: none;
}

.col-filter-input:focus,
.col-filter-select:focus {
  border-color: var(--primary-color);
}

.evtx-row {
  cursor: pointer;
  transition: background 0.1s ease;
}

.evtx-row:hover {
  background: var(--hover-bg);
}

.evtx-row.active {
  background: var(--active-bg);
}

.evtx-table td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-color);
  color: var(--text-primary);
  vertical-align: middle;
}

.th-id, .td-id { width: 55px; color: var(--text-secondary); }
.th-time, .td-time { width: 140px; white-space: nowrap; }
.th-level, .td-level { width: 55px; }
.th-eid, .td-eid { width: 65px; }
.th-title, .td-title { width: 180px; }
.th-provider, .td-provider { width: 130px; }

.cell-ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.level-tag {
  display: inline-block;
  padding: 1px 4px;
  border-radius: var(--radius-sm);
  font-size: 10px;
  font-weight: 500;
  white-space: nowrap;
}

.level-tag.lvl-critical {
  background: var(--sev-critical-bg);
  color: var(--sev-critical-fg);
  border: 1px solid var(--sev-critical-border);
}

.level-tag.lvl-error {
  background: var(--sev-critical-bg);
  color: var(--sev-critical-fg);
  border: 1px solid var(--sev-critical-border);
}

.level-tag.lvl-warn {
  background: var(--sev-warn-bg);
  color: var(--sev-warn-fg);
  border: 1px solid var(--sev-warn-border);
}

.level-tag.lvl-info {
  background: var(--sev-info-bg);
  color: var(--sev-info-fg);
  border: 1px solid var(--sev-info-border);
}

.level-tag.lvl-verbose,
.level-tag.lvl-normal {
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.loading-td, .empty-td {
  padding: 40px;
  text-align: center;
  color: var(--text-secondary);
}

/* 分页底栏 */
.table-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  border-top: 1px solid var(--border-color);
  background: var(--surface-secondary);
  font-size: 11px;
}

.pager-info {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
}

.filtered-from {
  opacity: 0.65;
}

.pager-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}

.page-size-select {
  height: 22px;
  padding: 0 4px;
  font-size: 11px;
  background: var(--surface-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
}

.pager-page {
  font-size: 11px;
  color: var(--text-secondary);
}

/* 详情检查器 */
.inspector-pane {
  width: 380px;
  min-width: 300px;
  max-width: 50%;
  display: flex;
  flex-direction: column;
  background: var(--surface-secondary);
  overflow: hidden;
}

.inspector-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
}

.rec-title-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.rec-id {
  font-size: 12px;
  font-weight: 600;
  color: var(--primary-color);
}

.rec-name {
  font-size: 12px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.inspector-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.inspector-tabs {
  margin: 6px 12px;
}

.inspector-body {
  flex: 1;
  overflow: auto;
  padding: 8px 12px;
}

.data-fields-wrap, .meta-fields-wrap {
  height: 100%;
}

.data-fields-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.field-row {
  border-bottom: 1px solid var(--border-color);
  cursor: copy;
}

.field-row:hover {
  background: var(--hover-bg);
}

.field-name {
  width: 110px;
  padding: 5px 6px;
  color: var(--text-secondary);
  vertical-align: top;
  word-break: break-all;
}

.field-val {
  padding: 5px 6px;
  color: var(--text-primary);
  word-break: break-all;
}

.xml-wrap {
  height: 100%;
}

.xml-code {
  margin: 0;
  padding: 8px;
  font-size: 11px;
  line-height: 1.5;
  background: var(--surface-tertiary);
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 100%;
  overflow: auto;
}

.empty-state {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 11px;
}
</style>
