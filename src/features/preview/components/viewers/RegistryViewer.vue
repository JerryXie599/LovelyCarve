<template>
  <div class="registry-viewer">
    <div v-if="loading" class="state-row mono">
      <Loader2 :size="14" class="spin" />
      {{ tr("正在解析注册表配置单元…") }}
    </div>
    <div v-else-if="error" class="state-row mono error">
      {{ tr("注册表解析失败：{0}", { 0: error }) }}
    </div>

    <template v-else-if="digest">
      <header class="reg-header">
        <div class="header-top">
          <div class="title-wrap">
            <FolderTree :size="15" class="reg-icon" />
            <span class="hive-name mono">{{ digest.hiveName }}</span>
            <span class="ui-badge sm mono">regf v{{ digest.version }}</span>
            <span class="seq-pill mono" :class="{ dirty: digest.isDirty }">
              seq {{ digest.primarySeq }}/{{ digest.secondarySeq }}
            </span>
            <span v-if="digest.isDirty && digest.sidecars.length === 0" class="dirty-pill mono">
              {{ tr("序列号不一致") }}
            </span>
            <span v-if="digest.deletedScanEnabled" class="meta-pill mono">
              {{ tr("含已删除项扫描") }}
            </span>
          </div>

          <div class="header-meta mono">
            <span v-if="digest.embeddedPath" class="embedded-path" :title="digest.embeddedPath">
              {{ digest.embeddedPath }}
            </span>
            <span v-if="digest.lastWritten" class="time-pill">
              <Clock :size="12" />
              {{ formatIso(digest.lastWritten) }}
            </span>
          </div>
        </div>

        <div v-if="digest.sidecars.length" class="sidecar-banner mono">
          <History :size="12" />
          <span>{{ tr("已自动重放事务日志：{0}", { 0: digest.sidecars.join(", ") }) }}</span>
        </div>

        <div v-if="digest.notes.length" class="notes-banner mono">
          <AlertTriangle :size="12" class="warn-icon" />
          <div class="notes-text">
            <div v-for="(note, idx) in digest.notes" :key="idx">{{ note }}</div>
          </div>
        </div>

        <div class="toolbar-row">
          <div class="breadcrumb mono">
            <button class="crumb-btn" :class="{ active: activePath === '' }" @click="selectKey('', null)">
              {{ digest.rootName || "ROOT" }}
            </button>
            <template v-for="(seg, idx) in pathSegments" :key="idx">
              <span class="crumb-sep">\</span>
              <button
                class="crumb-btn"
                :class="{ active: idx === pathSegments.length - 1 }"
                @click="selectKey(pathSegments.slice(0, idx + 1).join('\\'), null)"
              >
                {{ seg }}
              </button>
            </template>
          </div>

          <form class="search-form" @submit.prevent="runSearch">
            <div class="search-box">
              <Search :size="13" class="search-icon" />
              <input
                v-model="searchInput"
                class="search-input mono"
                :placeholder="tr('搜索注册表键名、键值名或数据（回车检索）…')"
              />
              <button v-if="searchInput" type="button" class="icon-clear" @click="clearSearch">
                <X :size="12" />
              </button>
            </div>
            <button type="submit" class="ui-btn sm mono" :disabled="searching || !searchInput.trim()">
              <Loader2 v-if="searching" :size="12" class="spin" />
              <span v-else>{{ tr("检索") }}</span>
            </button>
            <button
              v-if="searchResult"
              type="button"
              class="ui-btn sm mono"
              @click="clearSearch"
            >
              {{ tr("返回键值") }}
            </button>
          </form>
        </div>
      </header>

      <div class="reg-body">
        <aside class="reg-tree-pane">
          <div class="pane-head mono">
            <span>{{ tr("注册表树") }}</span>
            <span class="pane-count">{{ digest.rootSubkeyCount }}</span>
          </div>
          <div class="tree-filter-bar">
            <input
              v-model="treeFilterInput"
              class="col-filter-input mono"
              :placeholder="tr('筛选左侧子键名称…')"
            />
          </div>
          <div class="tree-scroll">
            <div
              v-for="item in filteredTreeRows"
              :key="item.nodeKey"
              class="tree-row"
              :class="{ active: isRowSelected(item), deleted: item.isDeleted }"
              :style="{ paddingLeft: `${8 + item.depth * 14}px` }"
              @click="selectTreeItem(item)"
            >
              <button
                type="button"
                class="twist-btn"
                :class="{ hidden: !item.hasSubkeys }"
                @click.stop="toggleExpand(item)"
              >
                <Loader2 v-if="item.expanding" :size="12" class="spin" />
                <ChevronDown v-else-if="item.expanded" :size="12" />
                <ChevronRight v-else :size="12" />
              </button>
              <FolderOpen v-if="item.expanded" :size="13" class="tree-folder" />
              <Folder v-else :size="13" class="tree-folder" />
              <span class="tree-label mono" :title="item.name">{{ item.name }}</span>
              <span v-if="item.isDeleted" class="del-tag">{{ tr("已删除") }}</span>
              <span v-else-if="item.subkeyCount > 0" class="sub-cnt mono">{{ item.subkeyCount }}</span>
            </div>
          </div>
        </aside>

        <section class="reg-detail-pane">
          <template v-if="searchResult">
            <div class="pane-head mono">
              <span>
                {{ tr("检索结果：{0} 条命中", { 0: String(searchResult.hits.length) }) }}
                <template v-if="searchResult.truncated"> ({{ tr("已达上限截断") }})</template>
              </span>
            </div>
            <div v-if="searchResult.hits.length === 0" class="state-row mono">
              {{ tr("未找到匹配的键或键值。") }}
            </div>
            <div v-else class="table-scroll">
              <table class="data-table reg-table">
                <thead>
                  <tr>
                    <th class="col-kind">{{ tr("目标") }}</th>
                    <th class="col-path">{{ tr("注册表键路径") }}</th>
                    <th class="col-vname">{{ tr("值名称") }}</th>
                    <th class="col-type">{{ tr("类型") }}</th>
                    <th>{{ tr("数据摘要") }}</th>
                  </tr>
                  <tr class="col-filter-row">
                    <th>
                      <select v-model="hitKindFilter" class="col-filter-select mono">
                        <option value="">{{ tr("全部") }}</option>
                        <option value="key">{{ tr("键") }}</option>
                        <option value="value">{{ tr("值") }}</option>
                      </select>
                    </th>
                    <th>
                      <input v-model="hitPathFilter" class="col-filter-input mono" :placeholder="tr('筛选路径…')" />
                    </th>
                    <th>
                      <input v-model="hitNameFilter" class="col-filter-input mono" :placeholder="tr('筛选值名称…')" />
                    </th>
                    <th>
                      <input v-model="hitTypeFilter" class="col-filter-input mono" :placeholder="tr('筛选类型…')" />
                    </th>
                    <th>
                      <input v-model="hitPreviewFilter" class="col-filter-input mono" :placeholder="tr('筛选数据摘要…')" />
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="(hit, idx) in filteredSearchHits"
                    :key="idx"
                    class="reg-row"
                    :class="{ deleted: hit.isDeleted }"
                    @click="openSearchHit(hit)"
                  >
                    <td class="mono">
                      <span class="type-pill">{{ hit.kind === "key" ? tr("键") : tr("值") }}</span>
                      <span v-if="hit.isDeleted" class="del-tag">{{ tr("已删除") }}</span>
                    </td>
                    <td class="mono path-cell" :title="hit.keyPath">{{ hit.keyPath || digest.rootName }}</td>
                    <td class="mono">{{ hit.valueName ?? "—" }}</td>
                    <td class="mono type-col">{{ hit.dataType ?? "KEY" }}</td>
                    <td class="mono val-cell" :title="hit.preview">{{ hit.preview }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </template>

          <template v-else>
            <div class="key-meta-bar mono" v-if="activeDetail">
              <span class="meta-entry">
                {{ tr("最后写入：") }}
                <strong>{{ activeDetail.lastWritten ? formatIso(activeDetail.lastWritten) : "—" }}</strong>
              </span>
              <span class="meta-entry">
                {{ tr("子键：") }}<strong>{{ activeDetail.subkeys.length }}</strong>
              </span>
              <span class="meta-entry">
                {{ tr("键值：") }}<strong>{{ activeDetail.values.length }}</strong>
              </span>
              <span class="meta-entry">Cell: 0x{{ activeDetail.cellOffset.toString(16) }}</span>
              <span v-if="activeDetail.isDeleted" class="del-tag">{{ tr("已删除") }}</span>
            </div>

            <div v-if="activeDetail?.notes.length" class="notes-banner mono">
              <AlertTriangle :size="12" class="warn-icon" />
              <div class="notes-text">
                <div v-for="(n, idx) in activeDetail.notes" :key="idx">{{ n }}</div>
              </div>
            </div>

            <div v-if="keyLoading && !activeDetail" class="state-row mono">
              <Loader2 :size="14" class="spin" />
              {{ tr("正在加载键值…") }}
            </div>
            <div v-else-if="keyError" class="state-row mono error">
              {{ keyError }}
            </div>
            <template v-else-if="activeDetail">
              <div class="values-area">
                <div v-if="activeDetail.values.length === 0" class="empty-values mono">
                  <div>{{ tr("该键下暂无键值数据。") }}</div>
                  <div v-if="activeDetail.subkeys.length > 0" class="quick-subkeys">
                    <span class="quick-label">{{ tr("下级子键：") }}</span>
                    <button
                      v-for="sk in activeDetail.subkeys.slice(0, 12)"
                      :key="sk.cellOffset"
                      class="subkey-chip mono"
                      :class="{ deleted: sk.isDeleted }"
                      @click="navigateToChild(sk)"
                    >
                      {{ sk.name }}
                    </button>
                  </div>
                </div>

                <div v-else class="table-scroll">
                  <table class="data-table reg-table">
                    <thead>
                      <tr>
                        <th class="col-name">{{ tr("名称") }}</th>
                        <th class="col-type">{{ tr("类型") }}</th>
                        <th class="col-data">{{ tr("数据") }}</th>
                        <th class="col-size">{{ tr("大小") }}</th>
                      </tr>
                      <tr class="col-filter-row">
                        <th>
                          <input
                            v-model="valueNameFilter"
                            class="col-filter-input mono"
                            :placeholder="tr('筛选名称…')"
                          />
                        </th>
                        <th>
                          <select v-model="valueTypeFilter" class="col-filter-select mono">
                            <option value="">{{ tr("全部") }}</option>
                            <option v-for="tp in availableValueTypes" :key="tp" :value="tp">{{ tp }}</option>
                          </select>
                        </th>
                        <th>
                          <input
                            v-model="valueDataFilter"
                            class="col-filter-input mono"
                            :placeholder="tr('筛选数据 / UTF-16…')"
                          />
                        </th>
                        <th>
                          <label class="del-filter-chk mono" :title="tr('仅显示已删除键值')">
                            <input v-model="onlyDeletedValues" type="checkbox" />
                            <span>{{ tr("已删") }}</span>
                          </label>
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr
                        v-for="(val, idx) in filteredValues"
                        :key="`${val.name}:${idx}`"
                        class="reg-row"
                        :class="{ active: selectedValue === val, deleted: val.isDeleted }"
                        @click="selectedValue = val"
                      >
                        <td class="mono name-cell" :title="val.name">
                          <span>{{ val.name }}</span>
                          <span v-if="val.isDeleted" class="del-tag">{{ tr("已删除") }}</span>
                        </td>
                        <td class="mono type-col">
                          <span class="type-pill">{{ val.dataType }}</span>
                        </td>
                        <td class="mono val-cell" :title="val.value">{{ val.value }}</td>
                        <td class="mono size-col">{{ val.rawSize }} B</td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </div>

              <div v-if="selectedValue" class="reg-inspector" :class="{ 'ts-open': timestampOpen }">
                <div class="inspector-head">
                  <div class="inspector-title mono">
                    <span>{{ selectedValue.name }}</span>
                    <span class="type-pill">{{ selectedValue.dataType }}</span>
                    <span class="meta-pill">rawSize={{ selectedValue.rawSize }} B</span>
                    <span class="meta-pill" title="VK DataSize">
                      dataLen=0x{{ selectedValue.dataSizeRaw.toString(16).padStart(8, "0") }}
                    </span>
                    <button
                      v-if="bestTimestampCandidate && !timestampOpen"
                      type="button"
                      class="ts-quick-pill mono"
                      :title="tr('点击展开更多时间戳格式')"
                      @click="timestampOpen = true"
                    >
                      <Clock :size="11" />
                      <span>{{ bestTimestampCandidate.tzFormatted }}</span>
                    </button>
                  </div>
                  <div class="inspector-actions">
                    <button
                      class="ui-btn sm mono"
                      :class="{ 'ts-btn-active': timestampOpen }"
                      :title="tr('将当前键值或原始字节转换为常见取证时间戳')"
                      @click="timestampOpen = !timestampOpen"
                    >
                      <Clock :size="12" />
                      {{ tr("转时间戳") }}
                      <span v-if="bestTimestampCandidate" class="ts-dot" />
                    </button>
                    <button class="ui-btn sm mono" @click="copyText(selectedValue.value)">
                      <Copy :size="12" /> {{ tr("复制值") }}
                    </button>
                    <button v-if="selectedValue.rawHex" class="ui-btn sm mono" @click="copyText(selectedValue.rawHex)">
                      <Copy :size="12" /> {{ tr("复制十六进制") }}
                    </button>
                  </div>
                </div>

                <div v-if="selectedValue.utf16Hint" class="utf16-hint mono">
                  UTF-16LE: "{{ selectedValue.utf16Hint }}"
                </div>

                <div v-if="timestampOpen" class="ts-drawer mono">
                  <div class="ts-toolbar">
                    <div class="ts-controls">
                      <span class="ts-label">{{ tr("字节序") }}:</span>
                      <button
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsEndian === 'le' }"
                        @click="tsEndian = 'le'"
                      >
                        LE
                      </button>
                      <button
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsEndian === 'be' }"
                        @click="tsEndian = 'be'"
                      >
                        BE
                      </button>

                      <template v-if="rawByteLength > 4">
                        <span class="ts-sep">|</span>
                        <span class="ts-label">{{ tr("起始偏移") }}:</span>
                        <button
                          type="button"
                          class="ts-chip"
                          :disabled="tsByteOffset <= 0"
                          @click="tsByteOffset = Math.max(0, tsByteOffset - 1)"
                        >
                          -1B
                        </button>
                        <span class="ts-offset-val">+{{ tsByteOffset }} B</span>
                        <button
                          type="button"
                          class="ts-chip"
                          :disabled="tsByteOffset + 4 >= rawByteLength"
                          @click="tsByteOffset = Math.min(rawByteLength - 4, tsByteOffset + 1)"
                        >
                          +1B
                        </button>
                        <button
                          v-if="tsByteOffset !== 0"
                          type="button"
                          class="ts-chip"
                          @click="tsByteOffset = 0"
                        >
                          {{ tr("归零") }}
                        </button>
                      </template>

                      <span class="ts-sep">|</span>
                      <span class="ts-label">{{ tr("时区") }}:</span>
                      <button
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsTzOffset === 480 }"
                        @click="tsTzOffset = 480"
                      >
                        UTC+08
                      </button>
                      <button
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsTzOffset === 0 }"
                        @click="tsTzOffset = 0"
                      >
                        UTC
                      </button>
                      <button
                        v-if="localTzOffsetMinutes !== 480 && localTzOffsetMinutes !== 0"
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsTzOffset === localTzOffsetMinutes }"
                        @click="tsTzOffset = localTzOffsetMinutes"
                      >
                        {{ tr("本地") }} ({{ localTzLabel }})
                      </button>

                      <button
                        type="button"
                        class="ts-chip"
                        :class="{ active: tsIncludeAll }"
                        @click="tsIncludeAll = !tsIncludeAll"
                      >
                        {{ tsIncludeAll ? tr("全部区间") : tr("常见年份 (1995-2045)") }}
                      </button>
                    </div>
                    <button
                      type="button"
                      class="icon-clear"
                      :title="tr('收起时间戳面板')"
                      @click="timestampOpen = false"
                    >
                      <X :size="12" />
                    </button>
                  </div>

                  <div v-if="displayedTimestampCandidates.length === 0" class="ts-empty">
                    {{ tr("未在当前字节或数值中识别出有效时间戳（可尝试切换字节序、偏移或展开全部区间）。") }}
                  </div>
                  <div v-else class="ts-list">
                    <div
                      v-for="cand in displayedTimestampCandidates"
                      :key="cand.id"
                      class="ts-row"
                      :class="{ plausible: cand.plausible }"
                    >
                      <span v-if="cand.plausible" class="ts-badge plausible">{{ tr("推荐") }}</span>
                      <span v-else class="ts-badge">{{ tr("候选") }}</span>
                      <span class="ts-fmt">{{ cand.format }}</span>
                      <span v-if="cand.endian !== 'n/a'" class="ts-endian">{{ cand.endian.toUpperCase() }}</span>
                      <span class="ts-time-primary" :title="cand.iso">{{ cand.tzFormatted }}</span>
                      <span v-if="cand.tzOffsetApplied" class="ts-time-utc">({{ cand.utc }})</span>
                      <button
                        type="button"
                        class="ts-copy-btn"
                        :title="tr('复制转换结果')"
                        @click="copyText(cand.tzFormatted)"
                      >
                        <Copy :size="11" />
                        {{ tr("复制") }}
                      </button>
                    </div>
                  </div>
                </div>

                <div class="inspector-panes">
                  <pre class="inspector-text mono">{{ selectedValue.value }}</pre>
                  <pre v-if="selectedValue.rawHex" class="inspector-hex mono">{{ selectedValue.rawHex }}<template v-if="selectedValue.hexTruncated">
