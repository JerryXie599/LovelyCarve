<template>
  <div class="git-code-browser">
    <!-- Top Controls Bar -->
    <div class="gcb-toolbar">
      <div class="gcb-rev-wrap">
        <GitBranch :size="14" class="gcb-rev-icon" />
        <select
          :value="currentRev"
          class="ui-select sm gcb-rev-select"
          :aria-label="tr('切换版本或分支')"
          @change="onRevSelect(($event.target as HTMLSelectElement).value)"
        >
          <option value="WORKTREE">{{ tr("工作目录 (WORKTREE)") }}</option>
          <option value="INDEX">{{ tr("暂存区 (INDEX)") }}</option>
          <optgroup v-if="localBranches.length" :label="tr('本地分支')">
            <option v-for="b in localBranches" :key="b.fullRef" :value="b.name">
              {{ b.name }}{{ b.isHead ? ' (HEAD)' : '' }}
            </option>
          </optgroup>
          <optgroup v-if="remoteBranches.length" :label="tr('远程分支')">
            <option v-for="b in remoteBranches" :key="b.fullRef" :value="b.fullRef">
              {{ b.name }}
            </option>
          </optgroup>
          <optgroup v-if="overview.tags.length" :label="tr('标签')">
            <option v-for="t in overview.tags" :key="t.name" :value="t.name">
              {{ t.name }}
            </option>
          </optgroup>
          <optgroup v-if="rolledBackCommits.length" :label="tr('已回滚 / 悬空提交')">
            <option v-for="c in rolledBackCommits" :key="c.sha" :value="c.sha">
              [{{ c.shortSha }}] {{ c.summary }}
            </option>
          </optgroup>
          <option v-if="customRevSelected" :value="currentRev">
            {{ tr("提交 {0}", { 0: shortSha(currentRev) }) }}
          </option>
        </select>
      </div>

      <!-- Breadcrumbs -->
      <nav class="gcb-breadcrumbs" :aria-label="tr('目录路径')">
        <button
          type="button"
          class="ui-btn link sm gcb-crumb"
          :class="{ current: !currentDir && !openedFile }"
          @click="navigateDir('')"
        >
          {{ overview.repoName }}
        </button>
        <template v-for="(seg, idx) in dirSegments" :key="seg.path">
          <span class="gcb-sep">/</span>
          <button
            type="button"
            class="ui-btn link sm gcb-crumb"
            :class="{ current: idx === dirSegments.length - 1 && !openedFile }"
            @click="navigateDir(seg.path)"
          >
            {{ seg.name }}
          </button>
        </template>
        <template v-if="openedFile">
          <span class="gcb-sep">/</span>
          <span class="gcb-crumb-file mono">{{ openedFile.name }}</span>
        </template>
      </nav>

      <div class="gcb-actions">
        <label v-if="!openedFile" class="ui-search gcb-filter">
          <Search :size="13" />
          <input v-model="filterText" :placeholder="tr('过滤文件名…')" />
          <button v-if="filterText" type="button" class="ui-icon-btn sm" @click="filterText = ''">
            <X :size="11" />
          </button>
        </label>

        <button
          v-if="!openedFile"
          type="button"
          class="ui-btn sm"
          :title="tr('在交互沙盒中新建文件')"
          @click="openInlineCreate('file')"
        >
          <FilePlus2 :size="13" />
          <span>{{ tr("新建文件") }}</span>
        </button>

        <button
          v-if="!openedFile"
          type="button"
          class="ui-btn sm"
          :title="tr('在交互沙盒中新建文件夹')"
          @click="openInlineCreate('folder')"
        >
          <FolderPlus :size="13" />
          <span>{{ tr("新建文件夹") }}</span>
        </button>

        <button
          v-if="isCommitRev"
          type="button"
          class="ui-btn sm"
          @click="emit('inspectCommitDiff', currentRev)"
        >
          <GitCommitHorizontal :size="13" />
          <span>{{ tr("查看提交 Diff") }}</span>
        </button>

        <button
          type="button"
          class="ui-btn sm"
          :disabled="exporting"
          :title="tr('导出当前目录或版本快照')"
          @click="handleExport(currentDir || undefined)"
        >
          <Loader2 v-if="exporting" :size="13" class="spin" />
          <Download v-else :size="13" />
          <span>{{ tr("导出快照") }}</span>
        </button>
      </div>
    </div>

    <!-- Inline Create / Rename Prompt Bar -->
    <form v-if="inlinePrompt" class="gcb-inline-form" @submit.prevent="submitInlinePrompt">
      <span class="gcb-inline-label">{{ inlinePromptTitle }}</span>
      <input
        ref="inlineInputRef"
        v-model="inlinePrompt.value"
        class="ui-input sm mono gcb-inline-input"
        :placeholder="inlinePromptPlaceholder"
      />
      <label v-if="inlinePrompt.kind === 'export'" class="gcb-check">
        <input v-model="includeGitOnExport" type="checkbox" />
        <span>{{ tr("包含 .git 目录") }}</span>
      </label>
      <button type="submit" class="ui-btn sm primary" :disabled="mutating">
        <Loader2 v-if="mutating" :size="12" class="spin" />
        <span>{{ tr("确定") }}</span>
      </button>
      <button type="button" class="ui-btn sm ghost" @click="inlinePrompt = null">
        {{ tr("取消") }}
      </button>
    </form>

    <!-- Main Surface: File Viewer/Editor OR Directory Listing -->
    <div v-if="fileLoading" class="gcb-state">
      <Loader2 :size="18" class="spin" />
      <span>{{ tr("正在读取文件内容…") }}</span>
    </div>

    <div v-else-if="openedFile" class="gcb-file-card ui-card">
      <header class="gcb-file-head">
        <button type="button" class="ui-btn sm ghost" @click="closeFile">
          <ArrowLeft :size="13" />
          <span>{{ tr("返回目录") }}</span>
        </button>
        <span class="gcb-file-path mono">{{ openedFile.path }}</span>
        <span class="ui-badge sm">{{ inferCodeLang(openedFile.name) }}</span>
        <span class="ui-badge sm mono">{{ formatBytes(openedFile.size) }}</span>
        <span v-if="openedFile.encoding" class="ui-badge sm mono">{{ openedFile.encoding }}</span>
        <span v-if="openedFile.blobSha" class="ui-badge sm mono">{{ shortSha(openedFile.blobSha) }}</span>

        <div class="gcb-file-btns">
          <button
            v-if="openedFile.kind === 'text' && isMarkdownFilename(openedFile.name) && !editing"
            type="button"
            class="ui-btn sm"
            :aria-pressed="previewMarkdown"
            @click="previewMarkdown = !previewMarkdown"
          >
            {{ previewMarkdown ? tr("源码") : tr("预览") }}
          </button>

          <button
            v-if="openedFile.kind === 'text' && !editing"
            type="button"
            class="ui-btn sm"
            @click="startEditing"
          >
            <Pencil :size="13" />
            <span>{{ tr("编辑文件") }}</span>
          </button>

          <template v-if="editing">
            <button type="button" class="ui-btn sm primary" :disabled="mutating" @click="saveEditedFile">
              <Loader2 v-if="mutating" :size="12" class="spin" />
              <Save v-else :size="12" />
              <span>{{ tr("保存到沙盒") }}</span>
            </button>
            <button type="button" class="ui-btn sm ghost" @click="editing = false">
              {{ tr("取消") }}
            </button>
          </template>

          <button
            v-if="openedFile.text"
            type="button"
            class="ui-btn sm"
            @click="copyFileText"
          >
            <Copy :size="12" />
            <span>{{ tr("复制") }}</span>
          </button>

          <button
            type="button"
            class="ui-btn sm"
            :disabled="exporting"
            @click="handleExport(openedFile.path)"
          >
            <Download :size="12" />
            <span>{{ tr("导出") }}</span>
          </button>
        </div>
      </header>

      <div v-if="openedFile.truncated" class="ui-alert warn gcb-trunc-note">
        {{ tr("文件超过 2 MiB，仅展示前 2 MiB 内容，可导出查看完整文件。") }}
      </div>

      <div v-if="editing" class="gcb-editor-wrap">
        <textarea v-model="editBuffer" class="gcb-editor mono" spellcheck="false" />
      </div>

      <div v-else-if="openedFile.kind === 'image' && openedFile.imageDataUrl" class="gcb-image-wrap">
        <img :src="openedFile.imageDataUrl" :alt="openedFile.name" class="gcb-image" />
      </div>

      <div v-else-if="openedFile.kind === 'binary'" class="gcb-hex-wrap">
        <pre class="gcb-hex mono">{{ openedFile.hexPreview }}</pre>
      </div>

      <div
        v-else-if="previewMarkdown && isMarkdownFilename(openedFile.name) && openedFile.text"
        class="gcb-markdown"
        v-html="renderMarkdownSafe(openedFile.text)"
      />

      <div v-else class="gcb-code-scroll">
        <table class="gcb-code-table mono">
          <tbody>
            <tr v-for="(line, idx) in fileLines" :key="idx">
              <td class="gcb-ln">{{ idx + 1 }}</td>
              <td class="gcb-code">{{ line || ' ' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <template v-else>
      <div v-if="treeLoading" class="gcb-state">
        <Loader2 :size="18" class="spin" />
        <span>{{ tr("正在加载目录树…") }}</span>
      </div>

      <div v-else-if="treeError" class="ui-alert danger">
        <span>{{ treeError }}</span>
      </div>

      <div v-else class="gcb-tree-card ui-card">
        <div class="gcb-tree-meta">
          <span class="gcb-meta-rev mono">
            {{ currentRev }}
            <template v-if="tree?.resolvedSha">· {{ shortSha(tree.resolvedSha) }}</template>
          </span>
          <span class="gcb-meta-count">{{ tr("{0} 项", { 0: filteredEntries.length }) }}</span>
        </div>

        <div v-if="filteredEntries.length === 0" class="gcb-empty-dir">
          {{ filterText ? tr("没有匹配的文件或文件夹") : tr("当前目录为空") }}
        </div>

        <table v-else class="gcb-table">
          <tbody>
            <tr v-if="currentDir" class="gcb-row" @click="navigateParent">
              <td class="gcb-col-name">
                <div class="gcb-name-cell">
                  <Folder :size="15" class="gcb-entry-icon dir" />
                  <span class="gcb-entry-link">..</span>
                </div>
              </td>
              <td class="gcb-col-status" />
              <td class="gcb-col-mode" />
              <td class="gcb-col-sha" />
              <td class="gcb-col-size" />
              <td class="gcb-col-ops" />
            </tr>
            <tr
              v-for="entry in filteredEntries"
              :key="entry.path"
              class="gcb-row"
              @click="openEntry(entry)"
            >
              <td class="gcb-col-name">
                <div class="gcb-name-cell">
                  <Folder v-if="entry.kind === 'dir'" :size="15" class="gcb-entry-icon dir" />
                  <FileCode v-else :size="15" class="gcb-entry-icon file" />
                  <span class="gcb-entry-link" :title="entry.name">{{ entry.name }}</span>
                </div>
              </td>
              <td class="gcb-col-status">
                <span v-if="entry.status" class="ui-badge sm" :class="statusTone(entry.status)">
                  {{ entry.status }}
                </span>
              </td>
              <td class="gcb-col-mode mono">{{ entry.mode }}</td>
              <td class="gcb-col-sha mono">{{ entry.sha ? shortSha(entry.sha, 7) : '' }}</td>
              <td class="gcb-col-size mono">
                {{ entry.size !== null ? formatBytes(entry.size) : '' }}
              </td>
              <td class="gcb-col-ops" @click.stop>
                <div class="gcb-row-ops">
                  <button
                    type="button"
                    class="ui-icon-btn sm"
                    :title="tr('重命名')"
                    @click="openInlineRename(entry)"
                  >
                    <Pencil :size="12" />
                  </button>
                  <button
                    type="button"
                    class="ui-icon-btn sm"
                    :title="tr('导出')"
                    @click="handleExport(entry.path)"
                  >
                    <Download :size="12" />
                  </button>
                  <button
                    type="button"
                    class="ui-icon-btn sm danger-soft"
                    :title="tr('删除')"
                    @click="handleDeleteEntry(entry)"
                  >
                    <Trash2 :size="12" />
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- README Preview Card -->
      <section v-if="tree?.readme" class="gcb-readme-card ui-card">
        <header class="gcb-readme-head">
          <BookOpen :size="14" />
          <span class="mono">{{ tree.readme.name }}</span>
        </header>
        <div
          v-if="isMarkdownFilename(tree.readme.name)"
          class="gcb-markdown"
          v-html="renderMarkdownSafe(tree.readme.content)"
        />
        <pre v-else class="gcb-readme-plain mono">{{ tree.readme.content }}</pre>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  ArrowLeft,
  BookOpen,
  Copy,
  Download,
  FileCode,
  FilePlus2,
  Folder,
  FolderPlus,
  GitBranch,
  GitCommitHorizontal,
  Loader2,
  Pencil,
  Save,
  Search,
  Trash2,
  X,
} from 'lucide-vue-next';
import { copyText } from '../../../composables/clipboard';
import { confirm } from '../../../composables/useConfirm';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceExport,
  gitWorkspaceFileRead,
  gitWorkspaceSandboxFs,
  gitWorkspaceTree,
  type GitFileContent,
  type GitRepoOverview,
  type GitTreeEntry,
  type GitTreeListing,
} from '../../../lib/ipc/gitWorkspace';
import { formatBytes } from '../../../utils/format';
import { escapeHtml, sanitizeHtml, type SafeHtmlString } from '../../../utils/safeHtml';
import { inferCodeLang, isMarkdownFilename, shortSha } from '../model/pluginTools';

