<template>
  <section class="git-workspace" :aria-label="tr('Git / GitHub 项目管理')">
    <!-- Empty / Initial Scanner State -->
    <div v-if="items.length === 0" class="gw-empty-wrap">
      <div class="ui-card gw-scanner-card">
        <span class="gw-empty-icon"><FolderGit2 :size="36" /></span>
        <h2 class="gw-empty-title">{{ tr("Git / GitHub 项目交互工作台") }}</h2>
        <p class="gw-empty-desc">
          {{ tr("自动扫描检材中的所有 .git 仓库，支持像 GitHub 一样浏览代码目录、回滚历史版本、比对 Reflog 差异、提取隐藏敏感数据并在隔离沙盒中直接操作仓库。") }}
        </p>

        <div class="gw-scan-controls">
          <input
            v-model="customRepoPath"
            class="ui-input sm mono gw-scan-input"
            :placeholder="tr('可选：指定仓库路径（留空则自动全盘扫描 .git）')"
          />
          <button
            type="button"
            class="ui-btn primary"
            :disabled="scanning"
            @click="startScan"
          >
            <Loader2 v-if="scanning" :size="15" class="spin" />
            <ScanSearch v-else :size="15" />
            <span>{{ tr("扫描检材 Git 仓库") }}</span>
          </button>
        </div>

        <p v-if="loadingItems" class="gw-loading-hint">
          <Loader2 :size="13" class="spin" />
          <span>{{ tr("正在检查已扫描的项目记录…") }}</span>
        </p>
      </div>
    </div>

    <!-- Active Repository Workspace -->
    <template v-else>
      <header class="gw-header ui-card">
        <div class="gw-head-top">
          <div class="gw-repo-picker">
            <FolderGit2 :size="18" class="gw-repo-icon" />
            <UiSelect
              :model-value="selectedKey"
              :options="repoOptions"
              class="sm gw-repo-select"
              :aria-label="tr('切换 Git 仓库')"
              @update:model-value="onRepoSwitch"
            />

            <span v-if="githubBadge" class="ui-badge sm info gw-gh-badge">
              {{ githubBadge }}
            </span>

            <span v-if="overview?.userName || overview?.userEmail" class="gw-author-chip">
              {{ overview.userName }} &lt;{{ overview.userEmail }}&gt;
            </span>
          </div>

          <div class="gw-head-actions">
            <!-- Mode Switcher: Read-only Evidence vs Interactive Case Sandbox -->
            <div class="ui-seg" role="group" :aria-label="tr('数据源模式')">
              <button
                type="button"
                class="ui-seg-item"
                :class="{ active: !useSandbox }"
                :title="tr('直接只读读取原始检材中的 Git 对象与工作区')"
                @click="useSandbox = false"
              >
                <ShieldCheck :size="13" />
                <span>{{ tr("镜像只读") }}</span>
              </button>
              <button
                type="button"
                class="ui-seg-item"
                :class="{ active: useSandbox }"
                :title="tr('在案件隔离沙盒中执行回滚、建分支、编辑与提交')"
                @click="enableSandboxMode"
              >
                <Terminal :size="13" />
                <span>{{ tr("交互沙盒") }}</span>
                <span v-if="overview?.sandbox.dirtyCount" class="ui-badge sm warn">
                  {{ overview.sandbox.dirtyCount }}
                </span>
              </button>
            </div>

            <button
              type="button"
              class="ui-btn sm"
              :title="tr('在文件列表中定位该仓库')"
              @click="locateRepoInFiles"
            >
              <MapPin :size="13" />
              <span>{{ tr("文件列表定位") }}</span>
            </button>

            <button
              type="button"
              class="ui-btn sm"
              :disabled="scanning"
              :title="tr('重新扫描全盘 Git 仓库')"
              @click="startScan"
            >
              <Loader2 v-if="scanning" :size="13" class="spin" />
              <RefreshCw v-else :size="13" />
              <span>{{ tr("重新扫描") }}</span>
            </button>
          </div>
        </div>

        <div v-if="overview" class="gw-head-meta">
          <span class="gw-path mono">{{ overview.repoPath }}</span>
          <template v-for="rem in overview.remotes" :key="rem.name">
            <span class="gw-remote">
              <Link2 :size="12" />
              <span class="mono">{{ rem.name }}: {{ rem.url }}</span>
              <button
                type="button"
                class="ui-icon-btn sm"
                :title="tr('复制远程地址')"
                @click="copyRemote(rem.url)"
              >
                <Copy :size="11" />
              </button>
            </span>
          </template>
        </div>

        <!-- Credential Leak Alert Banner -->
        <div v-if="credentialLeaks.length > 0" class="ui-alert danger gw-leak-banner">
          <TriangleAlert :size="15" />
          <div class="gw-leak-body">
            <strong>{{ tr("检测到仓库远程配置或对象中存在敏感凭据暴露 ({0} 处)", { 0: credentialLeaks.length }) }}</strong>
            <div class="gw-leak-preview mono">{{ credentialLeaks[0].preview }}</div>
          </div>
          <button type="button" class="ui-btn sm" @click="activeTab = 'recovery'">
            {{ tr("查看敏感恢复") }}
          </button>
        </div>

        <!-- GitHub-Style Tab Navigation -->
        <nav class="ui-tabs gw-tabs" :aria-label="tr('仓库功能页')">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            type="button"
            class="ui-tab gw-tab"
            :class="{ active: activeTab === tab.id }"
            @click="activeTab = tab.id"
          >
            <component :is="tab.icon" :size="14" />
            <span>{{ tab.label }}</span>
            <span v-if="tab.count !== null" class="ui-count" :class="{ accent: tab.accent }">
              {{ tab.count.toLocaleString() }}
            </span>
          </button>
        </nav>
      </header>

      <!-- Main Body -->
      <div class="gw-body">
        <div v-if="overviewLoading && !overview" class="gw-state ui-card">
          <Loader2 :size="20" class="spin" />
          <span>{{ tr("正在解析 Git 对象库与历史快照…") }}</span>
        </div>

        <div v-else-if="overviewError" class="ui-alert danger">
          <span>{{ overviewError }}</span>
          <button type="button" class="ui-btn sm" @click="loadOverview">{{ tr("重试") }}</button>
        </div>

        <template v-else-if="overview">
          <GitCodeBrowser
            v-if="activeTab === 'code'"
            :image-id="imageId"
            :overview="overview"
            :use-sandbox="useSandbox"
            :rev="codeRev"
            @update:rev="codeRev = $event"
            @sandbox-mutated="onSandboxMutated"
            @inspect-commit-diff="openCommitInTab"
          />

          <GitCommitsPanel
            v-else-if="activeTab === 'commits'"
            :image-id="imageId"
            :overview="overview"
            :use-sandbox="useSandbox"
            :focus-sha="focusCommitSha"
            @browse-commit="browseCommitInCode"
            @sandbox-mutated="onSandboxMutated"
          />

          <GitReflogPanel
            v-else-if="activeTab === 'reflog'"
            :image-id="imageId"
            :overview="overview"
            :use-sandbox="useSandbox"
            @browse-commit="browseCommitInCode"
            @sandbox-mutated="onSandboxMutated"
          />

          <GitRecoveryPanel
            v-else-if="activeTab === 'recovery'"
            :image-id="imageId"
            :overview="overview"
            :use-sandbox="useSandbox"
            @browse-commit="browseCommitInCode"
            @inspect-commit-diff="openCommitInTab"
            @sandbox-mutated="onSandboxMutated"
          />

          <GitChangesPanel
            v-else-if="activeTab === 'changes'"
            :image-id="imageId"
            :overview="overview"
            :use-sandbox="useSandbox"
            @sandbox-mutated="onSandboxMutated"
          />
        </template>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import {
  Code2,
  Copy,
  FolderGit2,
  GitBranch,
  GitCommitHorizontal,
  History,
  KeyRound,
  Link2,
  Loader2,
  MapPin,
  RefreshCw,
  ScanSearch,
  ShieldCheck,
  Terminal,
  TriangleAlert,
} from 'lucide-vue-next';
import UiSelect, { type UiSelectOption } from '../../../components/ui/UiSelect.vue';
import { copyText } from '../../../composables/clipboard';
import { tr } from '../../../composables/useLocale';
import { notifyError, notifyInfo } from '../../../composables/useToast';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  gitWorkspaceOverview,
  gitWorkspaceSandboxGit,
  type GitRepoOverview,
} from '../../../lib/ipc/gitWorkspace';
import type { FileLocationTarget } from '../../../lib/models/fileLocation';
import { usePluginTools } from '../composables/usePluginTools';
import { parseRepoEvidenceLocation, type PluginToolGroup } from '../model/pluginTools';
import GitChangesPanel from './GitChangesPanel.vue';
import GitCodeBrowser from './GitCodeBrowser.vue';
import GitCommitsPanel from './GitCommitsPanel.vue';
import GitRecoveryPanel from './GitRecoveryPanel.vue';
import GitReflogPanel from './GitReflogPanel.vue';