… ({{ tr("仅显示前 4096 字节") }})</template></pre>
                </div>
              </div>
            </template>
          </template>
        </section>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { tr } from "../../../../composables/useLocale";
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Clock,
  Copy,
  Folder,
  FolderOpen,
  FolderTree,
  History,
  Loader2,
  Search,
  X,
} from "lucide-vue-next";
import type { ImageId, NodeId } from "../../../../lib/ipc/ids";
import {
  fsRegistryKey,
  fsRegistryPreview,
  fsRegistrySearch,
  type RegistryDigest,
  type RegistryKeyDetail,
  type RegistryKeySummary,
  type RegistrySearchHit,
  type RegistrySearchResult,
  type RegistryValueItem,
} from "../../../../lib/ipc/registry";
import {
  extractTimestampCandidates,
  formatTzLabel,
  parseHexDumpBytes,
  type ByteEndian,
} from "../../../../lib/forensics/timestampDecoder";
import { notifySuccess } from "../../../../composables/useToast";

const props = defineProps<{
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
}>();

interface TreeState {
  expandedPaths: Set<string>;
  childrenByPath: Record<string, RegistryKeySummary[]>;
  loadingPaths: Set<string>;
}

const loading = ref(false);
const error = ref<string | null>(null);
const digest = ref<RegistryDigest | null>(null);

