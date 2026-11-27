<template>
  <section class="result-navigation" :aria-label="tr('分析结果目录')">
    <label class="ui-search directory-search">
      <Search :size="13" aria-hidden="true" />
      <input
        ref="searchInput"
        v-model="search"
        :aria-label="tr('搜索结果项')"
        :placeholder="tr('搜索结果项…')"
        @keydown.esc.stop.prevent="clearSearch"
      />
      <button v-if="search" class="ui-icon-btn sm" type="button" :title="tr('清除目录搜索')" :aria-label="tr('清除目录搜索')" @click="clearSearch">
        <X :size="12" aria-hidden="true" />
      </button>
    </label>

    <div ref="scroller" class="directory-scroll">
      <div v-if="treeLoading && !hasResults" class="directory-state" role="status"><Loader2 :size="16" class="spin" aria-hidden="true" />{{ tr("正在读取分析结果…") }}</div>
      <div v-else-if="treeError" class="directory-state" role="alert"><span>{{ tr("分析结果读取失败：{0}", { 0: treeError }) }}</span><button class="ui-btn link sm" @click="reload">{{ tr("重试") }}</button></div>
      <div v-else-if="!hasResults" class="directory-state" role="status">{{ search ? tr("未找到结果项") : tr("暂无分析结果") }}</div>
      <template v-else>
        <BcpResultDirectory
          v-for="directory in visibleBcp"
          :key="directory.run.nodeId"
          :directory="directory"
          :selected-id="selectedNode?.nodeId"
          :searching="!!search.trim()"
          :node-label="nodeLabel"
          @select="openResult"
        />

        <section v-for="section in sections" :key="section.id" class="result-section">
          <div class="dir-section-row" :class="{ selected: isCurrent(section.run) }">
            <button
              class="run-toggle"
              type="button"
              :aria-expanded="isExpanded(section.id, true)"
              :title="isExpanded(section.id, true) ? tr('收起') : tr('展开')"
              :aria-label="`${isExpanded(section.id, true) ? tr('收起') : tr('展开')} ${section.label}`"
              @click="toggleGroup(section.id, true)"
            >
              <ChevronRight :size="14" class="chevron" :class="{ open: isExpanded(section.id, true) }" aria-hidden="true" />
            </button>
            <button
              class="dir-section-open"
              type="button"
              :aria-current="isCurrent(section.run) ? 'true' : undefined"
              :title="section.title"
              @click="section.run ? openResult(section.run) : toggleGroup(section.id, true)"
            >
              <Folder :size="14" class="section-icon" aria-hidden="true" />
              <span class="dir-section-name">{{ section.label }}</span>
              <span v-if="section.singleRunTarget" class="dir-target">{{ section.singleRunTarget }}</span>
              <span class="result-count">{{ section.count.toLocaleString() }}</span>
              <span v-if="section.warningCount" class="ui-badge sm warn run-warnings" :title="tr('{0} 条告警', { 0: section.warningCount.toLocaleString() })">
                <TriangleAlert :size="11" aria-hidden="true" />{{ section.warningCount.toLocaleString() }}
              </span>
            </button>
          </div>

          <UiTransition>
            <div v-if="isExpanded(section.id, true)" class="dir-children">
              <template v-if="section.runs.length">
                <div v-for="(run, runIdx) in section.runs" :key="run.id" class="dir-run-block">
                  <div class="run-row" :class="{ selected: isCurrent(run.run) }">
                    <button
                      class="run-toggle"
                      type="button"
                      :aria-expanded="isExpanded(run.id, runIdx === 0)"
                      :title="isExpanded(run.id, runIdx === 0) ? tr('收起') : tr('展开')"
                      :aria-label="`${isExpanded(run.id, runIdx === 0) ? tr('收起') : tr('展开')} ${run.label}`"
                      @click="toggleGroup(run.id, runIdx === 0)"
                    >
                      <ChevronRight :size="13" class="chevron" :class="{ open: isExpanded(run.id, runIdx === 0) }" aria-hidden="true" />
                    </button>
                    <button
                      class="run-open"
                      type="button"
                      :aria-current="isCurrent(run.run) ? 'true' : undefined"
                      :title="run.title"
                      @click="run.run ? openResult(run.run) : toggleGroup(run.id, runIdx === 0)"
                    >
                      <img v-if="run.logo" :src="run.logo" class="dir-logo" alt="" aria-hidden="true" />
                      <Folder v-else :size="13" class="section-icon" aria-hidden="true" />
                      <span class="run-text">
                        <span class="run-name">{{ run.label }}</span>
                        <span v-if="run.target" class="run-target">{{ run.target }}</span>
                      </span>
                      <span v-if="run.legacy" class="ui-badge sm">{{ tr("旧版") }}</span>
                      <span class="result-count">{{ run.count.toLocaleString() }}</span>
                      <span v-if="run.warningCount" class="ui-badge sm warn run-warnings" :title="tr('{0} 条告警', { 0: run.warningCount.toLocaleString() })">
                        <TriangleAlert :size="11" aria-hidden="true" />{{ run.warningCount.toLocaleString() }}
                      </span>
                    </button>
                  </div>

                  <UiTransition>
                    <ul v-if="isExpanded(run.id, runIdx === 0)" class="result-list">
                      <template v-for="child in run.children" :key="child.id">
                        <li v-if="child.kind === 'app' || child.kind === 'group'" class="dir-app-item">
                          <div class="dir-app-row">
                            <button class="run-toggle sm" type="button" :aria-expanded="isExpanded(child.id, false)" :aria-label="`${isExpanded(child.id, false) ? tr('收起') : tr('展开')} ${child.label}`" @click="toggleGroup(child.id, false)">
                              <ChevronRight :size="12" class="chevron" :class="{ open: isExpanded(child.id, false) }" aria-hidden="true" />
                            </button>
                            <button class="dir-app-open" type="button" :title="child.title" @click="toggleGroup(child.id, false)">
                              <img v-if="child.kind === 'app' && child.logo" :src="child.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                              <AppWindow v-else-if="child.kind === 'app'" :size="13" class="result-icon" aria-hidden="true" />
                              <Folder v-else :size="13" class="result-icon" aria-hidden="true" />
                              <span class="result-name">{{ child.label }}</span>
                              <span class="result-count">{{ child.count.toLocaleString() }}</span>
                            </button>
                          </div>
                          <ul v-if="isExpanded(child.id, false)" class="dir-sub-list">
                            <li v-for="leaf in child.leaves" :key="leaf.id">
                              <button
                                class="result-item"
                                :class="{ selected: isCurrentLeaf(leaf.node), empty: leaf.count === 0 }"
                                :aria-current="isCurrentLeaf(leaf.node) ? 'true' : undefined"
                                :title="leaf.title"
                                @click="openResult(leaf.node)"
                              >
                                <img v-if="leaf.logo" :src="leaf.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                                <span class="result-name">{{ leaf.label }}</span>
                                <span class="result-count">{{ leaf.count.toLocaleString() }}</span>
                              </button>
                            </li>
                          </ul>
                        </li>

                        <li v-else class="dir-leaf-block">
                          <div class="dir-leaf-line">
                            <button
                              v-if="leafExpandable(child)"
                              class="run-toggle sm"
                              type="button"
                              :aria-expanded="isLeafExpanded(child)"
                              :aria-label="`${isLeafExpanded(child) ? tr('收起') : tr('展开')} ${child.label}`"
                              @click="toggleLeafFacets(child)"
                            >
                              <Loader2 v-if="facetLoading.has(facetKey(child))" :size="11" class="spin" aria-hidden="true" />
                              <ChevronRight v-else :size="12" class="chevron" :class="{ open: isLeafExpanded(child) }" aria-hidden="true" />
                            </button>
                            <button
                              class="result-item"
                              :class="{ selected: isCurrentLeaf(child.node), empty: child.count === 0, 'with-toggle': leafExpandable(child) }"
                              :aria-current="isCurrentLeaf(child.node) ? 'true' : undefined"
                              :title="child.title"
                              @click="openResult(child.node)"
                            >
                              <img v-if="child.logo" :src="child.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                              <component v-else :is="itemIcon(child.node)" :size="13" class="result-icon" aria-hidden="true" />
                              <span class="result-name">{{ child.label }}</span>
                              <span class="result-count">{{ child.count.toLocaleString() }}</span>
                            </button>
                          </div>
                          <ul v-if="isLeafExpanded(child) && facetsOf(child).length" class="dir-facet-list">
                            <li v-for="facet in facetsOf(child)" :key="facet.value">
                              <button
                                class="result-item facet"
                                :class="{ selected: isCurrentFacet(child, facet.value) }"
                                :aria-current="isCurrentFacet(child, facet.value) ? 'true' : undefined"
                                :title="facet.value"
                                @click="openResult(child.node, { [child.facetColumn!]: [facet.value] })"
                              >
                                <img v-if="facet.logo" :src="facet.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                                <AppWindow v-else :size="13" class="result-icon" aria-hidden="true" />
                                <span class="result-name">{{ facet.label }}</span>
                                <span class="result-count">{{ facet.count.toLocaleString() }}</span>
                              </button>
                            </li>
                          </ul>
                        </li>
                      </template>
                      <li v-if="!run.children.length" class="group-empty">{{ tr("没有记录") }}</li>
                    </ul>
                  </UiTransition>
                </div>
              </template>

              <ul v-else class="result-list">
                <template v-for="child in section.children" :key="child.id">
                  <li v-if="child.kind === 'app' || child.kind === 'group'" class="dir-app-item">
                    <div class="dir-app-row">
                      <button class="run-toggle sm" type="button" :aria-expanded="isExpanded(child.id, false)" :aria-label="`${isExpanded(child.id, false) ? tr('收起') : tr('展开')} ${child.label}`" @click="toggleGroup(child.id, false)">
                        <ChevronRight :size="12" class="chevron" :class="{ open: isExpanded(child.id, false) }" aria-hidden="true" />
                      </button>
                      <button class="dir-app-open" type="button" :title="child.title" @click="toggleGroup(child.id, false)">
                        <img v-if="child.kind === 'app' && child.logo" :src="child.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                        <AppWindow v-else-if="child.kind === 'app'" :size="13" class="result-icon" aria-hidden="true" />
                        <Folder v-else :size="13" class="result-icon" aria-hidden="true" />
                        <span class="result-name">{{ child.label }}</span>
                        <span class="result-count">{{ child.count.toLocaleString() }}</span>
                      </button>
                    </div>
                    <ul v-if="isExpanded(child.id, false)" class="dir-sub-list">
                      <li v-for="leaf in child.leaves" :key="leaf.id">
                        <button
                          class="result-item"
                          :class="{ selected: isCurrentLeaf(leaf.node), empty: leaf.count === 0 }"
                          :aria-current="isCurrentLeaf(leaf.node) ? 'true' : undefined"
                          :title="leaf.title"
                          @click="openResult(leaf.node)"
                        >
                          <img v-if="leaf.logo" :src="leaf.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                          <span class="result-name">{{ leaf.label }}</span>
                          <span class="result-count">{{ leaf.count.toLocaleString() }}</span>
                        </button>
                      </li>
                    </ul>
                  </li>

                  <li v-else class="dir-leaf-block">
                    <div class="dir-leaf-line">
                      <button
                        v-if="leafExpandable(child)"
                        class="run-toggle sm"
                        type="button"
                        :aria-expanded="isLeafExpanded(child)"
                        :aria-label="`${isLeafExpanded(child) ? tr('收起') : tr('展开')} ${child.label}`"
                        @click="toggleLeafFacets(child)"
                      >
                        <Loader2 v-if="facetLoading.has(facetKey(child))" :size="11" class="spin" aria-hidden="true" />
                        <ChevronRight v-else :size="12" class="chevron" :class="{ open: isLeafExpanded(child) }" aria-hidden="true" />
                      </button>
                      <button
                        class="result-item"
                        :class="{ selected: isCurrentLeaf(child.node), empty: child.count === 0, 'with-toggle': leafExpandable(child) }"
                        :aria-current="isCurrentLeaf(child.node) ? 'true' : undefined"
                        :title="child.title"
                        @click="openResult(child.node)"
                      >
                        <img v-if="child.logo" :src="child.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                        <component v-else :is="itemIcon(child.node)" :size="13" class="result-icon" aria-hidden="true" />
                        <span class="result-name">{{ child.label }}</span>
                        <span class="result-count">{{ child.count.toLocaleString() }}</span>
                      </button>
                    </div>
                    <ul v-if="isLeafExpanded(child) && facetsOf(child).length" class="dir-facet-list">
                      <li v-for="facet in facetsOf(child)" :key="facet.value">
                        <button
                          class="result-item facet"
                          :class="{ selected: isCurrentFacet(child, facet.value) }"
                          :aria-current="isCurrentFacet(child, facet.value) ? 'true' : undefined"
                          :title="facet.value"
                          @click="openResult(child.node, { [child.facetColumn!]: [facet.value] })"
                        >
                          <img v-if="facet.logo" :src="facet.logo" class="dir-leaf-logo" alt="" aria-hidden="true" />
                          <AppWindow v-else :size="13" class="result-icon" aria-hidden="true" />
                          <span class="result-name">{{ facet.label }}</span>
                          <span class="result-count">{{ facet.count.toLocaleString() }}</span>
                        </button>
                      </li>
                    </ul>
                  </li>
                </template>
                <li v-if="!section.children.length" class="group-empty">{{ tr("没有记录") }}</li>
              </ul>
            </div>
          </UiTransition>
        </section>
      </template>
    </div>
  </section>