const props = defineProps<{
  imageId: ImageId;
  group: PluginToolGroup;
}>();

const emit = defineEmits<{
  openFile: [target: FileLocationTarget];
}>();

const {
  itemsByTool,
  loadingByTool,
  scanningByTool,
  selectedKeyByTool,
  selectToolItem,
  triggerToolScan,
} = usePluginTools();

const customRepoPath = ref('');
const useSandbox = ref(false);
const activeTab = ref<'code' | 'commits' | 'reflog' | 'recovery' | 'changes'>('code');
const codeRev = ref('WORKTREE');
const focusCommitSha = ref<string | null>(null);

const overview = shallowRef<GitRepoOverview | null>(null);
const overviewLoading = ref(false);
const overviewError = ref<string | null>(null);

const items = computed(() => itemsByTool.value[props.group.toolId] ?? []);
const repoOptions = computed<UiSelectOption<string>[]>(() =>
  items.value.map(item => ({
    value: item.key,
    label: `${item.label} (${item.key})`,
  })),
);
const loadingItems = computed(() => Boolean(loadingByTool.value[props.group.toolId]));
const scanning = computed(() => Boolean(scanningByTool.value[props.group.toolId]));

const selectedKey = computed(() => {
  const cur = selectedKeyByTool.value[props.group.toolId];
  if (cur && items.value.some(i => i.key === cur)) return cur;
  return items.value[0]?.key ?? '';
});