const activePath = ref("");
const activeCellOffset = ref<number | null>(null);
const activeDetail = ref<RegistryKeyDetail | null>(null);
const keyLoading = ref(false);
const keyError = ref<string | null>(null);
const selectedValue = ref<RegistryValueItem | null>(null);

const timestampOpen = ref(false);
const tsEndian = ref<ByteEndian>("le");
const tsByteOffset = ref(0);
const tsTzOffset = ref(480);
const tsIncludeAll = ref(false);

const localTzOffsetMinutes = -new Date().getTimezoneOffset();
const localTzLabel = formatTzLabel(localTzOffsetMinutes);

watch(selectedValue, (val) => {
  tsByteOffset.value = 0;
  tsEndian.value = val?.dataType === "REG_DWORD_BIG_ENDIAN" ? "be" : "le";
});

const rawByteLength = computed(() => {
  if (!selectedValue.value?.rawHex) return 0;
  return parseHexDumpBytes(selectedValue.value.rawHex).length;
});

const timestampCandidates = computed(() => {
  const val = selectedValue.value;
  if (!val) return [];
  return extractTimestampCandidates({
    rawHex: val.rawHex,
    valueText: val.value,
    dataType: val.dataType,
    endian: tsEndian.value,
    byteOffset: tsByteOffset.value,
    tzOffsetMinutes: tsTzOffset.value,
    includeAllRanges: tsIncludeAll.value,
  });
});