const props = defineProps<{
  imageId: ImageId;
  overview: GitRepoOverview;
  useSandbox: boolean;
  rev: string;
}>();

const emit = defineEmits<{
  'update:rev': [rev: string];
  sandboxMutated: [];
  inspectCommitDiff: [sha: string];
}>();

const currentRev = computed(() => props.rev || 'WORKTREE');
const currentDir = ref('');
const filterText = ref('');
const tree = shallowRef<GitTreeListing | null>(null);
const treeLoading = ref(false);
const treeError = ref<string | null>(null);

const openedFile = shallowRef<GitFileContent | null>(null);
const fileLoading = ref(false);
const editing = ref(false);
const editBuffer = ref('');
const previewMarkdown = ref(true);
const mutating = ref(false);
const exporting = ref(false);
const includeGitOnExport = ref(false);

interface InlinePrompt {
  kind: 'file' | 'folder' | 'rename' | 'export';
  value: string;
  targetPath?: string;
}

const inlinePrompt = ref<InlinePrompt | null>(null);
const inlineInputRef = ref<HTMLInputElement | null>(null);

const localBranches = computed(() => props.overview.branches.filter(b => !b.isRemote));
const remoteBranches = computed(() => props.overview.branches.filter(b => b.isRemote));
const rolledBackCommits = computed(() =>
  props.overview.commits.filter(c => c.status === 'rolled_back' || c.status === 'dangling').slice(0, 30),
);

