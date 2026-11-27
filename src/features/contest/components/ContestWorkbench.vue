<template>
  <section class="contest-workbench">
    <div v-if="!activeCase" class="contest-empty">
      <h3>{{ tr("请先打开或创建案件") }}</h3>
      <p>{{ tr("赛题、证据 Pin 与 Writeup 均自动保存在案件目录内，支持桌面端与 CLI 双向同步。") }}</p>
      <button class="ui-btn primary" type="button" @click="$emit('open-case-manager')">
        {{ tr("打开案件管理") }}
      </button>
    </div>

    <template v-else>
      <header class="workbench-header">
        <div class="header-left">
          <h2 class="ws-title">{{ workspace?.title || tr("竞赛做题工作台") }}</h2>
          <span class="ws-stats mono">
            {{ tr("已答 {0}/{1}", { 0: summary.answeredQuestions, 1: summary.totalQuestions }) }}
            · {{ tr("检材 {0}/{1}", { 0: summary.boundExhibits, 1: summary.totalExhibits }) }}
            · {{ tr("Pin {0}", { 0: summary.totalPins }) }}
          </span>
        </div>

        <div class="header-actions">
          <button class="ui-btn sm" type="button" @click="importModalOpen = true">
            <FileUp :size="14" />
            <span>{{ tr("导入赛题") }}</span>
          </button>
          <button class="ui-btn sm" type="button" @click="handleExportWp">
            <Download :size="14" />
            <span>{{ tr("导出 WP") }}</span>
          </button>
          <button class="ui-btn ghost sm" type="button" :disabled="loading" @click="refreshWorkspace()">
            <RefreshCw :size="14" :class="{ spin: loading }" />
            <span>{{ tr("刷新") }}</span>
          </button>
          <button v-if="hasLoadedImage" class="ui-btn ghost sm" type="button" @click="$emit('back-to-evidence')">
            <span>{{ tr("返回检材") }}</span>
          </button>
        </div>
      </header>

      <!-- 检材与案件证据映射条 -->
      <div v-if="workspace?.exhibits.length" class="exhibit-bar">
        <div
          v-for="ex in workspace.exhibits"
          :key="ex.id"
          class="exhibit-chip"
          :class="{ active: activeScopeExhibitId === ex.id }"
        >
          <button class="exhibit-name" type="button" @click="selectExhibitScope(ex.id)">
            {{ ex.name }}
          </button>
          <select
            class="exhibit-bind-select"
            :value="ex.imageId ?? ''"
            :title="tr('绑定案件中的镜像检材')"
            @change="onBindExhibit(ex.id, ($event.target as HTMLSelectElement).value)"
          >
            <option value="">{{ tr("未映射检材") }}</option>
            <option v-for="ev in activeCase.evidence" :key="ev.image_id" :value="ev.image_id">
              {{ evidenceShortName(ev.path) }}
            </option>
          </select>
          <button
            v-if="ex.imageId"
            class="ui-icon-btn sm"
            type="button"
            :title="tr('在工作区打开此检材')"
            @click="openExhibitInWorkspace(ex.imageId)"
          >
            <ExternalLink :size="12" />
          </button>
        </div>
        <button
          v-if="activeScopeExhibitId !== 'all'"
          class="ui-btn ghost sm"
          type="button"
          @click="selectExhibitScope('all')"
        >
          {{ tr("显示全部检材") }}
        </button>
      </div>

      <div class="split-body">
        <!-- 左栏：题目卡片与证据 Pin -->
        <div class="left-pane">
          <div class="pane-toolbar">
            <div class="subtab-group">
              <button
                type="button"
                class="subtab-btn"
                :class="{ active: leftTab === 'questions' }"
                @click="leftTab = 'questions'"
              >
                {{ tr("题目 ({0})", { 0: summary.totalQuestions }) }}
              </button>
              <button
                type="button"
                class="subtab-btn"
                :class="{ active: leftTab === 'pins' }"
                @click="leftTab = 'pins'"
              >
                {{ tr("证据 Pin ({0})", { 0: summary.totalPins }) }}
              </button>
            </div>

            <div class="filter-row">
              <input
                v-model="keyword"
                type="text"
                class="ui-input sm search-input"
                :placeholder="leftTab === 'questions' ? tr('搜索题干/答案…') : tr('搜索 Pin…')"
              />
              <select v-if="leftTab === 'questions'" v-model="statusFilter" class="ui-select sm">
                <option value="all">{{ tr("全部状态") }}</option>
                <option value="unanswered">{{ tr("未作答") }}</option>
                <option value="answered">{{ tr("已作答") }}</option>
                <option value="review">{{ tr("待复核") }}</option>
                <option value="stuck">{{ tr("疑难") }}</option>
              </select>
              <button
                v-if="leftTab === 'questions'"
                class="ui-btn sm"
                type="button"
                @click="addQuestionOpen = !addQuestionOpen"
              >
                <Plus :size="13" />
                <span>{{ tr("加题") }}</span>
              </button>
              <button
                v-else
                class="ui-btn sm"
                type="button"
                @click="openNewFreePin"
              >
                <Plus :size="13" />
                <span>{{ tr("自由 Pin") }}</span>
              </button>
            </div>
          </div>

          <!-- 手动补录题目 -->
          <div v-if="addQuestionOpen && leftTab === 'questions'" class="quick-add-box">
            <input v-model="newStem" type="text" class="ui-input sm" :placeholder="tr('输入题干…')" />
            <div class="quick-add-row">
              <select v-model="newExhibitId" class="ui-select sm">
                <option value="">{{ tr("默认检材") }}</option>
                <option v-for="ex in workspace?.exhibits ?? []" :key="ex.id" :value="ex.id">
                  {{ ex.name }}
                </option>
              </select>
              <button class="ui-btn primary sm" type="button" :disabled="!newStem.trim()" @click="createManualQuestion">
                {{ tr("添加") }}
              </button>
            </div>
          </div>

          <!-- 题目列表 -->
          <div v-if="leftTab === 'questions'" class="cards-scroll">
            <div v-if="!filteredQuestions.length" class="pane-empty">
              <p>{{ tr("当前暂无题目。可点击右上角「导入赛题」，或切换到「证据 Pin」直接留证写 WP。") }}</p>
            </div>

            <article
              v-for="q in filteredQuestions"
              :key="q.id"
              class="question-card"
              :class="`status-${q.status}`"
            >
              <div class="q-head">
                <div class="q-meta">
                  <span class="q-id mono">{{ q.id }}</span>
                  <span v-if="q.score != null" class="ui-badge sm">{{ tr("{0} 分", { 0: q.score }) }}</span>
                  <span class="q-exhibit">{{ exhibitNameOf(q.exhibitId) }}</span>
                </div>
                <select
                  class="status-select"
                  :value="q.status"
                  @change="updateQuestionField(q, { status: ($event.target as HTMLSelectElement).value as QuestionStatus })"
                >
                  <option value="unanswered">{{ tr("未作答") }}</option>
                  <option value="answered">{{ tr("已作答") }}</option>
                  <option value="review">{{ tr("待复核") }}</option>
                  <option value="stuck">{{ tr("疑难") }}</option>
                </select>
              </div>

              <p class="q-stem">{{ q.stem }}</p>
              <p v-if="q.formatHint" class="q-hint">{{ tr("格式：{0}", { 0: q.formatHint }) }}</p>

              <ul v-if="q.options.length" class="q-options">
                <li v-for="opt in q.options" :key="opt">{{ opt }}</li>
              </ul>

              <div class="q-answer-row">
                <input
                  type="text"
                  class="ui-input sm answer-input"
                  :value="answerDrafts[q.id] ?? q.answer"
                  :placeholder="tr('填写答案…')"
                  @input="answerDrafts[q.id] = ($event.target as HTMLInputElement).value"
                  @blur="commitAnswer(q)"
                  @keydown.enter="commitAnswer(q)"
                />
                <button
                  class="ui-btn sm"
                  type="button"
                  :disabled="!(answerDrafts[q.id] ?? q.answer)"
                  @click="copyQuestionAnswer(q)"
                >
                  <Copy :size="13" />
                  <span>{{ tr("复制") }}</span>
                </button>
                <button class="ui-btn ghost sm" type="button" @click="jumpToWriteup(q.id)">
                  {{ tr("看题解") }}
                </button>
              </div>

              <!-- 关联的证据 Pin -->
              <div v-if="pinsOfQuestion(q.id).length" class="q-pins">
                <div v-for="pin in pinsOfQuestion(q.id)" :key="pin.id" class="mini-pin">
                  <div class="mini-pin-info">
                    <span class="source-tag mono">{{ pin.source }}</span>
                    <span class="mini-pin-title">{{ pin.title }}</span>
                    <span class="mini-pin-target mono">{{ formatPinTargetLabel(pin.target) }}</span>
                  </div>
                  <div class="mini-pin-actions">
                    <button
                      class="ui-btn ghost sm"
                      type="button"
                      @click="startPinCapture(pin, { navigateFirst: true, returnToContest: true })"
                    >
                      <Camera :size="12" />
                      <span>{{ pin.screenshotRel ? tr("重截") : tr("定位截图") }}</span>
                    </button>
                  </div>
                </div>
              </div>

              <!-- 笔记与 CLI 命令记录 -->
              <div class="q-notes">
                <div v-for="note in q.notes" :key="note.id" class="note-item">
                  <div class="note-head">
                    <span class="source-tag mono">{{ note.author }}</span>
                    <button
                      class="ui-icon-btn sm"
                      type="button"
                      :title="tr('删除笔记')"
                      @click="removeContestQuestionNote(q.id, note.id)"
                    >
                      <Trash2 :size="12" />
                    </button>
                  </div>
                  <pre v-if="note.command" class="note-cmd mono">{{ note.command }}</pre>
                  <p v-if="note.content" class="note-text">{{ note.content }}</p>
                </div>

                <div v-if="activeNoteQid === q.id" class="note-editor">
                  <input
                    v-model="noteCmdDraft"
                    type="text"
                    class="ui-input sm mono"
                    :placeholder="tr('命令（可选）')"
                  />
                  <div class="note-editor-row">
                    <input
                      v-model="noteContentDraft"
                      type="text"
                      class="ui-input sm"
                      :placeholder="tr('笔记或推理说明…')"
                      @keydown.enter="submitNote(q.id)"
                    />
                    <button class="ui-btn primary sm" type="button" @click="submitNote(q.id)">
                      {{ tr("保存") }}
                    </button>
                    <button class="ui-btn ghost sm" type="button" @click="activeNoteQid = null">
                      {{ tr("取消") }}
                    </button>
                  </div>
                </div>
                <button
                  v-else
                  class="note-add-trigger"
                  type="button"
                  @click="openNoteEditor(q.id)"
                >
                  {{ tr("+ 添加笔记 / 命令") }}
                </button>
              </div>
            </article>
          </div>

          <!-- 证据 Pin 列表 -->
          <div v-else class="cards-scroll">
            <div v-if="!filteredPins.length" class="pane-empty">
              <p>{{ tr("暂无证据 Pin。可在文件列表、分析结果右键「Pin 到 WP 留证」，或由 CLI 执行 pin add。") }}</p>
            </div>

            <article v-for="pin in filteredPins" :key="pin.id" class="pin-card">
              <div class="pin-head">
                <div class="pin-meta">
                  <span class="source-tag mono">{{ pin.source }}</span>
                  <span v-if="pin.questionId" class="ui-badge sm">{{ pin.questionId }}</span>
                  <strong class="pin-title">{{ pin.title }}</strong>
                </div>
                <div class="pin-actions">
                  <button
                    class="ui-btn sm"
                    type="button"
                    @click="startPinCapture(pin, { navigateFirst: pin.target.kind !== 'custom', returnToContest: true })"
                  >
                    <Camera :size="13" />
                    <span>{{ pin.screenshotRel ? tr("重截") : tr("定位并截图") }}</span>
                  </button>
                  <button class="ui-icon-btn sm" type="button" :title="tr('删除 Pin')" @click="removeContestPin(pin.id)">
                    <Trash2 :size="13" />
                  </button>
                </div>
              </div>

              <p class="pin-target mono">{{ formatPinTargetLabel(pin.target) }}</p>
              <p v-if="pin.summary" class="pin-summary">{{ pin.summary }}</p>
              <pre v-if="pin.command" class="note-cmd mono">{{ pin.command }}</pre>

              <div v-if="pin.screenshotRel && assetUrlCache[pin.screenshotRel]" class="pin-thumb-wrap">
                <img :src="assetUrlCache[pin.screenshotRel]" :alt="pin.title" class="pin-thumb" />
              </div>
            </article>
          </div>
        </div>

        <!-- 右栏：连续 Writeup Markdown 预览与编辑 -->
        <div class="right-pane">
          <div class="pane-toolbar">
            <div class="subtab-group">
              <button
                type="button"
                class="subtab-btn"
                :class="{ active: wpMode === 'preview' }"
                @click="wpMode = 'preview'"
              >
                {{ tr("WP 预览") }}
              </button>
              <button
                type="button"
                class="subtab-btn"
                :class="{ active: wpMode === 'edit' }"
                @click="wpMode = 'edit'"
              >
                {{ tr("编辑题解") }}
              </button>
              <button
                type="button"
                class="subtab-btn"
                :class="{ active: wpMode === 'source' }"
                @click="wpMode = 'source'"
              >
                {{ tr("Markdown 源码") }}
              </button>
            </div>

            <div class="wp-tools">
              <select
                v-if="workspace?.questions.length"
                class="ui-select sm toc-select"
                :value="selectedQuestionId"
                @change="onTocChange(($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ tr("跳转到题目…") }}</option>
                <option v-for="q in workspace.questions" :key="q.id" :value="q.id">
                  {{ q.id }} · {{ q.stem.slice(0, 26) }}
                </option>
              </select>
              <button class="ui-btn ghost sm" type="button" @click="copyWpSource">
                <Copy :size="13" />
                <span>{{ tr("复制 MD") }}</span>
              </button>
            </div>
          </div>

          <div v-if="wpMode === 'preview'" ref="wpPreviewRef" class="wp-preview-scroll">
            <template v-for="(block, idx) in writeupBlocks" :key="idx">
              <div v-if="block.kind === 'anchor'" :id="block.id" class="wp-anchor" />
              <component :is="`h${block.level}`" v-else-if="block.kind === 'heading'" :class="`wp-h${block.level}`">
                {{ block.text }}
              </component>
              <pre v-else-if="block.kind === 'code'" class="wp-code mono"><code>{{ block.code }}</code></pre>
              <blockquote v-else-if="block.kind === 'quote'" class="wp-quote">{{ block.text }}</blockquote>
              <figure v-else-if="block.kind === 'image'" class="wp-figure">
                <img
                  v-if="assetUrlCache[block.rel]"
                  :src="assetUrlCache[block.rel]"
                  :alt="block.alt"
                  class="wp-img"
                />
                <div v-else class="wp-img-missing mono">{{ block.rel }}</div>
                <figcaption v-if="block.alt">{{ block.alt }}</figcaption>
              </figure>
              <ul v-else-if="block.kind === 'list'" class="wp-list">
                <li v-for="(item, i) in block.items" :key="i">{{ item }}</li>
              </ul>
              <hr v-else-if="block.kind === 'hr'" class="wp-hr" />
              <p v-else class="wp-p">{{ block.text }}</p>
            </template>
          </div>

          <div v-else-if="wpMode === 'edit'" class="wp-edit-scroll">
            <div class="edit-block">
              <label class="edit-label">{{ tr("案件总体 WP 概述（Markdown）") }}</label>
              <textarea v-model="caseWpDraft" class="ui-textarea mono" rows="4" />
            </div>

            <div v-if="activeScopeExhibitId !== 'all'" class="edit-block">
              <label class="edit-label">{{ tr("当前检材分析补充：{0}", { 0: activeScopeExhibitId }) }}</label>
              <textarea v-model="exhibitWpDraft" class="ui-textarea mono" rows="4" />
            </div>

            <div v-if="selectedQuestion" class="edit-block">
              <label class="edit-label">
                {{ tr("题目详细解析：{0} — {1}", { 0: selectedQuestion.id, 1: selectedQuestion.stem }) }}
              </label>
              <textarea v-model="questionWpDraft" class="ui-textarea mono" rows="8" />
            </div>

            <div class="edit-footer">
              <button class="ui-btn primary sm" type="button" @click="saveEditedWp">
                {{ tr("保存 WP 内容") }}
              </button>
            </div>
          </div>

          <div v-else class="wp-source-scroll">
            <pre class="wp-source mono">{{ wpMarkdown }}</pre>
          </div>
        </div>
      </div>
    </template>

    <ContestImportModal :visible="importModalOpen" @close="importModalOpen = false" />
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue';
import { save } from '@tauri-apps/plugin-dialog';
import { Camera, Copy, Download, ExternalLink, FileUp, Plus, RefreshCw, Trash2 } from 'lucide-vue-next';
import { useCaseSession } from '../../cases/api';
import { tr } from '../../../composables/useLocale';
import { copyText } from '../../../composables/clipboard';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ContestQuestion, EvidencePin, QuestionStatus } from '../../../lib/ipc/contest';
import { filterQuestions, formatPinTargetLabel, parseWriteupBlocks, summarizeWorkspace } from '../model/contest';
import { useContestWorkspace } from '../composables/useContestWorkspace';
import ContestImportModal from './ContestImportModal.vue';