const bestTimestampCandidate = computed(
  () => timestampCandidates.value.find((c) => c.plausible) ?? null,
);

const displayedTimestampCandidates = computed(() => {
  const list = timestampCandidates.value.map((c) => ({
    ...c,
    tzOffsetApplied: tsTzOffset.value !== 0,
  }));
  if (tsIncludeAll.value) return list;
  const plausible = list.filter((c) => c.plausible);
  return plausible.length > 0 ? plausible : list.slice(0, 8);
});

const searchInput = ref("");
const searching = ref(false);
const searchResult = ref<RegistrySearchResult | null>(null);
const treeFilterInput = ref("");
const valueNameFilter = ref("");
const valueTypeFilter = ref("");
const valueDataFilter = ref("");
const onlyDeletedValues = ref(false);
const hitKindFilter = ref("");
const hitPathFilter = ref("");
const hitNameFilter = ref("");
const hitTypeFilter = ref("");
const hitPreviewFilter = ref("");

const availableValueTypes = computed(() => {
  const vals = activeDetail.value?.values ?? [];
  return Array.from(new Set(vals.map(v => v.dataType))).sort();
});

const filteredValues = computed(() => {
  const vals = activeDetail.value?.values ?? [];
  const nf = valueNameFilter.value.trim().toLowerCase();
  const tf = valueTypeFilter.value.trim().toLowerCase();
  const df = valueDataFilter.value.trim().toLowerCase();
  const onlyDel = onlyDeletedValues.value;
  return vals.filter(v => {
    if (onlyDel && !v.isDeleted) return false;
    if (nf && !v.name.toLowerCase().includes(nf)) return false;
    if (tf && v.dataType.toLowerCase() !== tf) return false;
    if (df) {
      const hay = `${v.value} ${v.utf16Hint ?? ""} ${v.rawHex ?? ""}`.toLowerCase();
      if (!hay.includes(df)) return false;
    }
    return true;
  });
});