</template>

<script setup lang="ts">
import { tr } from '../../../composables/useLocale';

import UiTransition from '../../../components/ui/UiTransition.vue';
import BcpResultDirectory from './BcpResultDirectory.vue';
import { computed, nextTick, reactive, ref, watch, type Component } from 'vue';
import {
  AppWindow, Bluetooth, Camera, ChevronRight, Compass, Contact, Database, FileText, Fingerprint, Folder,
  HardDrive, Image, KeyRound, Link2, Loader2, MapPin, MessageSquare, Network, Phone, Radio, Search,
  TriangleAlert, User, Wifi, X,
} from 'lucide-vue-next';
import { analysisRecordValues, type AnalysisResultNode } from '../../../lib/ipc/analysisResults';
import type { ImageId } from '../../../lib/ipc/ids';
import { resolveAppLogo } from '../../../utils/appIcons';
import { usePluginCatalog } from '../../tasks/api';
import { useAnalysisResults } from '../composables/useAnalysisResults';
import { useAnalysisLabels } from '../composables/useAnalysisLabels';
import { buildAnalysisGroups, toolAnalyzerFunctions } from '../model/analysisResultView';
import { buildBcpDirectories, searchBcpDirectory } from '../model/bcpDirectory';
import {
  buildResultDirectorySections,
  sectionContainsNode,
  type DirectoryLeafNode,
} from '../model/resultDirectoryTree';

