<template>
  <div class="structured-log-viewer">
    <header class="log-header">
      <div class="header-main">
        <div class="title-wrap">
          <FileText :size="15" class="log-icon" />
          <span class="file-name mono">{{ filename }}</span>
          <span class="ui-badge sm mono">{{ parsed.formatLabel }}</span>
          <span class="ui-badge sm mono">{{ parsed.entries.length.toLocaleString() }} {{ tr("条日志") }}</span>
        </div>

        <div class="header-right">
          <div v-if="parsed.earliestTime || parsed.latestTime" class="time-range mono">
            <Clock :size="12" />
            <span>{{ parsed.earliestTime || "—" }}</span>
            <span class="range-arrow">→</span>
            <span>{{ parsed.latestTime || "—" }}</span>
          </div>
          <button
            type="button"
            class="ui-btn sm mono"
            :title="tr('切换为原始文本/代码视图')"
            @click="emit('switch-code')"
          >
            <Code2 :size="12" />
            {{ tr("原始文本") }}
          </button>
        </div>
      </div>

      <!-- 级别与高频模块快速筛选 -->
      <div class="stats-bar">
        <div class="level-pills">
          <button
            type="button"
            class="lvl-pill"
            :class="{ active: selectedLevel === '' }"
            @click="selectedLevel = ''"
          >
            <span class="pill-dot all" />
            <span>{{ tr("全部") }}</span>
            <span class="pill-cnt mono">{{ parsed.entries.length }}</span>
          </button>
          <button
            v-for="lvl in levelItems"
            :key="lvl.key"
            type="button"
            class="lvl-pill"
            :class="[lvl.cls, { active: selectedLevel === lvl.key }]"
            @click="selectedLevel = selectedLevel === lvl.key ? '' : lvl.key"
          >
            <span class="pill-dot" :class="lvl.cls" />
            <span>{{ lvl.label }}</span>
            <span class="pill-cnt mono">{{ lvl.count }}</span>
          </button>
        </div>

        <div v-if="parsed.topSources.length > 0" class="source-chips">
          <span class="chips-label">{{ tr("模块/分类：") }}</span>
          <button
            v-for="item in parsed.topSources.slice(0, 8)"
            :key="item.source"
            type="button"
            class="src-chip mono"
            :class="{ active: sourceFilter.toLowerCase() === item.source.toLowerCase() }"
            @click="toggleSourceChip(item.source)"
          >
            <span>{{ item.source }}</span>
            <span class="chip-cnt">{{ item.count }}</span>
          </button>
        </div>
      </div>

      <!-- 搜索与排除工具栏 -->
      <div class="filter-toolbar">
        <div class="search-box">
          <Search :size="13" class="search-icon" />
          <input
            v-model="keywordInput"
            class="search-input mono"
            :placeholder="tr('搜索时间、模块或日志内容（支持 /正则/i）…')"
          />
          <button v-if="keywordInput" type="button" class="clear-btn" @click="keywordInput = ''">
            <X :size="12" />
          </button>
        </div>

        <div class="search-box exclude-box">
          <Filter :size="12" class="search-icon" />
          <input
            v-model="excludeInput"
            class="search-input mono"
            :placeholder="tr('排除关键词（逗号分隔）…')"
          />
          <button v-if="excludeInput" type="button" class="clear-btn" @click="excludeInput = ''">
            <X :size="12" />
          </button>
        </div>

        <button
          v-if="hasActiveFilter"
          type="button"
          class="ui-btn sm mono"
          @click="resetAllFilters"
        >
          <RotateCcw :size="12" />
          {{ tr("重置筛选") }}
        </button>
      </div>
    </header>

    <div class="log-body">
      <div class="table-scroll">
        <table class="data-table log-table">
          <thead>
            <tr>
              <th class="col-line mono">{{ tr("行号") }}</th>
              <th class="col-time mono">{{ tr("时间") }}</th>
              <th class="col-level mono">{{ tr("级别") }}</th>
              <th class="col-source mono">{{ tr("模块 / 来源") }}</th>
              <th class="col-msg">{{ tr("日志信息") }}</th>
            </tr>
            <!-- 列级筛选输入行 -->
            <tr class="col-filter-row">
              <th>
                <input
                  v-model="lineFilter"
                  class="col-filter-input mono"
                  :placeholder="tr('如 1-200')"
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
                <select v-model="selectedLevel" class="col-filter-select mono">
                  <option value="">{{ tr("全部") }}</option>
                  <option value="ERROR">ERROR</option>
                  <option value="WARN">WARN</option>
                  <option value="INFO">INFO</option>
                  <option value="DEBUG">DEBUG</option>
                  <option value="OTHER">OTHER</option>
                </select>
              </th>
              <th>
                <input
                  v-model="sourceFilter"
                  class="col-filter-input mono"
                  :placeholder="tr('筛选模块…')"
                />
              </th>
              <th>
                <input
                  v-model="messageFilter"
                  class="col-filter-input mono"
                  :placeholder="tr('筛选日志正文内容…')"
                />
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="item in pagedEntries"
              :key="item.index"
              class="log-row"
              :class="{ active: selectedEntry?.index === item.index }"
              @click="selectedEntry = item"
            >
              <td class="td-line mono">
                {{ item.lineStart === item.lineEnd ? item.lineStart : `${item.lineStart}-${item.lineEnd}` }}
              </td>
              <td class="td-time mono" :title="item.timestamp">{{ item.timestamp }}</td>
              <td class="td-level">
                <span class="level-tag mono" :class="levelClass(item.level)">{{ item.level }}</span>
              </td>
              <td class="td-source mono" :title="item.source">
                <div class="cell-ellipsis">{{ item.source }}</div>
              </td>
              <td class="td-msg mono" :title="item.message">
                <div class="cell-ellipsis">{{ firstLine(item.message) }}</div>
                <span v-if="item.lineEnd > item.lineStart" class="multiline-pill">
                  +{{ item.lineEnd - item.lineStart }} {{ tr("行") }}
                </span>
              </td>
            </tr>
            <tr v-if="filteredEntries.length === 0">
              <td colspan="5" class="empty-td mono">
                {{ tr("无匹配日志记录") }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <footer class="table-pager">
        <div class="pager-info mono">
          <span>{{ rangeStart }}–{{ rangeEnd }} / {{ filteredEntries.length.toLocaleString() }}</span>
          <span v-if="filteredEntries.length !== parsed.entries.length" class="filtered-from">
            ({{ tr("筛选自") }} {{ parsed.entries.length.toLocaleString() }})
          </span>
        </div>

        <div class="pager-controls">
          <select v-model.number="pageSize" class="page-size-select mono" @change="pageIndex = 0">
            <option :value="100">100 {{ tr("条/页") }}</option>
            <option :value="200">200 {{ tr("条/页") }}</option>
            <option :value="500">500 {{ tr("条/页") }}</option>
            <option :value="1000">1000 {{ tr("条/页") }}</option>
          </select>
          <button
            type="button"
            class="ui-icon-btn sm bordered"
            :disabled="pageIndex <= 0"
            @click="pageIndex = Math.max(0, pageIndex - 1)"
          >
            <ChevronLeft :size="14" />
          </button>
          <span class="page-num mono">{{ pageIndex + 1 }} / {{ pageCount }}</span>
          <button
            type="button"
            class="ui-icon-btn sm bordered"
            :disabled="pageIndex >= pageCount - 1"
            @click="pageIndex = Math.min(pageCount - 1, pageIndex + 1)"
          >
            <ChevronRight :size="14" />
          </button>
        </div>
      </footer>

      <!-- 底部选中行详情与多行堆栈查看器 -->
      <div v-if="selectedEntry" class="log-inspector">
        <div class="inspector-head mono">
          <div class="inspector-meta">
            <span>#{{ selectedEntry.index }} (L{{ selectedEntry.lineStart }}<template v-if="selectedEntry.lineEnd > selectedEntry.lineStart">–L{{ selectedEntry.lineEnd }}</template>)</span>
            <span class="level-tag" :class="levelClass(selectedEntry.level)">{{ selectedEntry.level }}</span>
            <span>{{ selectedEntry.timestamp }}</span>
            <span v-if="selectedEntry.source !== '—'" class="src-badge">{{ selectedEntry.source }}</span>
          </div>
          <div class="inspector-actions">
            <button type="button" class="ui-btn sm mono" @click="copyText(selectedEntry.message)">
              <Copy :size="12" /> {{ tr("复制内容") }}
            </button>
            <button type="button" class="ui-btn sm mono" @click="copyText(selectedEntry.rawText)">
              <Copy :size="12" /> {{ tr("复制原始行") }}
            </button>
          </div>
        </div>
        <pre class="inspector-pre mono">{{ selectedEntry.rawText }}</pre>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import {
  ChevronLeft,
  ChevronRight,
  Clock,
  Code2,
  Copy,
  FileText,
  Filter,
  RotateCcw,
  Search,
  X,
} from 'lucide-vue-next';
import { tr } from '../../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../../composables/useToast';
import {
  parseStructuredLog,
  type LogLevelKind,
  type ParsedLogEntry,
} from '../../../../lib/forensics/logParser';

const props = defineProps<{
  text: string;
  filename: string;
  query?: string;
}>();

const emit = defineEmits<{
  (e: 'switch-code'): void;
}>();

const keywordInput = ref(props.query ?? '');
const excludeInput = ref('');
const selectedLevel = ref<LogLevelKind | ''>('');
const lineFilter = ref('');
const timeFilter = ref('');
const sourceFilter = ref('');
const messageFilter = ref('');
const pageIndex = ref(0);
const pageSize = ref(200);
const selectedEntry = ref<ParsedLogEntry | null>(null);

watch(
  () => props.query,
  q => {
    if (q !== undefined && q !== keywordInput.value) {
      keywordInput.value = q;
    }
  },
);

const parsed = computed(() => parseStructuredLog(props.text));

const levelItems = computed(() => {
  const c = parsed.value.levelCounts;
  return [
    { key: 'ERROR' as LogLevelKind, label: tr('错误'), count: c.ERROR, cls: 'lvl-error' },
    { key: 'WARN' as LogLevelKind, label: tr('警告'), count: c.WARN, cls: 'lvl-warn' },
    { key: 'INFO' as LogLevelKind, label: tr('信息'), count: c.INFO, cls: 'lvl-info' },
    { key: 'DEBUG' as LogLevelKind, label: tr('调试'), count: c.DEBUG, cls: 'lvl-debug' },
    { key: 'OTHER' as LogLevelKind, label: tr('其他'), count: c.OTHER, cls: 'lvl-other' },
  ].filter(item => item.count > 0);
});

const hasActiveFilter = computed(() =>
  Boolean(
    keywordInput.value.trim() ||
      excludeInput.value.trim() ||
      selectedLevel.value ||
      lineFilter.value.trim() ||
      timeFilter.value.trim() ||
      sourceFilter.value.trim() ||
      messageFilter.value.trim(),
  ),
);

function parseRegexOrSubstring(raw: string): ((target: string) => boolean) | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('/') && trimmed.lastIndexOf('/') > 1) {
    const lastSlash = trimmed.lastIndexOf('/');
    const pattern = trimmed.slice(1, lastSlash);
    const flags = trimmed.slice(lastSlash + 1) || 'i';
    try {
      const re = new RegExp(pattern, flags);
      return (target: string) => re.test(target);
    } catch {
      // Fallback to substring
    }
  }
  const needle = trimmed.toLowerCase();
  return (target: string) => target.toLowerCase().includes(needle);
}