const githubBadge = computed(() =>
  overview.value?.remotes.find(r => r.githubRepo)?.githubRepo ?? null,
);

const credentialLeaks = computed(() =>
  (overview.value?.recoveries ?? []).filter(
    r => r.kind === 'config_leak' || r.kind === 'secret_blob',
  ),
);

const tabs = computed(() => [
  { id: 'code' as const, icon: Code2, label: tr("代码"), count: null, accent: false },
  {
    id: 'commits' as const,
    icon: GitCommitHorizontal,
    label: tr("提交历史"),
    count: overview.value?.commits.length ?? 0,
    accent: false,
  },
  {
    id: 'reflog' as const,
    icon: History,
    label: tr("回滚审计"),
    count: overview.value?.reflogs.length ?? 0,
    accent: false,
  },
  {
    id: 'recovery' as const,
    icon: KeyRound,
    label: tr("残留与敏感恢复"),
    count: overview.value?.recoveries.length ?? 0,
    accent: (overview.value?.recoveries.length ?? 0) > 0,
  },
  {
    id: 'changes' as const,
    icon: GitBranch,
    label: tr("工作区与分支"),
    count: overview.value?.statusEntries.length ?? 0,
    accent: (overview.value?.statusEntries.length ?? 0) > 0,
  },
]);

let ovSeq = 0;

async function loadOverview() {
  const repoPath = selectedKey.value;
  if (!props.imageId || !repoPath) {
    overview.value = null;
    return;
  }
  const seq = ++ovSeq;
  overviewLoading.value = true;
  overviewError.value = null;
  try {
    const res = await gitWorkspaceOverview({
      imageId: props.imageId,
      repoPath,
      useSandbox: useSandbox.value,
    });
    if (seq === ovSeq) overview.value = res;
  } catch (err) {
    if (seq === ovSeq) overviewError.value = String(err);
  } finally {
    if (seq === ovSeq) overviewLoading.value = false;
  }
}