interface SoftwareFacetItem {
  value: string;
  label: string;
  count: number;
  logo: string | null;
}

const props = defineProps<{ imageId: ImageId }>();
const emit = defineEmits<{ select: [] }>();
const { tree, treeLoading, treeError, selectedNode, filters, selectNode, reload, setImageId, ensureFresh } = useAnalysisResults();
const { nodeLabels, nodeLabel } = useAnalysisLabels();
const catalog = usePluginCatalog();
const excludedFunctions = computed(() => toolAnalyzerFunctions(catalog.packages?.value ?? []));

const search = ref('');
const searchInput = ref<HTMLInputElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const toggledGroups = ref(new Map<string, boolean>());
const expandedLeaves = ref(new Set<string>());
const facetCache = reactive(new Map<string, SoftwareFacetItem[]>());
const facetLoading = ref(new Set<string>());

function openResult(node: AnalysisResultNode, presetFilters?: Record<string, string[]>) {
  void selectNode(node, presetFilters);
  emit('select');
}

const bcp = computed(() => buildBcpDirectories(tree.value, props.imageId, tr));
const visibleBcp = computed(() => bcp.value.flatMap(directory => searchBcpDirectory(directory, search.value, nodeLabel) ?? []));
const groups = computed(() => {
  const groupedRuns = new Set(bcp.value.map(directory => directory.run.runId));
  return buildAnalysisGroups(tree.value, props.imageId, search.value, nodeLabels, excludedFunctions.value)
    .filter(group => !group.run || !groupedRuns.has(group.run.runId));
});
const sections = computed(() =>
  buildResultDirectorySections(groups.value, nodeLabel, catalog.packages?.value ?? [], tr),
);
const hasResults = computed(() => sections.value.length > 0 || visibleBcp.value.length > 0);