defineProps<{ hasLoadedImage?: boolean }>();
defineEmits<{ 'open-case-manager': []; 'back-to-evidence': [] }>();

const { activeCase } = useCaseSession();
const {
  workspace,
  wpMarkdown,
  loading,
  assetUrlCache,
  activeScopeExhibitId,
  refreshWorkspace,
  refreshWpPreviewOnly,
  bindContestExhibit,
  upsertContestQuestion,
  addContestQuestionNote,
  removeContestQuestionNote,
  removeContestPin,
  saveWpMarkdown,
  exportWriteupPackage,
  openQuickPin,
  openExhibitInWorkspace,
  startPinCapture,
} = useContestWorkspace();

const importModalOpen = ref(false);
const leftTab = ref<'questions' | 'pins'>('questions');
const wpMode = ref<'preview' | 'edit' | 'source'>('preview');
const keyword = ref('');
const statusFilter = ref<QuestionStatus | 'all'>('all');
const selectedQuestionId = ref('');
const answerDrafts = reactive<Record<string, string>>({});

const addQuestionOpen = ref(false);
const newStem = ref('');
const newExhibitId = ref('');

const activeNoteQid = ref<string | null>(null);
const noteCmdDraft = ref('');
const noteContentDraft = ref('');

const caseWpDraft = ref('');
const exhibitWpDraft = ref('');
const questionWpDraft = ref('');
const wpPreviewRef = ref<HTMLElement | null>(null);