const isCommitRev = computed(() => {
  const r = currentRev.value;
  return r !== 'WORKTREE' && r !== 'INDEX';
});

const customRevSelected = computed(() => {
  const r = currentRev.value;
  if (r === 'WORKTREE' || r === 'INDEX') return false;
  if (props.overview.branches.some(b => b.name === r || b.fullRef === r)) return false;
  if (props.overview.tags.some(t => t.name === r)) return false;
  if (rolledBackCommits.value.some(c => c.sha === r)) return false;
  return true;
});

const dirSegments = computed(() => {
  if (!currentDir.value) return [];
  const parts = currentDir.value.split('/').filter(Boolean);
  return parts.map((name, i) => ({
    name,
    path: parts.slice(0, i + 1).join('/'),
  }));
});

const filteredEntries = computed(() => {
  const list = tree.value?.entries ?? [];
  const q = filterText.value.trim().toLowerCase();
  if (!q) return list;
  return list.filter(e => e.name.toLowerCase().includes(q));
});

const fileLines = computed(() => {
  const text = openedFile.value?.text ?? '';
  return text.split('\n');
});

const inlinePromptTitle = computed(() => {
  switch (inlinePrompt.value?.kind) {
    case 'file': return tr("新建文件路径");
    case 'folder': return tr("新建文件夹路径");
    case 'rename': return tr("重命名 / 移动至");
    default: return '';
  }
});