const isCurrent = (node: AnalysisResultNode | null) => !!node && selectedNode.value?.nodeId === node.nodeId;
function isCurrentLeaf(node: AnalysisResultNode): boolean {
  return isCurrent(node) && Object.keys(filters.value).length === 0;
}
function isCurrentFacet(leaf: DirectoryLeafNode, value: string): boolean {
  if (!isCurrent(leaf.node) || !leaf.facetColumn) return false;
  const active = filters.value[leaf.facetColumn];
  return Array.isArray(active) && active.length === 1 && active[0] === value;
}

function isExpanded(id: string, defaultOpen = true): boolean {
  if (search.value.trim()) return true;
  if (toggledGroups.value.has(id)) return toggledGroups.value.get(id)!;
  return defaultOpen;
}
function toggleGroup(id: string, defaultOpen = true) {
  const current = toggledGroups.value.has(id) ? toggledGroups.value.get(id)! : defaultOpen;
  toggledGroups.value.set(id, !current);
}

function facetKey(leaf: DirectoryLeafNode): string {
  return `${leaf.node.runId}::${leaf.node.artifactType}::${leaf.facetColumn ?? ''}`;
}
function leafExpandable(leaf: DirectoryLeafNode): boolean {
  return Boolean(leaf.facetColumn && leaf.count > 1);
}
function isLeafExpanded(leaf: DirectoryLeafNode): boolean {
  return expandedLeaves.value.has(leaf.id);
}
function facetsOf(leaf: DirectoryLeafNode): SoftwareFacetItem[] {
  return facetCache.get(facetKey(leaf)) ?? [];
}