const summary = computed(() => summarizeWorkspace(workspace.value));

const filteredQuestions = computed(() =>
  filterQuestions(workspace.value, {
    exhibitId: activeScopeExhibitId.value,
    status: statusFilter.value,
    keyword: keyword.value,
  }),
);

const filteredPins = computed(() => {
  const pins = workspace.value?.pins ?? [];
  const kw = keyword.value.trim().toLowerCase();
  return pins.filter(p => {
    if (activeScopeExhibitId.value !== 'all' && p.exhibitId !== activeScopeExhibitId.value) {
      return false;
    }
    if (!kw) return true;
    return `${p.title} ${p.summary} ${p.questionId ?? ''} ${p.command ?? ''}`.toLowerCase().includes(kw);
  });
});

const selectedQuestion = computed(
  () => workspace.value?.questions.find(q => q.id === selectedQuestionId.value) ?? null,
);

const writeupBlocks = computed(() => parseWriteupBlocks(wpMarkdown.value));

watch(
  workspace,
  ws => {
    if (!ws) return;
    caseWpDraft.value = ws.caseWpMd;
    if (!selectedQuestionId.value && ws.questions.length > 0) {
      selectedQuestionId.value = ws.questions[0].id;
    }
    syncScopedDrafts();
  },
  { immediate: true },
);