const inlinePromptPlaceholder = computed(() => {
  switch (inlinePrompt.value?.kind) {
    case 'file': return tr("例如：notes/todo.md");
    case 'folder': return tr("例如：docs");
    case 'rename': return tr("输入新的相对路径");
    default: return '';
  }
});

let treeReq = 0;

async function loadTree() {
  const seq = ++treeReq;
  treeLoading.value = true;
  treeError.value = null;
  try {
    const res = await gitWorkspaceTree({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      rev: currentRev.value,
      dirPath: currentDir.value,
      useSandbox: props.useSandbox,
    });
    if (seq !== treeReq) return;
    tree.value = res;
  } catch (err) {
    if (seq === treeReq) treeError.value = String(err);
  } finally {
    if (seq === treeReq) treeLoading.value = false;
  }
}

watch(
  [() => props.overview.repoPath, () => props.useSandbox, currentRev, currentDir],
  () => {
    void loadTree();
  },
  { immediate: true },
);

watch(
  () => props.overview.repoPath,
  () => {
    currentDir.value = '';
    openedFile.value = null;
    editing.value = false;
  },
);

function onRevSelect(nextRev: string) {
  openedFile.value = null;
  editing.value = false;
  emit('update:rev', nextRev);
}

function navigateDir(path: string) {
  openedFile.value = null;
  editing.value = false;
  currentDir.value = path;
}