const filteredEntries = computed(() => {
  const list = parsed.value.entries;
  const kwMatch = parseRegexOrSubstring(keywordInput.value);
  const excludes = excludeInput.value
    .split(/[,，]/)
    .map(s => s.trim().toLowerCase())
    .filter(Boolean);
  const lvl = selectedLevel.value;
  const tm = timeFilter.value.trim().toLowerCase();
  const src = sourceFilter.value.trim().toLowerCase();
  const msg = messageFilter.value.trim().toLowerCase();
  const lf = lineFilter.value.trim();

  let lineMin: number | null = null;
  let lineMax: number | null = null;
  if (lf) {
    const rangeMatch = lf.match(/^(\d+)\s*-\s*(\d+)$/);
    if (rangeMatch) {
      lineMin = Number(rangeMatch[1]);
      lineMax = Number(rangeMatch[2]);
    } else if (/^\d+$/.test(lf)) {
      lineMin = Number(lf);
      lineMax = Number(lf);
    }
  }

  return list.filter(item => {
    if (lvl && item.level !== lvl) return false;
    if (lineMin !== null && lineMax !== null) {
      if (item.lineEnd < lineMin || item.lineStart > lineMax) return false;
    }
    if (tm && !item.timestamp.toLowerCase().includes(tm)) return false;
    if (src && !item.source.toLowerCase().includes(src)) return false;
    if (msg && !item.message.toLowerCase().includes(msg)) return false;
    if (kwMatch && !kwMatch(`${item.timestamp} ${item.source} ${item.message}`)) return false;
    if (excludes.length > 0) {
      const hay = `${item.source} ${item.message}`.toLowerCase();
      if (excludes.some(ex => hay.includes(ex))) return false;
    }
    return true;
  });
});