watch([activeScopeExhibitId, selectedQuestionId], () => {
  syncScopedDrafts();
});

function syncScopedDrafts() {
  const ws = workspace.value;
  if (!ws) return;
  const ex = ws.exhibits.find(e => e.id === activeScopeExhibitId.value);
  exhibitWpDraft.value = ex?.wpMd ?? '';
  const q = ws.questions.find(item => item.id === selectedQuestionId.value);
  questionWpDraft.value = q?.wpSectionMd ?? '';
}

function evidenceShortName(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

function exhibitNameOf(id: string): string {
  return workspace.value?.exhibits.find(e => e.id === id)?.name || id;
}

function pinsOfQuestion(qid: string): EvidencePin[] {
  return (workspace.value?.pins ?? []).filter(p => p.questionId === qid);
}

async function selectExhibitScope(id: string) {
  activeScopeExhibitId.value = id;
  await refreshWpPreviewOnly(id);
}

async function onBindExhibit(exhibitId: string, imageId: string) {
  try {
    await bindContestExhibit(exhibitId, imageId || null);
  } catch (err) {
    notifyError(tr('绑定检材失败'), String(err));
  }
}

async function updateQuestionField(
  q: ContestQuestion,
  patch: { answer?: string; status?: QuestionStatus },
) {
  try {
    await upsertContestQuestion({
      id: q.id,
      answer: patch.answer,
      status: patch.status,
    });
  } catch (err) {
    notifyError(tr('更新题目失败'), String(err));
  }
}

async function commitAnswer(q: ContestQuestion) {
  const nextVal = answerDrafts[q.id];
  if (nextVal === undefined || nextVal === q.answer) return;
  await updateQuestionField(q, { answer: nextVal });
}

async function copyQuestionAnswer(q: ContestQuestion) {
  const text = answerDrafts[q.id] ?? q.answer;
  if (!text) return;
  await copyText(text, tr('题目 {0} 答案', { 0: q.id }));
}

async function createManualQuestion() {
  if (!newStem.value.trim()) return;
  try {
    const q = await upsertContestQuestion({
      stem: newStem.value.trim(),
      exhibitId: newExhibitId.value || (activeScopeExhibitId.value !== 'all' ? activeScopeExhibitId.value : undefined),
    });
    newStem.value = '';
    addQuestionOpen.value = false;
    selectedQuestionId.value = q.id;
  } catch (err) {
    notifyError(tr('添加题目失败'), String(err));
  }
}

function openNoteEditor(qid: string) {
  activeNoteQid.value = qid;
  noteCmdDraft.value = '';
  noteContentDraft.value = '';
}

async function submitNote(qid: string) {
  if (!noteContentDraft.value.trim() && !noteCmdDraft.value.trim()) return;
  try {
    await addContestQuestionNote({
      questionId: qid,
      author: 'user',
      kind: noteCmdDraft.value.trim() ? 'command' : 'note',
      command: noteCmdDraft.value.trim() || undefined,
      content: noteContentDraft.value.trim(),
    });
    activeNoteQid.value = null;
  } catch (err) {
    notifyError(tr('保存笔记失败'), String(err));
  }
}

function openNewFreePin() {
  openQuickPin({
    title: '',
    summary: '',
    target: { kind: 'custom', detail: tr('手动取证记录') },
    exhibitId: activeScopeExhibitId.value !== 'all' ? activeScopeExhibitId.value : null,
  });
}

async function jumpToWriteup(qid: string) {
  selectedQuestionId.value = qid;
  wpMode.value = 'preview';
  await nextTick();
  const anchor = wpPreviewRef.value?.querySelector(`#wp-${CSS.escape(qid)}`);
  anchor?.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

function onTocChange(qid: string) {
  if (!qid) return;
  void jumpToWriteup(qid);
}

async function saveEditedWp() {
  try {
    await saveWpMarkdown({
      caseWpMd: caseWpDraft.value,
      exhibitId: activeScopeExhibitId.value !== 'all' ? activeScopeExhibitId.value : undefined,
      exhibitWpMd: activeScopeExhibitId.value !== 'all' ? exhibitWpDraft.value : undefined,
      questionId: selectedQuestionId.value || undefined,
      questionWpMd: selectedQuestionId.value ? questionWpDraft.value : undefined,
    });
    notifySuccess(tr('已保存 WP 内容'), '');
  } catch (err) {
    notifyError(tr('保存 WP 失败'), String(err));
  }
}

async function copyWpSource() {
  await copyText(wpMarkdown.value, tr('Markdown Writeup'));
}

async function handleExportWp() {
  try {
    const defaultName = `${(workspace.value?.title || 'Writeup').replace(/[\\/:*?"<>|]/g, '_')}.md`;
    const output = await save({
      title: tr('导出 Markdown Writeup（同时生成同目录 assets 图片文件夹）'),
      defaultPath: defaultName,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    });
    if (!output) return;
    const res = await exportWriteupPackage(output, activeScopeExhibitId.value);
    notifySuccess(
      tr('WP 导出成功'),
      tr('已导出 {0} 及 {1} 张截图', { 0: res.markdownPath, 1: res.exportedAssets }),
    );
  } catch (err) {
    notifyError(tr('WP 导出失败'), String(err));
  }
}
</script>

<style scoped>
.contest-workbench {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px 16px;
  gap: 10px;
  overflow: hidden;
  background: var(--bg-color);
  color: var(--text-color);
}

.contest-empty {
  margin: auto;
  text-align: center;
  max-width: 420px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}

.contest-empty p {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary);
}

.workbench-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.header-left {
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
}

.ws-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.ws-stats {
  font-size: 12px;
  color: var(--text-secondary);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.exhibit-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  overflow-x: auto;
  padding-bottom: 2px;
}

.exhibit-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: 8px;
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.2));
  background: var(--card-bg, rgba(128, 128, 128, 0.04));
  flex-shrink: 0;
}