function navigateParent() {
  const idx = currentDir.value.lastIndexOf('/');
  navigateDir(idx === -1 ? '' : currentDir.value.slice(0, idx));
}

async function openEntry(entry: GitTreeEntry) {
  if (entry.kind === 'dir') {
    currentDir.value = entry.path;
    return;
  }
  fileLoading.value = true;
  editing.value = false;
  previewMarkdown.value = true;
  try {
    openedFile.value = await gitWorkspaceFileRead({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      rev: currentRev.value,
      filePath: entry.path,
      blobSha: entry.sha,
      useSandbox: props.useSandbox,
    });
  } catch (err) {
    notifyError(tr("读取文件失败"), String(err));
  } finally {
    fileLoading.value = false;
  }
}

function closeFile() {
  openedFile.value = null;
  editing.value = false;
}

function startEditing() {
  editBuffer.value = openedFile.value?.text ?? '';
  editing.value = true;
}

async function saveEditedFile() {
  if (!openedFile.value) return;
  mutating.value = true;
  try {
    await gitWorkspaceSandboxFs({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'write_file',
      targetPath: openedFile.value.path,
      content: editBuffer.value,
    });
    editing.value = false;
    notifySuccess(tr("已保存修改"), openedFile.value.path);
    emit('sandboxMutated');
    openedFile.value = await gitWorkspaceFileRead({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      rev: 'WORKTREE',
      filePath: openedFile.value.path,
      useSandbox: true,
    });
  } catch (err) {
    notifyError(tr("保存文件失败"), String(err));
  } finally {
    mutating.value = false;
  }
}

function openInlineCreate(kind: 'file' | 'folder') {
  const prefix = currentDir.value ? `${currentDir.value}/` : '';
  inlinePrompt.value = { kind, value: prefix };
  void nextTick(() => inlineInputRef.value?.focus());
}

function openInlineRename(entry: GitTreeEntry) {
  inlinePrompt.value = { kind: 'rename', value: entry.path, targetPath: entry.path };
  void nextTick(() => inlineInputRef.value?.focus());
}

async function submitInlinePrompt() {
  const prompt = inlinePrompt.value;
  if (!prompt) return;
  const rel = prompt.value.trim().replace(/^\/+|\/+$/g, '');
  if (!rel) return;

  mutating.value = true;
  try {
    if (prompt.kind === 'folder') {
      await gitWorkspaceSandboxFs({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        action: 'mkdir',
        targetPath: rel,
      });
      notifySuccess(tr("已创建文件夹"), rel);
    } else if (prompt.kind === 'file') {
      await gitWorkspaceSandboxFs({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        action: 'write_file',
        targetPath: rel,
        content: '',
      });
      notifySuccess(tr("已创建文件"), rel);
      emit('sandboxMutated');
      inlinePrompt.value = null;
      openedFile.value = await gitWorkspaceFileRead({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        rev: 'WORKTREE',
        filePath: rel,
        useSandbox: true,
      });
      editBuffer.value = '';
      editing.value = true;
      return;
    } else if (prompt.kind === 'rename' && prompt.targetPath) {
      await gitWorkspaceSandboxFs({
        imageId: props.imageId,
        repoPath: props.overview.repoPath,
        action: 'rename',
        targetPath: prompt.targetPath,
        newPath: rel,
      });
      notifySuccess(tr("已重命名"), `${prompt.targetPath} → ${rel}`);
    }
    inlinePrompt.value = null;
    emit('sandboxMutated');
    await loadTree();
  } catch (err) {
    notifyError(tr("操作失败"), String(err));
  } finally {
    mutating.value = false;
  }
}