async function toggleLeafFacets(leaf: DirectoryLeafNode): Promise<void> {
  if (!leaf.facetColumn) return;
  if (expandedLeaves.value.has(leaf.id)) {
    expandedLeaves.value.delete(leaf.id);
    return;
  }
  expandedLeaves.value.add(leaf.id);
  const key = facetKey(leaf);
  if (facetCache.has(key) || facetLoading.value.has(key)) return;
  facetLoading.value.add(key);
  try {
    const res = await analysisRecordValues({
      imageId: leaf.node.imageId,
      runId: leaf.node.runId,
      recordType: leaf.node.artifactType,
      legacy: leaf.node.legacy,
      column: leaf.facetColumn,
      limit: 50,
    });
    const items: SoftwareFacetItem[] = res.values
      .filter(entry => typeof entry.value === 'string' && entry.value.trim().length > 0 && entry.count > 0)
      .map(entry => {
        const value = String(entry.value).trim();
        return { value, label: value, count: entry.count, logo: resolveAppLogo(value) };
      });
    facetCache.set(key, items);
    if (items.length <= 1) expandedLeaves.value.delete(leaf.id);
  } catch {
    facetCache.set(key, []);
    expandedLeaves.value.delete(leaf.id);
  } finally {
    facetLoading.value.delete(key);
  }
}