const filteredSearchHits = computed(() => {
  const hits = searchResult.value?.hits ?? [];
  const kf = hitKindFilter.value.trim().toLowerCase();
  const pf = hitPathFilter.value.trim().toLowerCase();
  const nf = hitNameFilter.value.trim().toLowerCase();
  const tf = hitTypeFilter.value.trim().toLowerCase();
  const prf = hitPreviewFilter.value.trim().toLowerCase();
  return hits.filter(h => {
    if (kf && h.kind !== kf) return false;
    if (pf && !(h.keyPath || "").toLowerCase().includes(pf)) return false;
    if (nf && !(h.valueName || "").toLowerCase().includes(nf)) return false;
    if (tf && !(h.dataType || "KEY").toLowerCase().includes(tf)) return false;
    if (prf && !(h.preview || "").toLowerCase().includes(prf)) return false;
    return true;
  });
});

const tree = reactive<TreeState>({
  expandedPaths: new Set([""]),
  childrenByPath: {},
  loadingPaths: new Set(),
});

let loadGen = 0;

interface VisibleTreeRow {
  nodeKey: string;
  name: string;
  path: string;
  cellOffset: number;
  depth: number;
  hasSubkeys: boolean;
  subkeyCount: number;
  expanded: boolean;
  expanding: boolean;
  isDeleted: boolean;
}

const pathSegments = computed(() =>
  activePath.value ? activePath.value.split("\\").filter(Boolean) : [],
);

const visibleTreeRows = computed<VisibleTreeRow[]>(() => {
  if (!digest.value) return [];
  const rows: VisibleTreeRow[] = [];
  const rootExpanded = tree.expandedPaths.has("");
  rows.push({
    nodeKey: "root",
    name: digest.value.rootName || "ROOT",
    path: "",
    cellOffset: 0,
    depth: 0,
    hasSubkeys: (tree.childrenByPath[""]?.length ?? digest.value.rootSubkeyCount) > 0,
    subkeyCount: tree.childrenByPath[""]?.length ?? digest.value.rootSubkeyCount,
    expanded: rootExpanded,
    expanding: tree.loadingPaths.has(""),
    isDeleted: false,
  });

  function pushChildren(parentKey: string, depth: number) {
    const children = tree.childrenByPath[parentKey] ?? [];
    for (const child of children) {
      const nodeKey = child.isDeleted ? `${child.path}@${child.cellOffset}` : child.path;
      const expanded = !child.isDeleted && tree.expandedPaths.has(child.path);
      rows.push({
        nodeKey,
        name: child.name,
        path: child.path,
        cellOffset: child.cellOffset,
        depth,
        hasSubkeys: !child.isDeleted && child.hasSubkeys,
        subkeyCount: child.subkeyCount,
        expanded,
        expanding: tree.loadingPaths.has(child.path),
        isDeleted: child.isDeleted,
      });
      if (expanded) {
        pushChildren(child.path, depth + 1);
      }
    }
  }

  if (rootExpanded) {
    pushChildren("", 1);
  }
  return rows;
});