async function handleDeleteEntry(entry: GitTreeEntry) {
  const ok = await confirm({
    title: tr("删除沙盒项目"),
    message: tr("将在交互沙盒中删除「{0}」（原始镜像不受影响），是否继续？", { 0: entry.path }),
    confirmText: tr("删除"),
    danger: true,
  });
  if (!ok) return;
  try {
    await gitWorkspaceSandboxFs({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      action: 'delete',
      targetPath: entry.path,
    });
    notifySuccess(tr("已删除"), entry.path);
    emit('sandboxMutated');
    await loadTree();
  } catch (err) {
    notifyError(tr("删除失败"), String(err));
  }
}

async function handleExport(subPath?: string) {
  const destination = await openDialog({
    directory: true,
    multiple: false,
    title: tr("选择导出目标目录"),
  });
  if (!destination || typeof destination !== 'string') return;
  exporting.value = true;
  try {
    const res = await gitWorkspaceExport({
      imageId: props.imageId,
      repoPath: props.overview.repoPath,
      rev: currentRev.value,
      subPath,
      destination,
      includeGitDir: !subPath && includeGitOnExport.value,
      useSandbox: props.useSandbox,
    });
    notifySuccess(
      tr("导出完成"),
      tr("已导出 {0} 个文件（{1}）至 {2}", {
        0: res.exportedFiles,
        1: formatBytes(res.exportedBytes),
        2: res.destination,
      }),
    );
  } catch (err) {
    notifyError(tr("导出失败"), String(err));
  } finally {
    exporting.value = false;
  }
}

async function copyFileText() {
  if (!openedFile.value?.text) return;
  await copyText(openedFile.value.text, openedFile.value.name);
}

function statusTone(status: string): string {
  if (status === 'modified') return 'warn';
  if (status === 'untracked' || status === 'added') return 'info';
  if (status === 'deleted') return 'danger';
  return '';
}

function renderMarkdownSafe(raw: string): SafeHtmlString {
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  const htmlParts: string[] = [];
  let inCode = false;
  let codeLines: string[] = [];

  for (const line of lines) {
    if (line.trim().startsWith('```')) {
      if (inCode) {
        htmlParts.push(`<pre><code>${escapeHtml(codeLines.join('\n'))}</code></pre>`);
        codeLines = [];
        inCode = false;
      } else {
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      codeLines.push(line);
      continue;
    }
    if (/^###\s+/.test(line)) htmlParts.push(`<p><strong>${escapeHtml(line.replace(/^###\s+/, ''))}</strong></p>`);
    else if (/^##\s+/.test(line)) htmlParts.push(`<p><strong>${escapeHtml(line.replace(/^##\s+/, ''))}</strong></p>`);
    else if (/^#\s+/.test(line)) htmlParts.push(`<p><strong>${escapeHtml(line.replace(/^#\s+/, ''))}</strong></p>`);
    else if (/^[-*]\s+/.test(line)) htmlParts.push(`<ul><li>${formatInlineMd(line.replace(/^[-*]\s+/, ''))}</li></ul>`);
    else if (/^>\s+/.test(line)) htmlParts.push(`<blockquote>${formatInlineMd(line.replace(/^>\s+/, ''))}</blockquote>`);
    else if (!line.trim()) htmlParts.push('<br />');
    else htmlParts.push(`<p>${formatInlineMd(line)}</p>`);
  }
  if (inCode && codeLines.length) {
    htmlParts.push(`<pre><code>${escapeHtml(codeLines.join('\n'))}</code></pre>`);
  }
  return sanitizeHtml(htmlParts.join(''));
}

function formatInlineMd(text: string): string {
  return escapeHtml(text)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>');
}
</script>

<style scoped>
.git-code-browser {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.gcb-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.gcb-rev-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
}

.gcb-rev-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}

.gcb-rev-select {
  min-width: 140px;
  max-width: 240px;
}

.gcb-breadcrumbs {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 2px;
  flex: 1;
  min-width: 140px;
}

.gcb-crumb.current {
  color: var(--text-primary);
  font-weight: 600;
}

.gcb-sep {
  color: var(--text-secondary);
  font-size: 12px;
}

.gcb-crumb-file {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  padding: 0 4px;
}

.gcb-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.gcb-filter {
  width: 160px;
}

.gcb-inline-form {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-secondary);
}

.gcb-inline-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
}

.gcb-inline-input {
  flex: 1;
  min-width: 180px;
}

.gcb-check {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.gcb-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 36px;
  color: var(--text-secondary);
  font-size: 12px;
}

.gcb-tree-card {
  overflow: hidden;
  padding: 0;
  flex-shrink: 0;
}

.gcb-tree-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
  font-size: 12px;
  color: var(--text-secondary);
}

