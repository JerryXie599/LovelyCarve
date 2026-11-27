<template>
  <GitRepoWorkspacePanel
    v-if="group?.view === 'git_repo'"
    :image-id="imageId"
    :group="group"
    @open-file="emit('openFile', $event)"
  />

  <section v-else-if="group" class="generic-tool-panel">
    <header class="ui-card gtp-header">
      <div class="gtp-title-row">
        <Wrench :size="18" class="gtp-icon" />
        <h2 class="gtp-title">{{ group.label }}</h2>
        <UiSelect
          v-if="itemOptions.length"
          :model-value="selectedItem?.key ?? ''"
          :options="itemOptions"
          class="sm gtp-select"
          @update:model-value="selectToolItem(group.toolId, $event)"
        />
        <button
          type="button"
          class="ui-btn sm"
          :disabled="Boolean(scanningByTool[group.toolId])"
          @click="triggerScan"
        >
          <Loader2 v-if="scanningByTool[group.toolId]" :size="13" class="spin" />
          <ScanSearch v-else :size="13" />
          <span>{{ tr("扫描项目") }}</span>
        </button>
      </div>
    </header>

    <div v-if="!selectedItem" class="ui-card gtp-empty">
      <span>{{ tr("尚未扫描到项目，点击上方按钮开始扫描。") }}</span>
    </div>

    <div v-else class="ui-card gtp-fields">
      <h3 class="gtp-item-title">{{ selectedItem.label }}</h3>
      <p v-if="selectedItem.sublabel" class="gtp-item-sub mono">{{ selectedItem.sublabel }}</p>
      <dl class="gtp-dl">
        <div v-for="(val, key) in selectedItem.values" :key="key" class="gtp-row">
          <dt class="mono">{{ key }}</dt>
          <dd class="mono">{{ formatVal(val) }}</dd>
        </div>
      </dl>
    </div>
  </section>

  <section v-else class="generic-tool-panel">
    <div class="ui-card gtp-empty">
      <span>{{ tr("未找到对应插件工具 ({0})", { 0: toolId }) }}</span>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, watch } from 'vue';
import { Loader2, ScanSearch, Wrench } from 'lucide-vue-next';
import UiSelect, { type UiSelectOption } from '../../../components/ui/UiSelect.vue';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifyInfo } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import type { FileLocationTarget } from '../../../lib/models/fileLocation';
import { usePluginTools } from '../composables/usePluginTools';
import GitRepoWorkspacePanel from './GitRepoWorkspacePanel.vue';

const props = defineProps<{
  imageId: ImageId;
  toolId: string;
}>();

const emit = defineEmits<{
  openFile: [target: FileLocationTarget];
}>();

const {
  toolGroups,
  itemsByTool,
  scanningByTool,
  setImageId,
  selectToolItem,
  selectedItemFor,
  triggerToolScan,
} = usePluginTools();

watch(
  () => props.imageId,
  id => {
    setImageId(id || null);
  },
  { immediate: true },
);

const group = computed(() =>
  toolGroups.value.find(g => g.toolId === props.toolId) ?? null,
);

const items = computed(() =>
  group.value ? itemsByTool.value[group.value.toolId] ?? [] : [],
);

const itemOptions = computed<UiSelectOption<string>[]>(() =>
  items.value.map(item => ({
    value: item.key,
    label: item.label,
  })),
);

const selectedItem = computed(() =>
  group.value ? selectedItemFor(group.value.toolId) : null,
);

async function triggerScan() {
  if (!group.value) return;
  try {
    await triggerToolScan(group.value.toolId, props.imageId);
    notifyInfo(tr("已提交扫描任务"), tr("完成后将自动加载项目列表。"));
  } catch (err) {
    notifyError(tr("扫描失败"), String(err));
  }
}

function formatVal(val: unknown): string {
  if (val === null || val === undefined) return '—';
  if (typeof val === 'string') return val;
  return JSON.stringify(val);
}
</script>

<style scoped>
.generic-tool-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px 18px;
  height: 100%;
  overflow-y: auto;
}

.gtp-header {
  padding: 12px 16px;
}

.gtp-fields {
  padding: 14px 16px;
}

.gtp-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.gtp-icon {
  color: var(--primary-color);
}

.gtp-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}

.gtp-select {
  max-width: 320px;
}

.gtp-empty {
  padding: 36px;
  text-align: center;
  font-size: 13px;
  color: var(--text-secondary);
}

.gtp-item-title {
  margin: 0 0 4px;
  font-size: 15px;
}

.gtp-item-sub {
  margin: 0 0 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.gtp-dl {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
}

.gtp-row {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 10px;
  padding: 6px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  font-size: 12px;
}

.gtp-row dt {
  color: var(--text-secondary);
}

.gtp-row dd {
  margin: 0;
  word-break: break-word;
}
</style>