const filteredTreeRows = computed<VisibleTreeRow[]>(() => {
  const q = treeFilterInput.value.trim().toLowerCase();
  if (!q) return visibleTreeRows.value;
  return visibleTreeRows.value.filter(
    row => row.nodeKey === "root" || row.name.toLowerCase().includes(q) || row.path.toLowerCase().includes(q),
  );
});

function isRowSelected(row: VisibleTreeRow): boolean {
  if (activePath.value !== row.path) return false;
  if (row.isDeleted && activeCellOffset.value !== null) {
    return activeCellOffset.value === row.cellOffset;
  }
  return true;
}

function formatIso(iso: string): string {
  return iso.replace("T", " ").replace("+00:00", " UTC").replace("Z", " UTC");
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    notifySuccess(tr("已复制到剪贴板"));
  } catch {
    // ignore clipboard errors
  }
}

async function loadDigest() {
  const gen = ++loadGen;
  loading.value = true;
  error.value = null;
  digest.value = null;
  activePath.value = "";
  activeCellOffset.value = null;
  activeDetail.value = null;
  selectedValue.value = null;
  searchInput.value = "";
  searchResult.value = null;
  tree.expandedPaths = new Set([""]);
  tree.childrenByPath = {};
  tree.loadingPaths = new Set();

  try {
    const res = await fsRegistryPreview({
      imageId: props.imageId,
      nodeId: props.nodeId,
      parentId: props.parentId,
    });
    if (gen !== loadGen) return;
    digest.value = res;
    tree.childrenByPath[""] = res.rootChildren;
    await selectKey("", null, gen);
  } catch (err) {
    if (gen !== loadGen) return;
    error.value = String(err);
  } finally {
    if (gen === loadGen) loading.value = false;
  }
}

async function selectKey(path: string, cellOffset: number | null, gen = loadGen, highlightValue?: string | null) {
  activePath.value = path;
  activeCellOffset.value = cellOffset;
  keyLoading.value = true;
  keyError.value = null;
  selectedValue.value = null;

  try {
    const detail = await fsRegistryKey({
      imageId: props.imageId,
      nodeId: props.nodeId,
      parentId: props.parentId,
      keyPath: path,
      cellOffset,
    });
    if (gen !== loadGen) return;
    activeDetail.value = detail;
    if (!detail.isDeleted && !tree.childrenByPath[path]) {
      tree.childrenByPath[path] = detail.subkeys;
    }
    if (highlightValue) {
      selectedValue.value =
        detail.values.find((v) => v.name.toLowerCase() === highlightValue.toLowerCase()) ??
        detail.values[0] ??
        null;
    } else {
      selectedValue.value = detail.values[0] ?? null;
    }
  } catch (err) {
    if (gen !== loadGen) return;
    keyError.value = String(err);
  } finally {
    if (gen === loadGen) keyLoading.value = false;
  }
}

async function toggleExpand(row: VisibleTreeRow) {
  if (!row.hasSubkeys) return;
  if (tree.expandedPaths.has(row.path)) {
    const next = new Set(tree.expandedPaths);
    next.delete(row.path);
    tree.expandedPaths = next;
    return;
  }
  if (!tree.childrenByPath[row.path]) {
    tree.loadingPaths.add(row.path);
    try {
      const detail = await fsRegistryKey({
        imageId: props.imageId,
        nodeId: props.nodeId,
        parentId: props.parentId,
        keyPath: row.path,
        cellOffset: row.isDeleted ? row.cellOffset : null,
      });
      tree.childrenByPath[row.path] = detail.subkeys;
    } catch {
      return;
    } finally {
      tree.loadingPaths.delete(row.path);
    }
  }
  const next = new Set(tree.expandedPaths);
  next.add(row.path);
  tree.expandedPaths = next;
}

async function selectTreeItem(row: VisibleTreeRow) {
  searchResult.value = null;
  await selectKey(row.path, row.isDeleted ? row.cellOffset : null);
}

async function navigateToChild(sk: RegistryKeySummary) {
  const next = new Set(tree.expandedPaths);
  next.add(activePath.value);
  tree.expandedPaths = next;
  await selectKey(sk.path, sk.isDeleted ? sk.cellOffset : null);
}