watch(
  [keywordInput, excludeInput, selectedLevel, lineFilter, timeFilter, sourceFilter, messageFilter],
  () => {
    pageIndex.value = 0;
  },
);

const pageCount = computed(() =>
  Math.max(1, Math.ceil(filteredEntries.value.length / pageSize.value)),
);

const pagedEntries = computed(() => {
  const start = pageIndex.value * pageSize.value;
  return filteredEntries.value.slice(start, start + pageSize.value);
});

const rangeStart = computed(() =>
  filteredEntries.value.length === 0 ? 0 : pageIndex.value * pageSize.value + 1,
);

const rangeEnd = computed(() =>
  Math.min(filteredEntries.value.length, (pageIndex.value + 1) * pageSize.value),
);

watch(
  pagedEntries,
  rows => {
    if (rows.length > 0 && (!selectedEntry.value || !rows.some(r => r.index === selectedEntry.value?.index))) {
      selectedEntry.value = rows[0];
    }
  },
  { immediate: true },
);

function toggleSourceChip(src: string) {
  sourceFilter.value = sourceFilter.value.toLowerCase() === src.toLowerCase() ? '' : src;
}

function resetAllFilters() {
  keywordInput.value = '';
  excludeInput.value = '';
  selectedLevel.value = '';
  lineFilter.value = '';
  timeFilter.value = '';
  sourceFilter.value = '';
  messageFilter.value = '';
  pageIndex.value = 0;
}

