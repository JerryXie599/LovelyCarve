<template>
  <section class="plugin-tools-nav" :aria-label="tr('插件工具')">
    <div v-for="group in toolGroups" :key="group.toolId" class="pt-group">
      <div class="pt-header" :class="{ active: isGroupPanelActive(group.toolId) }">
        <button
          type="button"
          class="pt-toggle"
          :aria-expanded="isToolExpanded(group.toolId)"
          :aria-label="isToolExpanded(group.toolId) ? tr('收起 {0}', { 0: group.label }) : tr('展开 {0}', { 0: group.label })"
          :title="isToolExpanded(group.toolId) ? tr('收起') : tr('展开')"
          @click="toggleToolExpanded(group.toolId)"
        >
          <ChevronRight :size="14" class="pt-chevron" :class="{ open: isToolExpanded(group.toolId) }" />
        </button>

        <button
          type="button"
          class="pt-title-btn"
          :title="group.label"
          @click="handleGroupClick(group.toolId)"
        >
          <component :is="resolveIcon(group.icon)" :size="15" class="pt-icon" />
          <span class="pt-label">{{ group.label }}</span>
          <span v-if="itemsOf(group.toolId).length > 0" class="ui-badge sm pt-count">
            {{ itemsOf(group.toolId).length }}
          </span>
        </button>

        <button
          type="button"
          class="ui-icon-btn sm pt-scan-btn"
          :disabled="Boolean(scanningByTool[group.toolId] || loadingByTool[group.toolId])"
          :title="tr('扫描 {0}', { 0: group.label })"
          :aria-label="tr('扫描 {0}', { 0: group.label })"
          @click.stop="handleQuickScan(group.toolId)"
        >
          <Loader2
            v-if="scanningByTool[group.toolId] || loadingByTool[group.toolId]"
            :size="12"
            class="spin"
          />
          <ScanSearch v-else :size="12" />
        </button>
      </div>

      <UiTransition>
        <div v-if="isToolExpanded(group.toolId)" class="pt-dropdown">
          <div v-if="loadingByTool[group.toolId] && itemsOf(group.toolId).length === 0" class="pt-empty">
            <Loader2 :size="12" class="spin" />
            <span>{{ tr("正在加载项目…") }}</span>
          </div>

          <div v-else-if="itemsOf(group.toolId).length === 0" class="pt-empty-box">
            <span class="pt-empty-text">{{ tr("尚未扫描到项目") }}</span>
            <button
              type="button"
              class="ui-btn sm pt-empty-scan"
              :disabled="Boolean(scanningByTool[group.toolId])"
              @click="handleQuickScan(group.toolId)"
            >
              <ScanSearch :size="12" />
              <span>{{ tr("立即扫描") }}</span>
            </button>
          </div>

          <ul v-else class="pt-item-list">
            <li v-for="item in itemsOf(group.toolId)" :key="item.key">
              <button
                type="button"
                class="pt-item-btn"
                :class="{ active: isItemActive(group.toolId, item.key) }"
                :aria-current="isItemActive(group.toolId, item.key) ? 'true' : undefined"
                :title="item.sublabel ? `${item.label} · ${item.sublabel}` : item.label"
                @click="handleItemClick(group.toolId, item.key)"
              >
                <FolderGit2 :size="14" class="pt-item-icon" />
                <span class="pt-item-text">
                  <span class="pt-item-name">{{ item.label }}</span>
                  <span v-if="item.sublabel" class="pt-item-sub mono">{{ item.sublabel }}</span>
                </span>
              </button>
            </li>
          </ul>
        </div>
      </UiTransition>
    </div>
  </section>
</template>

<script setup lang="ts">
import { watch, type Component } from 'vue';
import {
  ChevronRight,
  FolderGit2,
  GitBranch,
  Loader2,
  ScanSearch,
  Wrench,
} from 'lucide-vue-next';
import UiTransition from '../../../components/ui/UiTransition.vue';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifyInfo } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import { usePluginTools } from '../composables/usePluginTools';

const props = defineProps<{
  imageId: ImageId;
  activePanel: string;
}>();

const emit = defineEmits<{
  select: [panelId: string];
}>();