async function expandAncestorsAndSelect(targetPath: string, offset: number | null, valueName?: string | null) {
  const segments = targetPath ? targetPath.split("\\").filter(Boolean) : [];
  const nextExpanded = new Set(tree.expandedPaths);
  nextExpanded.add("");
  let current = "";
  for (let i = 0; i < segments.length - 1; i++) {
    current = current ? `${current}\\${segments[i]}` : segments[i];
    if (!tree.childrenByPath[current]) {
      try {
        const d = await fsRegistryKey({
          imageId: props.imageId,
          nodeId: props.nodeId,
          parentId: props.parentId,
          keyPath: current,
        });
        tree.childrenByPath[current] = d.subkeys;
      } catch {
        break;
      }
    }
    nextExpanded.add(current);
  }
  tree.expandedPaths = nextExpanded;
  searchResult.value = null;
  await selectKey(targetPath, offset, loadGen, valueName);
}

async function runSearch() {
  const q = searchInput.value.trim();
  if (!q) {
    searchResult.value = null;
    return;
  }
  const gen = loadGen;
  searching.value = true;
  try {
    const res = await fsRegistrySearch({
      imageId: props.imageId,
      nodeId: props.nodeId,
      parentId: props.parentId,
      query: q,
      limit: 300,
    });
    if (gen !== loadGen) return;
    searchResult.value = res;
  } finally {
    if (gen === loadGen) searching.value = false;
  }
}

function clearSearch() {
  searchInput.value = "";
  searchResult.value = null;
}

async function openSearchHit(hit: RegistrySearchHit) {
  await expandAncestorsAndSelect(
    hit.keyPath,
    hit.isDeleted ? hit.keyOffset : null,
    hit.valueName,
  );
}

watch(
  () => [props.imageId, props.nodeId],
  () => {
    void loadDigest();
  },
);

onMounted(() => {
  void loadDigest();
});
</script>

<style scoped>
.registry-viewer {
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
  padding: 28px;
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

.reg-header {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
}

.header-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.title-wrap {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-wrap: wrap;
}

.reg-icon {
  color: var(--primary-color);
}

.hive-name {
  font-weight: 700;
  font-size: 13px;
}

.seq-pill,
.meta-pill,
.time-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 6px;
  font-size: 11px;
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.seq-pill.dirty,
.dirty-pill {
  padding: 1px 6px;
  font-size: 11px;
  border-radius: var(--radius-sm);
  background: var(--sev-warn-bg);
  border: 1px solid var(--sev-warn-border);
  color: var(--sev-warn-fg);
}

.header-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: var(--text-secondary);
}

.embedded-path {
  max-width: 280px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  opacity: 0.85;
}

.sidecar-banner {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  font-size: 11px;
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
}

.notes-banner {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 4px 8px;
  font-size: 11px;
  border-radius: var(--radius-sm);
  background: var(--sev-warn-bg);
  border: 1px solid var(--sev-warn-border);
  color: var(--sev-warn-fg);
}

.warn-icon {
  flex-shrink: 0;
  margin-top: 2px;
}

.toolbar-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.breadcrumb {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 2px;
  font-size: 11px;
  min-width: 0;
  flex: 1;
}

.crumb-btn {
  border: 0;
  background: transparent;
  padding: 2px 5px;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  cursor: pointer;
}

.crumb-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.crumb-btn.active {
  color: var(--text-primary);
  font-weight: 600;
  background: var(--surface-tertiary);
}

.crumb-sep {
  opacity: 0.45;
}

.search-form {
  display: flex;
  align-items: center;
  gap: 6px;
}

.search-box {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 7px;
  color: var(--text-secondary);
  pointer-events: none;
}

.search-input {
  width: 240px;
  height: 26px;
  padding: 0 24px 0 24px;
  font-size: 11px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-primary);
}

.icon-clear {
  position: absolute;
  right: 4px;
  border: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: inline-flex;
  padding: 2px;
}

.reg-body {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

.reg-tree-pane {
  width: 280px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-color);
  background: var(--surface-secondary);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.pane-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 5px 10px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border-color);
}

.pane-count {
  opacity: 0.7;
}

.tree-scroll {
  flex: 1;
  overflow: auto;
  padding: 4px 0;
}

.tree-row {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding-right: 8px;
  cursor: pointer;
  user-select: none;
}

.tree-row:hover {
  background: var(--hover-bg);
}

.tree-row.active {
  background: var(--active-bg);
  font-weight: 600;
}

.tree-row.deleted .tree-label {
  opacity: 0.7;
  font-style: italic;
}

.twist-btn {
  width: 16px;
  height: 16px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  flex-shrink: 0;
}

.twist-btn.hidden {
  visibility: hidden;
}