function itemIcon(node: AnalysisResultNode): Component {
  const type = (node.typeName || '').toLowerCase();
  const label = (nodeLabel(node) || '').toLowerCase();
  if (type.includes('device') || label.includes('设备') || label.includes('硬件')) return HardDrive;
  if (type.includes('volume') || label.includes('卷信息') || label.includes('分区')) return Database;
  if (type.includes('connection') || label.includes('连接痕迹') || label.includes('挂载')) return Link2;
  if (type.includes('data_usage') || label.includes('流量') || label.includes('网络使用')) return Network;
  if (type.includes('airdrop') || label.includes('隔空投送')) return Radio;
  if (type.includes('photo') || label.includes('照片') || label.includes('相册') || label.includes('exif')) return Camera;
  if (type.includes('media') || label.includes('媒体') || label.includes('视频') || label.includes('音频')) return Image;
  if (type.includes('message') || type.includes('chat') || label.includes('聊天') || label.includes('短信') || label.includes('消息') || label.includes('对话')) return MessageSquare;
  if (type.includes('call') || label.includes('通话') || label.includes('电话')) return Phone;
  if (type.includes('contact') || label.includes('联系人') || label.includes('通讯录')) return Contact;
  if (type.includes('app') || label.includes('应用') || label.includes('app')) return AppWindow;
  if (type.includes('safari') || type.includes('web') || label.includes('浏览') || label.includes('历史记录') || label.includes('书签') || label.includes('标签页')) return Compass;
  if (type.includes('location') || label.includes('位置') || label.includes('定位') || label.includes('轨迹')) return MapPin;
  if (type.includes('keychain') || type.includes('password') || label.includes('密码') || label.includes('钥匙串') || label.includes('密钥')) return KeyRound;
  if (type.includes('account') || type.includes('user') || label.includes('账户') || label.includes('用户')) return User;
  if (type.includes('wifi') || label.includes('wifi') || label.includes('无线')) return Wifi;
  if (type.includes('bluetooth') || label.includes('蓝牙')) return Bluetooth;
  if (type.includes('fact') || type.includes('profile') || label.includes('画像')) return Fingerprint;
  return FileText;
}

function clearSearch() { search.value = ''; searchInput.value?.focus(); }

function revealSelection() {
  const container = scroller.value;
  const item = container?.querySelector<HTMLElement>('[aria-current="true"]');
  if (!container || !item) return;
  const top = item.offsetTop;
  const bottom = top + item.offsetHeight;
  if (top < container.scrollTop) container.scrollTop = Math.max(0, top - 36);
  else if (bottom > container.scrollTop + container.clientHeight) container.scrollTop = bottom - container.clientHeight + 8;
}

watch(() => selectedNode.value?.nodeId, async nodeId => {
  if (nodeId) {
    for (const section of sections.value) {
      const hit = sectionContainsNode(section, nodeId);
      if (!hit) continue;
      toggledGroups.value.set(section.id, true);
      if (hit.runId) toggledGroups.value.set(hit.runId, true);
      if (hit.groupId) toggledGroups.value.set(hit.groupId, true);
      break;
    }
  }
  await nextTick();
  revealSelection();
}, { immediate: true });

watch(() => props.imageId, imageId => {
  search.value = '';
  toggledGroups.value.clear();
  expandedLeaves.value.clear();
  facetCache.clear();
  setImageId(imageId);
  ensureFresh();
}, { immediate: true });
</script>

<style scoped>
.result-navigation { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; color: var(--text-primary); }
.directory-search { flex-shrink: 0; margin: 12px 12px 4px; padding-right: 4px; }
.directory-scroll { position: relative; flex: 1; min-height: 0; overflow-y: auto; padding: 4px 8px 16px; }
.result-section + .result-section { margin-top: 10px; }