watch(
  [() => props.imageId, selectedKey, useSandbox],
  () => {
    void loadOverview();
  },
  { immediate: true },
);

watch(selectedKey, () => {
  useSandbox.value = false;
  codeRev.value = 'WORKTREE';
  focusCommitSha.value = null;
  activeTab.value = 'code';
});

function onRepoSwitch(key: string) {
  selectToolItem(props.group.toolId, key);
}

async function enableSandboxMode() {
  if (useSandbox.value) return;
  if (!selectedKey.value) return;
  try {
    if (!overview.value?.sandbox.initialized) {
      await gitWorkspaceSandboxGit({
        imageId: props.imageId,
        repoPath: selectedKey.value,
        action: 'init',
      });
    }
    useSandbox.value = true;
    notifyInfo(tr("已切换至交互沙盒模式"), tr("可在沙盒中自由新建/编辑文件、回滚或建分支，原始镜像不受影响。"));
  } catch (err) {
    notifyError(tr("初始化交互沙盒失败"), String(err));
  }
}

async function onSandboxMutated() {
  useSandbox.value = true;
  await loadOverview();
}

function browseCommitInCode(sha: string) {
  codeRev.value = sha;
  activeTab.value = 'code';
}

function openCommitInTab(sha: string) {
  focusCommitSha.value = sha;
  activeTab.value = 'commits';
}

function locateRepoInFiles() {
  const target = parseRepoEvidenceLocation(props.imageId, selectedKey.value);
  if (target) emit('openFile', target);
}

async function copyRemote(url: string) {
  await copyText(url, tr("远程地址"));
}

async function startScan() {
  try {
    const opts = customRepoPath.value.trim()
      ? { repo_path: customRepoPath.value.trim() }
      : {};
    await triggerToolScan(props.group.toolId, props.imageId, opts);
    notifyInfo(tr("已开始扫描"), tr("正在扫描检材中的 .git 仓库，完成后将自动加载。"));
  } catch (err) {
    notifyError(tr("扫描任务提交失败"), String(err));
  }
}
</script>

<style scoped>
.git-workspace {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  min-height: 0;
  padding: 12px 16px;
  overflow: hidden;
}

.gw-empty-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
}

.gw-scanner-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 12px;
  max-width: 580px;
  padding: 32px 28px;
}

.gw-empty-icon {
  display: grid;
  place-items: center;
  width: 64px;
  height: 64px;
  border-radius: var(--radius-xl);
  background: var(--sidebar-active-bg);
  color: var(--primary-color);
}

.gw-empty-title {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
}

.gw-empty-desc {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary);
}

.gw-scan-controls {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
  width: 100%;
  margin-top: 4px;
}

.gw-scan-input {
  flex: 1;
  min-width: 240px;
}

.gw-loading-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}

.gw-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex-shrink: 0;
  padding: 12px 16px 0;
}

.gw-head-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.gw-repo-picker {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  min-width: 0;
  flex: 1;
}

.gw-repo-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}

.gw-repo-select {
  max-width: 420px;
  font-weight: 600;
}

.gw-author-chip {
  font-size: 12px;
  color: var(--text-secondary);
}

.gw-head-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.gw-head-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 14px;
  font-size: 12px;
  color: var(--text-secondary);
}

.gw-path {
  font-size: 11px;
  overflow-wrap: anywhere;
}

.gw-remote {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  min-width: 0;
  overflow-wrap: anywhere;
}

.gw-leak-banner {
  display: flex;
  align-items: center;
  gap: 10px;
}

.gw-leak-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
}

.gw-leak-preview {
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gw-tabs {
  gap: 18px;
  margin: 2px -16px 0;
  padding: 0 16px;
  border-top: 1px solid var(--border-color);
  border-bottom: 0;
  overflow-x: auto;
}

.gw-tab {
  height: 36px;
  font-size: 12px;
}

.gw-body {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.gw-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 40px;
  color: var(--text-secondary);
  font-size: 13px;
}
</style>