.gcb-empty-dir {
  padding: 28px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.gcb-table {
  width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
  font-size: 12px;
}

.gcb-row {
  border-bottom: 1px solid var(--border-color);
  cursor: pointer;
  transition: background var(--transition-fast);
}

.gcb-row:last-child {
  border-bottom: 0;
}

.gcb-row:hover {
  background: var(--hover-bg);
}

.gcb-row td {
  padding: 7px 12px;
  vertical-align: middle;
}

.gcb-col-name {
  min-width: 0;
}

.gcb-name-cell {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-weight: 500;
  color: var(--text-primary);
}

.gcb-entry-link {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gcb-entry-icon {
  flex-shrink: 0;
}

.gcb-entry-icon.dir {
  color: var(--primary-color);
}

.gcb-entry-icon.file {
  color: var(--text-secondary);
}

.gcb-row:hover .gcb-entry-link {
  color: var(--primary-color);
}

.gcb-col-mode,
.gcb-col-sha,
.gcb-col-size {
  width: 82px;
  color: var(--text-secondary);
  font-size: 11px;
  text-align: right;
  white-space: nowrap;
}

.gcb-col-status {
  width: 86px;
  white-space: nowrap;
}

.gcb-col-ops {
  width: 96px;
  text-align: right;
  white-space: nowrap;
}

.gcb-row-ops {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  opacity: 0.7;
}

.gcb-row:hover .gcb-row-ops {
  opacity: 1;
}

.gcb-file-card {
  display: flex;
  flex-direction: column;
  padding: 0;
  overflow: hidden;
  flex-shrink: 0;
}

.gcb-file-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
}

.gcb-file-path {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  flex: 1;
  min-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gcb-file-btns {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.gcb-trunc-note {
  margin: 8px 12px 0;
}

.gcb-editor-wrap {
  padding: 10px;
}

.gcb-editor {
  width: 100%;
  min-height: 420px;
  padding: 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  line-height: 1.55;
  resize: vertical;
}

.gcb-code-scroll {
  max-height: 68vh;
  overflow: auto;
}

.gcb-code-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  line-height: 1.5;
}

.gcb-ln {
  width: 1%;
  min-width: 48px;
  padding: 0 10px;
  text-align: right;
  color: var(--text-secondary);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  vertical-align: top;
  user-select: none;
  border-right: 1px solid var(--border-color);
}

.gcb-code {
  width: 99%;
  padding: 0 12px;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--text-primary);
}

.gcb-image-wrap {
  display: flex;
  justify-content: center;
  padding: 24px;
  background: var(--bg-primary);
}

.gcb-image {
  max-width: 100%;
  max-height: 480px;
  border-radius: var(--radius-md);
}

.gcb-hex-wrap {
  padding: 12px;
  max-height: 480px;
  overflow: auto;
}

.gcb-hex {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
}

.gcb-readme-card {
  padding: 0;
  overflow: hidden;
  flex-shrink: 0;
}

.gcb-readme-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-primary);
  font-size: 12px;
  font-weight: 600;
}

.gcb-markdown {
  padding: 14px 18px;
  font-size: 13px;
  line-height: 1.65;
  color: var(--text-primary);
}

.gcb-markdown :deep(p + p),
.gcb-markdown :deep(ul),
.gcb-markdown :deep(blockquote),
.gcb-markdown :deep(pre) {
  margin-top: 8px;
}

.gcb-markdown :deep(ul) {
  padding-left: 20px;
}

.gcb-markdown :deep(blockquote) {
  padding: 6px 12px;
  border-radius: var(--radius-sm);
  background: var(--surface-primary);
  color: var(--text-secondary);
}

.gcb-markdown :deep(pre) {
  padding: 10px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-primary);
  overflow-x: auto;
}

.gcb-markdown :deep(code) {
  font-family: var(--font-mono);
  font-size: 12px;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--surface-tertiary);
}

.gcb-readme-plain {
  margin: 0;
  padding: 14px 18px;
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
}
</style>