.tree-folder {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.tree-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.sub-cnt {
  font-size: 10px;
  color: var(--text-secondary);
}

.del-tag {
  font-size: 10px;
  padding: 0 4px;
  border-radius: 3px;
  background: var(--sev-critical-bg);
  color: var(--sev-critical-fg);
  margin-left: 4px;
  flex-shrink: 0;
}

.reg-detail-pane {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.key-meta-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  padding: 5px 10px;
  font-size: 11px;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
}

.values-area {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.empty-values {
  padding: 20px;
  color: var(--text-secondary);
  font-size: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.quick-subkeys {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.quick-label {
  color: var(--text-secondary);
}

.subkey-chip {
  padding: 2px 8px;
  font-size: 11px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  background: var(--surface-secondary);
  color: var(--text-primary);
  cursor: pointer;
}

.subkey-chip:hover {
  border-color: var(--primary-color);
}

.table-scroll {
  flex: 1;
  overflow: auto;
}

.reg-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.reg-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  text-align: left;
  padding: 5px 8px;
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-color);
  font-weight: 600;
  color: var(--text-secondary);
}

.col-filter-row th {
  top: 26px;
  padding: 4px 6px;
  background: var(--surface-primary);
}

.tree-filter-bar {
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-color);
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

.del-filter-chk {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  cursor: pointer;
}

.reg-row {
  cursor: pointer;
  border-bottom: 1px solid var(--border-color);
}

.reg-row:hover {
  background: var(--hover-bg);
}

.reg-row.active {
  background: var(--active-bg);
}

.reg-row.deleted td {
  opacity: 0.78;
}

.reg-table td {
  padding: 5px 8px;
  vertical-align: top;
}

.col-name {
  width: 210px;
}

.col-type {
  width: 135px;
}

.col-size {
  width: 80px;
}

.name-cell,
.path-cell,
.val-cell {
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.type-pill {
  display: inline-block;
  padding: 0 5px;
  font-size: 10px;
  border-radius: 3px;
  background: var(--surface-tertiary);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.size-col {
  text-align: right;
  color: var(--text-secondary);
}

.reg-inspector {
  flex-shrink: 0;
  height: 155px;
  border-top: 1px solid var(--border-color);
  background: var(--surface-secondary);
  display: flex;
  flex-direction: column;
  padding: 6px 10px;
  gap: 6px;
  overflow: hidden;
}

.reg-inspector.ts-open {
  height: auto;
  min-height: 225px;
  max-height: 350px;
}

.ts-btn-active {
  border-color: var(--primary-color) !important;
  color: var(--primary-color) !important;
  background: var(--primary-soft) !important;
}

.ts-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sev-success-fg);
  display: inline-block;
  margin-left: 2px;
}

.ts-quick-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 7px;
  border-radius: 999px;
  border: 1px solid var(--sev-success-border);
  background: var(--sev-success-bg);
  color: var(--sev-success-fg);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
}

.ts-quick-pill:hover {
  background: var(--sev-success-bg-hover);
}

.ts-drawer {
  flex-shrink: 0;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 170px;
}

.ts-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ts-controls {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  font-size: 11px;
}

.ts-label {
  color: var(--text-secondary);
}

.ts-sep {
  color: var(--border-color);
  margin: 0 2px;
}

.ts-offset-val {
  color: var(--text-primary);
  padding: 0 4px;
}

.ts-chip {
  padding: 1px 6px;
  font-size: 10px;
  border-radius: 3px;
  border: 1px solid var(--border-color);
  background: var(--surface-secondary);
  color: var(--text-secondary);
  cursor: pointer;
}

.ts-chip:hover:not(:disabled) {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.ts-chip:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.ts-chip.active {
  border-color: var(--primary-color);
  background: var(--primary-soft);
  color: var(--primary-color);
  font-weight: 600;
}

.ts-empty {
  font-size: 11px;
  color: var(--text-secondary);
  padding: 4px 2px;
}

.ts-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
  overflow-y: auto;
}

.ts-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 6px;
  border-radius: var(--radius-sm);
  font-size: 11px;
  background: var(--surface-secondary);
}

.ts-row.plausible {
  background: var(--sev-success-bg);
  border: 1px solid var(--sev-success-border);
}

.ts-badge {
  padding: 0 4px;
  border-radius: 3px;
  font-size: 10px;
  background: var(--surface-tertiary);
  color: var(--text-secondary);
  flex-shrink: 0;
}

.ts-badge.plausible {
  background: var(--sev-success-tag-bg);
  color: var(--sev-success-tag-fg);
  font-weight: 600;
}

.ts-fmt {
  color: var(--text-secondary);
  min-width: 170px;
}

.ts-endian {
  padding: 0 4px;
  border-radius: 3px;
  border: 1px solid var(--border-color);
  font-size: 10px;
  color: var(--text-secondary);
}

.ts-time-primary {
  font-weight: 600;
  color: var(--text-primary);
}

.ts-time-utc {
  color: var(--text-secondary);
  font-size: 10px;
}

.ts-copy-btn {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 6px;
  border-radius: 3px;
  border: 1px solid var(--border-color);
  background: var(--surface-primary);
  color: var(--text-secondary);
  font-size: 10px;
  cursor: pointer;
}

.ts-copy-btn:hover {
  color: var(--text-primary);
  background: var(--hover-bg);
}

.inspector-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}

.inspector-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  min-width: 0;
  overflow: hidden;
}

.inspector-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.utf16-hint {
  flex-shrink: 0;
  padding: 3px 8px;
  border-radius: var(--radius-sm);
  background: var(--sev-success-bg);
  border: 1px solid var(--sev-success-border);
  font-size: 11px;
}

.inspector-panes {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 8px;
  overflow: hidden;
}

.inspector-text,
.inspector-hex {
  flex: 1;
  min-width: 0;
  margin: 0;
  padding: 6px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  font-size: 11px;
  line-height: 1.45;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
