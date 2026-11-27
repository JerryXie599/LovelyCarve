<template>
  <div v-if="visible" class="modal-backdrop" @click.self="$emit('close')">
    <div class="modal-card contest-import-modal" role="dialog" :aria-label="tr('导入赛题')">
      <header class="modal-header">
        <div>
          <h3>{{ tr("导入赛题或检材元数据") }}</h3>
          <p class="modal-sub">{{ tr("可选导入标准 Markdown / JSON 赛题；不导入也可直接使用证据 Pin 与 Writeup") }}</p>
        </div>
        <button class="ui-icon-btn" type="button" :aria-label="tr('关闭')" @click="$emit('close')">
          <X :size="16" />
        </button>
      </header>

      <div class="modal-body">
        <div class="import-toolbar">
          <button class="ui-btn sm" type="button" @click="pickFile">
            <FolderOpen :size="14" />
            <span>{{ tr("选择本地赛题文件") }}</span>
          </button>
          <select v-model="format" class="ui-select sm">
            <option value="auto">{{ tr("自动识别格式") }}</option>
            <option value="markdown">{{ tr("Markdown 题库规范") }}</option>
            <option value="json">{{ tr("JSON 题库规范") }}</option>
            <option value="exhibits_meta">exhibits_meta.json</option>
          </select>
          <input v-model="title" type="text" class="ui-input sm title-input" :placeholder="tr('工作台标题（可选）')" />
          <label class="merge-check">
            <input v-model="merge" type="checkbox" />
            <span>{{ tr("合并保留已有作答与 Pin") }}</span>
          </label>
        </div>

        <textarea
          v-model="content"
          class="ui-textarea import-textarea mono"
          rows="11"
          :placeholder="tr('在此粘贴赛题 Markdown / JSON 内容，或点击上方选择文件…')"
        />

        <details class="spec-details">
          <summary>{{ tr("查看题目格式规范 & AI 整理提示词") }}</summary>
          <div class="spec-box">
            <div class="spec-actions">
              <span class="spec-tip">{{ tr("若原题格式杂乱，可复制下方提示词让 AI 整理后再导入") }}</span>
              <button class="ui-btn sm" type="button" @click="copyPrompt">
                <Copy :size="13" />
                <span>{{ tr("复制提示词") }}</span>
              </button>
            </div>
            <pre class="spec-pre mono">{{ CONTEST_AI_FORMAT_PROMPT }}</pre>
          </div>
        </details>
      </div>

      <footer class="modal-footer">
        <button class="ui-btn ghost" type="button" @click="$emit('close')">{{ tr("取消") }}</button>
        <button class="ui-btn primary" type="button" :disabled="submitting || !content.trim()" @click="submit">
          {{ submitting ? tr("正在导入…") : tr("导入到工作台") }}
        </button>
      </footer>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { readTextFile } from '@tauri-apps/plugin-fs';
import { Copy, FolderOpen, X } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { copyText } from '../../../composables/clipboard';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import { CONTEST_AI_FORMAT_PROMPT } from '../model/contest';
import { useContestWorkspace } from '../composables/useContestWorkspace';

defineProps<{ visible: boolean }>();
const emit = defineEmits<{ close: [] }>();

const { importContestContent } = useContestWorkspace();

const content = ref('');
const format = ref('auto');
const title = ref('');
const merge = ref(false);
const submitting = ref(false);

async function pickFile() {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: '赛题文件', extensions: ['md', 'markdown', 'txt', 'json'] }],
    });
    if (!selected || Array.isArray(selected)) return;
    content.value = await readTextFile(selected);
    if (!title.value.trim()) {
      const base = selected.split(/[/\\]/).pop() ?? '';
      title.value = base.replace(/\.(md|markdown|txt|json)$/i, '');
    }
  } catch (err) {
    notifyError(tr('读取赛题文件失败'), String(err));
  }
}

async function copyPrompt() {
  await copyText(CONTEST_AI_FORMAT_PROMPT, tr('赛题格式整理提示词'));
}

async function submit() {
  if (!content.value.trim()) return;
  submitting.value = true;
  try {
    const ws = await importContestContent(
      content.value,
      format.value,
      title.value.trim() || undefined,
      merge.value,
    );
    notifySuccess(
      tr('导入完成'),
      tr('共 {0} 题，{1} 个检材', { 0: ws.questions.length, 1: ws.exhibits.length }),
    );
    emit('close');
  } catch (err) {
    notifyError(tr('导入失败'), String(err));
  } finally {
    submitting.value = false;
  }
}
</script>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 9000;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}

.contest-import-modal {
  width: min(760px, 100%);
  max-height: 88vh;
  background: var(--bg-elevated, #ffffff);
  color: var(--text-color, #111827);
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.25));
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.25);
}

.modal-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  padding: 16px 20px 12px;
  border-bottom: 1px solid var(--border-color, rgba(128, 128, 128, 0.18));
}

.modal-header h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}

.modal-sub {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
}

.modal-body {
  padding: 14px 20px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.import-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.title-input {
  min-width: 170px;
  flex: 1;
}

.merge-check {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
  cursor: pointer;
}

.import-textarea {
  width: 100%;
  resize: vertical;
  font-size: 12px;
  line-height: 1.5;
  padding: 10px 12px;
  border-radius: 8px;
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.25));
  background: var(--bg-color, rgba(128, 128, 128, 0.05));
  color: inherit;
}

.spec-details summary {
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
  cursor: pointer;
  user-select: none;
}

.spec-box {
  margin-top: 8px;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-color, rgba(128, 128, 128, 0.06));
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.18));
}

.spec-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}

.spec-tip {
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
}

.spec-pre {
  margin: 0;
  white-space: pre-wrap;
  font-size: 11px;
  line-height: 1.5;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px 16px;
  border-top: 1px solid var(--border-color, rgba(128, 128, 128, 0.18));
}
</style>