.exhibit-chip.active {
  border-color: var(--primary-color);
}

.exhibit-name {
  border: none;
  background: transparent;
  color: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  padding: 0;
}

.exhibit-bind-select {
  font-size: 11px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  max-width: 150px;
}

.split-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(380px, 1.05fr) minmax(380px, 1fr);
  gap: 12px;
}

.left-pane,
.right-pane {
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.2));
  border-radius: 10px;
  background: var(--card-bg, rgba(128, 128, 128, 0.03));
  overflow: hidden;
}

.pane-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color, rgba(128, 128, 128, 0.16));
}

.subtab-group {
  display: inline-flex;
  gap: 4px;
}

.subtab-btn {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 500;
  padding: 5px 10px;
  border-radius: 6px;
  cursor: pointer;
}

.subtab-btn.active {
  background: var(--primary-color);
  color: #ffffff;
}

.filter-row,
.wp-tools {
  display: flex;
  align-items: center;
  gap: 6px;
}

.search-input {
  width: 145px;
}

.toc-select {
  max-width: 210px;
}

.quick-add-box {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color, rgba(128, 128, 128, 0.16));
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.quick-add-row {
  display: flex;
  gap: 6px;
}

.cards-scroll,
.wp-preview-scroll,
.wp-edit-scroll,
.wp-source-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px;
}