.dir-section-row, .run-row, .dir-app-row, .dir-leaf-line {
  display: flex;
  align-items: center;
  min-width: 0;
  border-radius: var(--radius-md);
  transition: background var(--transition-fast);
}
.dir-section-row:hover, .run-row:hover, .dir-app-row:hover { background: var(--sidebar-hover-bg); }
.dir-section-row.selected, .run-row.selected { background: var(--sidebar-active-bg); }

.dir-section-open, .run-open, .dir-app-open {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
  padding: 5px 8px 5px 2px;
  border: 0;
  border-radius: 0 var(--radius-md) var(--radius-md) 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.dir-section-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: 600; color: var(--text-primary); }
.dir-section-row.selected .dir-section-name, .run-row.selected .run-name { color: var(--primary-color); }
.dir-target { color: var(--text-secondary); font-size: 11px; max-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.dir-logo, .dir-leaf-logo { width: 14px; height: 14px; flex-shrink: 0; border-radius: 3px; object-fit: contain; }
.section-icon { flex-shrink: 0; color: var(--text-secondary); opacity: 0.55; }

.run-toggle { display: grid; place-items: center; flex: none; width: 22px; height: 28px; padding: 0; border: 0; border-radius: var(--radius-md) 0 0 var(--radius-md); background: transparent; color: var(--text-secondary); cursor: pointer; }
.run-toggle.sm { width: 18px; height: 28px; }
.run-toggle:hover { color: var(--text-primary); }
.chevron { transition: transform var(--motion-normal) var(--ease-out); }
.chevron.open { transform: rotate(90deg); }

.run-text { display: flex; flex: 1; flex-direction: column; gap: 1px; min-width: 0; }
.run-name, .run-target { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.run-name { color: var(--text-primary); font-size: 12px; font-weight: 600; line-height: 16px; }
.run-target { color: var(--text-secondary); font-size: 11px; line-height: 14px; }
.run-open .ui-badge, .dir-section-open .ui-badge { flex-shrink: 0; }
.run-warnings { gap: 3px; font-family: var(--font-mono); }

.dir-children { margin-left: 10px; padding-left: 6px; border-left: 1px solid var(--border-color); }
.result-list, .dir-sub-list, .dir-facet-list { margin: 2px 0 4px; padding: 0; list-style: none; }
.dir-sub-list, .dir-facet-list { margin-left: 14px; padding-left: 8px; border-left: 1px solid var(--border-color); }

.dir-app-open .result-name { font-size: 12px; font-weight: 500; color: var(--text-primary); }

.result-item {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1;
  min-width: 0;
  width: 100%;
  height: 29px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
  transition: background var(--transition-fast), color var(--transition-fast);
}
.result-item.with-toggle { padding-left: 2px; }
.result-item.facet { height: 27px; font-size: 11.5px; }
.result-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.result-count { flex-shrink: 0; color: var(--text-secondary); opacity: 0.75; font-family: var(--font-mono); font-size: 11px; font-variant-numeric: tabular-nums; }
.result-item.empty { color: var(--text-secondary); }
.result-item:hover { background: var(--sidebar-hover-bg); }
.result-icon { flex-shrink: 0; color: var(--text-secondary); opacity: 0.55; }
.result-item.selected .result-icon { color: var(--primary-color); opacity: 1; }
.result-item.selected { background: var(--sidebar-active-bg); color: var(--primary-color); font-weight: 500; }
.result-item.selected .result-count { color: var(--primary-color); opacity: 1; }

.run-toggle:focus-visible, .dir-section-open:focus-visible, .run-open:focus-visible, .dir-app-open:focus-visible, .result-item:focus-visible {
  outline: 2px solid var(--primary-color);
  outline-offset: -2px;
}
.group-empty { padding: 4px 8px 6px; color: var(--text-secondary); font-size: 11px; line-height: 1.5; }
.directory-state { padding: 20px 10px; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 12px; color: var(--text-secondary); overflow-wrap: anywhere; }
</style>
