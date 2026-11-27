<template>
  <div v-if="quickPinDraft" class="modal-backdrop" @click.self="closeQuickPin">
    <div class="modal-card quick-pin-modal" role="dialog" :aria-label="tr('Pin 到 WP 留证')">
      <header class="modal-header">
        <div>
          <h3>{{ tr("Pin 到 WP 留证") }}</h3>
          <p class="modal-sub mono">{{ formatPinTargetLabel(quickPinDraft.target) }}</p>
        </div>
        <button class="ui-icon-btn" type="button" :aria-label="tr('关闭')" @click="closeQuickPin">
          <X :size="16" />
        </button>
      </header>

      <div class="modal-body">
        <label class="field-label">
          <span>{{ tr("证据标题") }}</span>
          <input v-model="title" type="text" class="ui-input" :placeholder="tr('如：可疑登录时间记录')" />
        </label>

        <div class="field-row">
          <label class="field-label">
            <span>{{ tr("关联题目（可选）") }}</span>
            <select v-model="questionId" class="ui-select">
              <option value="">{{ tr("不关联题目（自由留证）") }}</option>
              <option v-for="q in workspace?.questions ?? []" :key="q.id" :value="q.id">
                {{ q.id }} · {{ q.stem.slice(0, 36) }}
              </option>
            </select>
          </label>

          <label class="field-label">
            <span>{{ tr("关联检材（可选）") }}</span>
            <select v-model="exhibitId" class="ui-select">
              <option value="">{{ tr("自动或无") }}</option>
              <option v-for="ex in workspace?.exhibits ?? []" :key="ex.id" :value="ex.id">
                {{ ex.name }}
              </option>
            </select>
          </label>
        </div>

        <label class="field-label">
          <span>{{ tr("证据说明 / 结论备注") }}</span>
          <textarea v-model="summary" class="ui-textarea" rows="3" :placeholder="tr('记录该证据如何支撑答案或排查结论')" />
        </label>

        <label class="field-label">
          <span>{{ tr("命令记录（可选）") }}</span>
          <input v-model="command" type="text" class="ui-input mono" :placeholder="tr('如：lovelycarve-cli sqlite query ...')" />
        </label>
      </div>

      <footer class="modal-footer">
        <button class="ui-btn ghost" type="button" @click="closeQuickPin">{{ tr("取消") }}</button>
        <button class="ui-btn" type="button" :disabled="saving || !title.trim()" @click="saveOnly(false)">
          {{ tr("仅保存 Pin") }}
        </button>
        <button class="ui-btn primary" type="button" :disabled="saving || !title.trim()" @click="saveOnly(true)">
          <Camera :size="14" />
          <span>{{ tr("保存并框选截图") }}</span>
        </button>
      </footer>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { Camera, X } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import { formatPinTargetLabel } from '../model/contest';
import { useContestWorkspace } from '../composables/useContestWorkspace';

const {
  workspace,
  quickPinDraft,
  closeQuickPin,
  addContestPin,
  startPinCapture,
} = useContestWorkspace();

const title = ref('');
const summary = ref('');
const questionId = ref('');
const exhibitId = ref('');
const command = ref('');
const saving = ref(false);

watch(quickPinDraft, draft => {
  if (!draft) return;
  title.value = draft.title;
  summary.value = draft.summary;
  questionId.value = '';
  exhibitId.value = draft.exhibitId ?? '';
  command.value = '';
});

async function saveOnly(withCapture: boolean) {
  if (!quickPinDraft.value || !title.value.trim()) return;
  saving.value = true;
  try {
    const pin = await addContestPin({
      title: title.value.trim(),
      summary: summary.value.trim(),
      questionId: questionId.value || null,
      exhibitId: exhibitId.value || null,
      command: command.value.trim() || null,
      source: 'desktop',
      target: quickPinDraft.value.target,
    });
    closeQuickPin();
    if (withCapture) {
      await startPinCapture(pin, { navigateFirst: false, returnToContest: false });
    } else {
      notifySuccess(tr('已固定证据'), pin.title);
    }
  } catch (err) {
    notifyError(tr('保存 Pin 失败'), String(err));
  } finally {
    saving.value = false;
  }
}
</script>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 9100;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
}

.quick-pin-modal {
  width: min(560px, 100%);
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
  word-break: break-all;
}

.modal-body {
  padding: 14px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.field-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.field-label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary, #6b7280);
}

.field-label .ui-input,
.field-label .ui-select,
.field-label .ui-textarea {
  color: var(--text-color, #111827);
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px 16px;
  border-top: 1px solid var(--border-color, rgba(128, 128, 128, 0.18));
}
</style>