.cards-scroll {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.pane-empty {
  text-align: center;
  padding: 36px 20px;
  font-size: 13px;
  color: var(--text-secondary);
}

.question-card,
.pin-card {
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.2));
  border-radius: 8px;
  padding: 10px 12px;
  background: var(--bg-elevated, var(--bg-color));
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.question-card.status-answered {
  border-left: 3px solid #10b981;
}

.question-card.status-review {
  border-left: 3px solid #f59e0b;
}

.question-card.status-stuck {
  border-left: 3px solid #ef4444;
}

.q-head,
.pin-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.q-meta,
.pin-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.q-id {
  font-weight: 700;
  font-size: 12px;
}

.q-exhibit {
  font-size: 11px;
  color: var(--text-secondary);
}

.status-select {
  font-size: 11px;
  border-radius: 6px;
  padding: 2px 6px;
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.24));
  background: transparent;
  color: inherit;
}

.q-stem {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.q-hint {
  margin: 0;
  font-size: 11px;
  color: var(--text-secondary);
}

.q-options {
  margin: 0;
  padding-left: 16px;
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.q-answer-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.answer-input {
  flex: 1;
}

.q-pins {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-top: 4px;
  border-top: 1px dashed var(--border-color, rgba(128, 128, 128, 0.2));
}

.mini-pin {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
}

.mini-pin-info {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  flex: 1;
}

.mini-pin-title {
  font-weight: 500;
}

.mini-pin-target,
.pin-target {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin: 0;
}

.source-tag {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(59, 130, 246, 0.12);
  color: var(--primary-color);
  text-transform: uppercase;
}

.q-notes {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.note-item {
  font-size: 12px;
  padding: 6px 8px;
  border-radius: 6px;
  background: rgba(128, 128, 128, 0.06);
}

.note-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 4px;
}

.note-cmd {
  margin: 0 0 4px;
  padding: 4px 6px;
  border-radius: 4px;
  background: rgba(15, 23, 42, 0.08);
  font-size: 11px;
  overflow-x: auto;
}

.note-text,
.pin-summary {
  margin: 0;
  font-size: 12px;
  line-height: 1.45;
}

.note-editor {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.note-editor-row {
  display: flex;
  gap: 6px;
}

.note-add-trigger {
  align-self: flex-start;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
  padding: 2px 0;
}

.pin-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.pin-thumb-wrap {
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid var(--border-color, rgba(128, 128, 128, 0.2));
}

.pin-thumb,
.wp-img {
  display: block;
  max-width: 100%;
  height: auto;
}

.wp-anchor {
  height: 1px;
}

.wp-h1 { font-size: 18px; margin: 4px 0 12px; }
.wp-h2 { font-size: 16px; margin: 16px 0 8px; padding-bottom: 4px; border-bottom: 1px solid var(--border-color, rgba(128, 128, 128, 0.2)); }
.wp-h3 { font-size: 14px; margin: 12px 0 6px; }
.wp-h4 { font-size: 13px; margin: 10px 0 4px; }
.wp-p { font-size: 13px; line-height: 1.6; margin: 4px 0 8px; }
.wp-quote {
  margin: 6px 0 10px;
  padding: 6px 10px;
  border-left: 3px solid var(--primary-color);
  background: rgba(128, 128, 128, 0.06);
  font-size: 12px;
}
.wp-code {
  margin: 6px 0 10px;
  padding: 8px 10px;
  border-radius: 6px;
  background: rgba(15, 23, 42, 0.08);
  font-size: 12px;
  overflow-x: auto;
}
.wp-list {
  margin: 4px 0 8px;
  padding-left: 18px;
  font-size: 13px;
}
.wp-figure {
  margin: 8px 0 12px;
}
.wp-figure figcaption {
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  text-align: center;
}
.wp-img-missing {
  padding: 12px;
  border: 1px dashed var(--border-color);
  font-size: 12px;
  color: var(--text-secondary);
}
.wp-source {
  margin: 0;
  white-space: pre-wrap;
  font-size: 12px;
  line-height: 1.55;
}
.wp-edit-scroll {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.edit-block {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.edit-label {
  font-size: 12px;
  font-weight: 600;
}
.edit-footer {
  display: flex;
  justify-content: flex-end;
}
</style>