function firstLine(text: string): string {
  const idx = text.indexOf('\n');
  return idx >= 0 ? text.slice(0, idx) : text;
}

function levelClass(level: LogLevelKind): string {
  switch (level) {
    case 'ERROR':
      return 'lvl-error';
    case 'WARN':
      return 'lvl-warn';
    case 'INFO':
      return 'lvl-info';
    case 'DEBUG':
      return 'lvl-debug';
    default:
      return 'lvl-other';
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    notifySuccess(tr("已复制到剪贴板"));
  } catch (err) {
    notifyError(tr("复制失败"), String(err));
  }
}
</script>

<style scoped>
.structured-log-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  overflow: hidden;
}

.mono {
  font-family: var(--font-mono);
}

.log-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px;
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.header-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}

.log-icon {
  color: var(--primary-color);
}

.file-name {
  font-weight: 600;
  font-size: 13px;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.time-range {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
  font-size: 11px;
}

.stats-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.level-pills,
.source-chips {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-wrap: wrap;
}

.lvl-pill,
.src-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid var(--border-color);
  background: var(--surface-primary);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
}

.lvl-pill.active,
.src-chip.active {
  border-color: var(--primary-color);
  background: var(--sidebar-active-bg);
  color: var(--primary-color);
  font-weight: 600;
}

