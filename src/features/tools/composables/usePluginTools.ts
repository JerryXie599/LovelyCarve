import { computed, ref, shallowRef, watch } from 'vue';
import { analysisRecordsPage, type AnalysisRecord, type AnalysisResultNode } from '../../../lib/ipc/analysisResults';
import type { ImageId } from '../../../lib/ipc/ids';
import { schedTaskCreate } from '../../../lib/ipc/tasks';
import { useAnalysisResults } from '../../analysis/api';
import { usePluginCatalog } from '../../tasks/api';
import {
  collectPluginToolGroups,
  extractToolItemsFromRecords,
  type PluginToolGroup,
  type PluginToolItem,
} from '../model/pluginTools';

const activeImageId = ref<ImageId | null>(null);
const expandedTools = ref<Set<string>>(new Set(['git_workspace']));
const selectedKeyByTool = ref<Record<string, string>>({});
const itemsByTool = shallowRef<Record<string, PluginToolItem[]>>({});
const loadingByTool = ref<Record<string, boolean>>({});
const scanningByTool = ref<Record<string, boolean>>({});

let refreshSeq = 0;
let requestedToolId: string | null = null;
let requestedItemKey: string | null = null;

export function focusPluginTool(toolId: string, itemKey?: string | null): void {
  requestedToolId = toolId;
  expandedTools.value = new Set([...expandedTools.value, toolId]);
  if (itemKey) {
    requestedItemKey = itemKey;
    selectedKeyByTool.value = { ...selectedKeyByTool.value, [toolId]: itemKey };
  }
}

function collectMatchingResultNodes(
  nodes: AnalysisResultNode[],
  imageId: ImageId,
  group: PluginToolGroup,
): AnalysisResultNode[] {
  const found: AnalysisResultNode[] = [];
  const fnSet = new Set(group.functions);
  function visit(list: AnalysisResultNode[]) {
    for (const node of list) {
      if (node.imageId !== imageId) continue;
      if (
        node.kind === 'result' &&
        node.artifactType === group.itemRecord &&
        fnSet.has(node.function)
      ) {
        found.push(node);
      } else if (node.children.length > 0) {
        visit(node.children);
      }
    }
  }
  visit(nodes);
  return found;
}

export function usePluginTools() {
  const catalog = usePluginCatalog();
  const analysis = useAnalysisResults();

  const toolGroups = computed(() =>
    collectPluginToolGroups(catalog.packages.value, catalog.text),
  );

  async function refreshToolItems(imageIdOverride?: ImageId | null): Promise<void> {
    const img = imageIdOverride ?? activeImageId.value;
    if (!img) {
      itemsByTool.value = {};
      return;
    }
    const seq = ++refreshSeq;
    const groups = toolGroups.value;
    const nextItems: Record<string, PluginToolItem[]> = {};

    for (const group of groups) {
      const nodes = collectMatchingResultNodes(analysis.tree.value, img, group);
      if (nodes.length === 0) {
        nextItems[group.toolId] = [];
        continue;
      }
      loadingByTool.value = { ...loadingByTool.value, [group.toolId]: true };
      try {
        const allRecords: AnalysisRecord[] = [];
        for (const node of nodes) {
          const page = await analysisRecordsPage({
            imageId: node.imageId,
            runId: node.runId,
            recordType: node.artifactType,
            legacy: node.legacy,
            skip: 0,
            limit: 500,
          });
          allRecords.push(...page.items);
        }
        if (seq !== refreshSeq) return;
        const extracted = extractToolItemsFromRecords(group, allRecords);
        nextItems[group.toolId] = extracted;

        // Auto-select first item if current selection is missing or empty
        const currentKey = selectedKeyByTool.value[group.toolId];
        if (
          extracted.length > 0 &&
          (!currentKey || !extracted.some(item => item.key === currentKey))
        ) {
          selectedKeyByTool.value = {
            ...selectedKeyByTool.value,
            [group.toolId]: requestedToolId === group.toolId && requestedItemKey
              ? requestedItemKey
              : extracted[0].key,
          };
        }
      } finally {
        if (seq === refreshSeq) {
          loadingByTool.value = { ...loadingByTool.value, [group.toolId]: false };
        }
      }
    }

    if (seq === refreshSeq) {
      itemsByTool.value = nextItems;
      requestedToolId = null;
      requestedItemKey = null;
    }
  }

  function setImageId(imageId: ImageId | null): void {
    if (activeImageId.value === imageId) return;
    activeImageId.value = imageId;
    if (imageId) {
      analysis.setImageId(imageId);
      analysis.ensureFresh();
    } else {
      itemsByTool.value = {};
    }
  }

  watch(
    [() => activeImageId.value, () => analysis.tree.value, toolGroups],
    () => {
      void refreshToolItems();
    },
    { immediate: true },
  );

  function isToolExpanded(toolId: string): boolean {
    return expandedTools.value.has(toolId);
  }

  function toggleToolExpanded(toolId: string): void {
    const next = new Set(expandedTools.value);
    if (next.has(toolId)) next.delete(toolId);
    else next.add(toolId);
    expandedTools.value = next;
  }

  function selectToolItem(toolId: string, itemKey: string): void {
    expandedTools.value = new Set([...expandedTools.value, toolId]);
    selectedKeyByTool.value = { ...selectedKeyByTool.value, [toolId]: itemKey };
  }

  async function triggerToolScan(
    toolId: string,
    imageId: ImageId,
    options?: Record<string, unknown>,
  ): Promise<boolean> {
    const group = toolGroups.value.find(g => g.toolId === toolId);
    if (!group || !imageId) return false;
    scanningByTool.value = { ...scanningByTool.value, [toolId]: true };
    try {
      await schedTaskCreate({
        function: group.primaryFunction,
        imageId,
        options: options ?? {},
      });
      return true;
    } finally {
      scanningByTool.value = { ...scanningByTool.value, [toolId]: false };
    }
  }

  function selectedItemFor(toolId: string): PluginToolItem | null {
    const list = itemsByTool.value[toolId] ?? [];
    const key = selectedKeyByTool.value[toolId];
    return list.find(i => i.key === key) ?? list[0] ?? null;
  }

  return {
    toolGroups,
    itemsByTool,
    loadingByTool,
    scanningByTool,
    selectedKeyByTool,
    setImageId,
    refreshToolItems,
    isToolExpanded,
    toggleToolExpanded,
    selectToolItem,
    triggerToolScan,
    selectedItemFor,
    analysisTreeDirty: analysis.dirty,
    reloadAnalysisTree: analysis.reload,
  };
}