const {
  toolGroups,
  itemsByTool,
  loadingByTool,
  scanningByTool,
  selectedKeyByTool,
  setImageId,
  isToolExpanded,
  toggleToolExpanded,
  selectToolItem,
  triggerToolScan,
} = usePluginTools();

watch(
  () => props.imageId,
  id => {
    setImageId(id || null);
  },
  { immediate: true },
);

function itemsOf(toolId: string) {
  return itemsByTool.value[toolId] ?? [];
}

function isGroupPanelActive(toolId: string): boolean {
  return props.activePanel === `tool:${toolId}`;
}

function isItemActive(toolId: string, itemKey: string): boolean {
  if (!isGroupPanelActive(toolId)) return false;
  const activeKey = selectedKeyByTool.value[toolId] ?? itemsOf(toolId)[0]?.key;
  return activeKey === itemKey;
}

function handleGroupClick(toolId: string): void {
  if (!isToolExpanded(toolId)) {
    toggleToolExpanded(toolId);
  }
  emit('select', `tool:${toolId}`);
}

function handleItemClick(toolId: string, itemKey: string): void {
  selectToolItem(toolId, itemKey);
  emit('select', `tool:${toolId}`);
}

async function handleQuickScan(toolId: string): Promise<void> {
  if (!props.imageId) return;
  try {
    const ok = await triggerToolScan(toolId, props.imageId);
    if (ok) {
      notifyInfo(tr("已提交扫描任务"), tr("正在扫描检材中的项目，完成后会自动出现在工具列表。"));
      emit('select', `tool:${toolId}`);
    }
  } catch (error) {
    notifyError(tr("创建扫描任务失败"), String(error));
  }
}

const ICON_MAP: Record<string, Component> = {
  GitBranch,
  FolderGit2,
  Wrench,
};

function resolveIcon(name: string): Component {
  return ICON_MAP[name] ?? Wrench;
}
</script>

<style scoped>
.plugin-tools-nav {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 6px 8px 14px 4px;
  border-top: 1px solid var(--border-color);
}

.pt-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.pt-header {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 30px;
  padding-right: 4px;
  border-radius: var(--radius-md);
  transition: background var(--transition-fast);
}

.pt-header:hover {
  background: var(--sidebar-hover-bg);
}

.pt-header.active {
  background: color-mix(in srgb, var(--primary-color) 10%, transparent);
}

.pt-toggle {
  display: grid;
  place-items: center;
  flex: none;
  width: 22px;
  height: 28px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.pt-chevron {
  transition: transform var(--motion-normal) var(--ease-out);
}

.pt-chevron.open {
  transform: rotate(90deg);
}

.pt-title-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 4px 0 0;
  border: 0;
  background: transparent;
  color: var(--text-primary);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
}

.pt-header.active .pt-title-btn,
.pt-header.active .pt-icon {
  color: var(--primary-color);
}

.pt-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.pt-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pt-count {
  flex-shrink: 0;
}

.pt-scan-btn {
  flex-shrink: 0;
  opacity: 0.8;
}

.pt-scan-btn:hover {
  opacity: 1;
}

.pt-dropdown {
  margin: 2px 0 4px 11px;
  padding-left: 6px;
  border-left: 1px solid var(--border-color);
}

.pt-item-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.pt-item-btn {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 5px 8px;
  border: 0;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background var(--transition-fast), color var(--transition-fast);
}

.pt-item-btn:hover {
  background: var(--sidebar-hover-bg);
}

.pt-item-btn.active {
  background: var(--sidebar-active-bg);
  color: var(--primary-color);
}

.pt-item-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.pt-item-btn.active .pt-item-icon {
  color: var(--primary-color);
}

.pt-item-text {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  gap: 1px;
}

.pt-item-name {
  font-size: 12px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pt-item-sub {
  font-size: 10px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  opacity: 0.85;
}

.pt-empty {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  font-size: 11px;
  color: var(--text-secondary);
}

.pt-empty-box {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 6px 8px;
}

.pt-empty-text {
  font-size: 11px;
  color: var(--text-secondary);
}

.pt-empty-scan {
  --ui-btn-h: 24px;
  font-size: 11px;
}
</style>