.pill-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--text-secondary);
}
.pill-dot.lvl-error { background: var(--sev-critical-fg); }
.pill-dot.lvl-warn { background: var(--sev-warn-fg); }
.pill-dot.lvl-info { background: var(--sev-info-fg); }
.pill-dot.lvl-debug { background: var(--chart-4); }

.chips-label {
  font-size: 11px;
  color: var(--text-secondary);
}

.chip-cnt,
.pill-cnt {
  opacity: 0.75;
  font-size: 10px;
}

.filter-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.search-box {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  height: 28px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--surface-primary);
}

.exclude-box {
  max-width: 220px;
}

.search-input {
  flex: 1;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}

.clear-btn {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 2px;
}

.log-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.table-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.log-table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
}

.log-table th {
  position: sticky;
  top: 0;
  z-index: 2;
  background: var(--surface-secondary);
  text-align: left;
  padding: 6px 8px;
  font-size: 11px;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border-color);
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
  padding: 0 6px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 11px;
  outline: none;
}

.col-filter-input:focus,
.col-filter-select:focus {
  border-color: var(--primary-color);
}

.col-line { width: 68px; }
.col-time { width: 165px; }
.col-level { width: 74px; }
.col-source { width: 140px; }

.log-row {
  border-bottom: 1px solid color-mix(in srgb, var(--border-color) 55%, transparent);
  cursor: pointer;
}

.log-row:hover {
  background: var(--hover-bg);
}

.log-row.active {
  background: var(--active-bg);
}

.log-row td {
  padding: 5px 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.td-msg {
  display: flex;
  align-items: center;
  gap: 6px;
}

.cell-ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.multiline-pill {
  flex-shrink: 0;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--surface-secondary);
  color: var(--primary-color);
  font-size: 10px;
}

.level-tag {
  display: inline-block;
  padding: 1px 6px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 600;
}
.level-tag.lvl-error { background: var(--sev-critical-bg); color: var(--sev-critical-fg); }
.level-tag.lvl-warn { background: var(--sev-warn-bg); color: var(--sev-warn-fg); }
.level-tag.lvl-info { background: var(--sev-info-bg); color: var(--sev-info-fg); }
.level-tag.lvl-debug { background: color-mix(in srgb, var(--chart-4) 14%, transparent); color: var(--chart-4); }
.level-tag.lvl-other { background: var(--surface-tertiary); color: var(--text-secondary); }

.empty-td {
  text-align: center;
  padding: 28px;
  color: var(--text-secondary);
}

.table-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  border-top: 1px solid var(--border-color);
  background: var(--surface-secondary);
  flex-shrink: 0;
}

.pager-controls {
  display: flex;
  align-items: center;
  gap: 6px;
}

.page-size-select {
  height: 24px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--surface-primary);
  color: var(--text-primary);
  padding: 0 6px;
}

.log-inspector {
  flex-shrink: 0;
  max-height: 165px;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--border-color);
  background: var(--surface-secondary);
}

.inspector-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 5px 12px;
  border-bottom: 1px solid var(--border-color);
  gap: 8px;
}

.inspector-meta,
.inspector-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.src-badge {
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--surface-primary);
  border: 1px solid var(--border-color);
}

.inspector-pre {
  margin: 0;
  padding: 8px 12px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 11.5px;
  line-height: 1.45;
}
</style>
