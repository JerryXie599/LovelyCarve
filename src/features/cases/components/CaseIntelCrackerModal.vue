<template>
  <Teleport to="body">
    <Transition name="ui-modal">
      <div
        v-if="visible"
        class="ui-modal-overlay intel-overlay"
        role="dialog"
        aria-modal="true"
        aria-labelledby="case-intel-title"
        @click.self="emit('close')"
      >
        <div ref="dialogEl" class="ui-modal intel-modal" tabindex="-1" @keydown.esc.stop="emit('close')">
          <header class="ui-modal-header">
            <span class="ui-modal-icon accent"><Shield :size="18" /></span>
            <div class="ui-modal-heading">
              <h2 id="case-intel-title" class="ui-modal-title">{{ tr("案件情报中心与多核破解") }}</h2>
              <p class="ui-modal-subtitle">
                {{ caseName ? tr("当前案件：{0}", { 0: caseName }) : tr("汇总全案 IOC、生成案件字典及并发破解密码/哈希") }}
              </p>
            </div>
            <button class="ui-icon-btn" type="button" :title="tr('关闭')" :aria-label="tr('关闭')" @click="emit('close')">
              <X :size="16" />
            </button>
          </header>

          <div class="intel-tabs-bar">
            <div class="ui-tabs" role="tablist">
              <button
                type="button"
                class="ui-tab"
                role="tab"
                :class="{ active: activeTab === 'iocs' }"
                :aria-selected="activeTab === 'iocs'"
                @click="activeTab = 'iocs'"
              >
                {{ tr("案件 IOC 汇总") }}
              </button>
              <button
                type="button"
                class="ui-tab"
                role="tab"
                :class="{ active: activeTab === 'wordlist' }"
                :aria-selected="activeTab === 'wordlist'"
                @click="activeTab = 'wordlist'"
              >
                {{ tr("案件字典生成") }}
              </button>
              <button
                type="button"
                class="ui-tab"
                role="tab"
                :class="{ active: activeTab === 'cracker' }"
                :aria-selected="activeTab === 'cracker'"
                @click="activeTab = 'cracker'"
              >
                {{ tr("多核口令/哈希破解") }}
              </button>
            </div>
          </div>

          <div class="ui-modal-body intel-body">
            <!-- Tab 1: Case IOCs -->
            <section v-if="activeTab === 'iocs'" class="intel-section">
              <div class="intel-toolbar">
                <UiSelect v-model="iocKind" :options="iocKindOptions" class="ioc-select" :aria-label="tr('IOC 类型筛选')" />
                <input
                  v-model="iocQuery"
                  class="ui-input sm intel-search"
                  :placeholder="tr('按关键字筛选 IOC 值或来源…')"
                  @keydown.enter="loadIocs"
                />
                <button type="button" class="ui-btn sm primary" :disabled="iocLoading" @click="loadIocs">
                  <Loader2 v-if="iocLoading" :size="13" class="spin" />
                  <RefreshCw v-else :size="13" />
                  <span>{{ iocLoading ? tr("汇总中…") : tr("刷新汇总") }}</span>
                </button>
                <button type="button" class="ui-btn sm" :disabled="!iocReport || iocReport.items.length === 0" @click="exportIocsCsv">
                  <Download :size="13" />
                  <span>{{ tr("导出 CSV") }}</span>
                </button>
              </div>

              <div v-if="iocReport" class="intel-stats mono">
                <span class="ui-badge sm">{{ tr("总 IOC") }}: {{ iocReport.totalIocs }}</span>
                <span class="ui-badge sm warn">{{ tr("跨镜像命中") }}: {{ iocReport.crossImageCount }}</span>
                <span v-for="(cnt, kind) in iocReport.byKind" :key="kind" class="ui-badge sm">{{ kind }}: {{ cnt }}</span>
              </div>

              <p v-if="iocError" class="ui-alert danger">{{ iocError }}</p>
              <div v-else-if="!iocReport && !iocLoading" class="ui-empty intel-empty">
                <p class="ui-empty-desc">{{ tr("点击「刷新汇总」跨检材聚合 IP、MAC、邮箱、域名、账号、Wi-Fi SSID、硬件序列号与令牌。") }}</p>
              </div>
              <div v-else-if="iocReport && iocReport.items.length === 0" class="ui-empty intel-empty">
                <p class="ui-empty-desc">{{ tr("未提取到匹配的 IOC 条目") }}</p>
              </div>
              <div v-else-if="iocReport" class="intel-table-wrap">
                <table class="data-table mono intel-table">
                  <thead>
                    <tr>
                      <th>{{ tr("类型") }}</th>
                      <th>{{ tr("IOC 值") }}</th>
                      <th>{{ tr("命中次数") }}</th>
                      <th>{{ tr("关联检材") }}</th>
                      <th>{{ tr("来源") }}</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="(item, idx) in iocReport.items" :key="`${item.kind}:${item.value}:${idx}`">
                      <td><span class="ui-badge sm">{{ item.kind }}</span></td>
                      <td class="ioc-val">
                        {{ item.value }}
                        <span v-if="item.crossImageHit" class="ui-badge sm warn">{{ tr("跨镜像") }}</span>
                      </td>
                      <td>{{ item.count }}</td>
                      <td>{{ item.images.join(', ') }}</td>
                      <td class="ioc-sources" :title="item.sources.join('\n')">{{ item.sources.slice(0, 3).join(' · ') }}</td>
                      <td>
                        <button type="button" class="ui-icon-btn xs" :title="tr('复制值')" :aria-label="tr('复制值')" @click="copyVal(item.value)">
                          <Copy :size="12" />
                        </button>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </section>

            <!-- Tab 2: Case Wordlist -->
            <section v-else-if="activeTab === 'wordlist'" class="intel-section">
              <div class="intel-toolbar">
                <button type="button" class="ui-btn sm primary" :disabled="wlLoading" @click="generateWordlist()">
                  <Loader2 v-if="wlLoading" :size="13" class="spin" />
                  <Sparkles v-else :size="13" />
                  <span>{{ wlLoading ? tr("生成中…") : tr("生成案件专属字典") }}</span>
                </button>
                <button type="button" class="ui-btn sm" :disabled="!wlReport || wlReport.words.length === 0" @click="copyWordlist">
                  <Copy :size="13" />
                  <span>{{ tr("复制全部词条") }}</span>
                </button>
                <button type="button" class="ui-btn sm" :disabled="wlLoading" @click="exportWordlistFile">
                  <Download :size="13" />
                  <span>{{ tr("导出为字典文件") }}</span>
                </button>
                <button type="button" class="ui-btn sm" :disabled="!wlReport || wlReport.words.length === 0" @click="sendWordlistToCracker">
                  <Zap :size="13" />
                  <span>{{ tr("发送到破解器") }}</span>
                </button>
              </div>

              <div v-if="wlReport" class="intel-stats mono">
                <span class="ui-badge sm">{{ tr("基础词根") }}: {{ wlReport.baseCount }}</span>
                <span class="ui-badge sm info">{{ tr("含变体词条") }}: {{ wlReport.totalWithVariants }}</span>
                <span v-if="wlReport.exportedPath" class="ui-badge sm success">{{ tr("已保存") }}: {{ wlReport.exportedPath }}</span>
              </div>
              <div v-if="wlReport && wlReport.sources.length" class="wl-sources mono">
                {{ tr("来源") }}: {{ wlReport.sources.join(' · ') }}
              </div>
              <p v-if="wlError" class="ui-alert danger">{{ wlError }}</p>
              <div v-else-if="wlReport" class="wl-preview mono">
                <pre>{{ wlReport.words.slice(0, 500).join('\n') }}</pre>
                <p v-if="wlReport.words.length > 500" class="wl-trunc">{{ tr("仅展示前 500 条，共 {0} 条；可导出或复制到破解器使用。", { 0: wlReport.words.length }) }}</p>
              </div>
              <div v-else-if="!wlLoading" class="ui-empty intel-empty">
                <p class="ui-empty-desc">{{ tr("从案件账户名、主机名、Wi-Fi SSID、密码提示文件和便签中提取词汇并自动扩充常见口令变体。") }}</p>
              </div>
            </section>

            <!-- Tab 3: Multi-Core Cracker -->
            <section v-else class="intel-section">
              <div class="crack-grid">
                <label class="crack-field">
                  <span>{{ tr("目标类型") }}</span>
                  <UiSelect v-model="crackType" :options="crackTypeOptions" />
                </label>
                <label class="crack-field">
                  <span>{{ tr("数字 PIN 范围") }}</span>
                  <UiSelect v-model="pinLength" :options="pinLengthOptions" />
                </label>
                <label class="crack-field wide">
                  <span>{{ tr("目标摘要 / 令牌 / Vault JSON / ZIP 路径或十六进制") }}</span>
                  <div class="crack-input-row">
                    <input
                      v-model="crackTarget"
                      class="ui-input sm mono"
                      :placeholder="crackType === 'zip' ? tr('选择受加密的 ZIP 文件路径或粘贴十六进制内容') : tr('粘贴哈希 hex、Fernet Token (gAAAAA...) 或 MetaMask Vault JSON')"
                    />
                    <button v-if="crackType === 'zip' || crackType === 'metamask'" type="button" class="ui-btn sm" @click="pickTargetFile">
                      {{ tr("选择文件") }}
                    </button>
                  </div>
                </label>
                <label class="crack-field wide">
                  <span>{{ tr("外部字典文件路径（可选）") }}</span>
                  <div class="crack-input-row">
                    <input v-model="crackWordlistPath" class="ui-input sm mono" :placeholder="tr('可选：选择密码字典 .txt 文件')" />
                    <button type="button" class="ui-btn sm" @click="pickCrackWordlist">{{ tr("选择字典") }}</button>
                    <button v-if="crackWordlistPath" type="button" class="ui-btn sm ghost" @click="crackWordlistPath = ''">{{ tr("清空") }}</button>
                  </div>
                </label>
                <label class="crack-field wide">
                  <span>{{ tr("自定义候选词列表（每行一个）") }}</span>
                  <textarea
                    v-model="crackCandidatesRaw"
                    rows="5"
                    class="ui-input mono crack-textarea"
                    :placeholder="tr('每行输入一个候选密码，或从「案件字典生成」一键导入')"
                  ></textarea>
                </label>
                <div class="crack-controls wide">
                  <label class="ui-check">
                    <input v-model="enableRules" type="checkbox" />
                    <span>{{ tr("启用常见年份/符号/大小写变体规则") }}</span>
                  </label>
                  <button type="button" class="ui-btn primary" :disabled="crackLoading || !crackTarget.trim()" @click="runCrack">
                    <Loader2 v-if="crackLoading" :size="14" class="spin" />
                    <Zap v-else :size="14" />
                    <span>{{ crackLoading ? tr("多核并发破解中…") : tr("开始多核破解") }}</span>
                  </button>
                </div>
              </div>

              <p v-if="crackError" class="ui-alert danger">{{ crackError }}</p>
              <div v-if="crackResult" class="crack-result ui-card mono" :class="{ found: crackResult.found }">
                <div class="crack-result-head">
                  <span class="ui-badge sm" :class="crackResult.found ? 'success' : 'warn'">
                    {{ crackResult.found ? tr("破解成功") : tr("未命中") }}
                  </span>
                  <span>{{ tr("类型") }}: {{ crackResult.targetType }}</span>
                  <span>{{ tr("尝试次数") }}: {{ crackResult.testedCandidates.toLocaleString() }}</span>
                  <span>{{ tr("耗时") }}: {{ crackResult.elapsedMs }} ms</span>
                </div>
                <div v-if="crackResult.password !== null" class="crack-pw-row">
                  <span>{{ tr("匹配密码") }}: <strong>{{ crackResult.password }}</strong></span>
                  <button type="button" class="ui-btn sm" @click="copyVal(crackResult.password ?? '')">
                    <Copy :size="12" />{{ tr("复制密码") }}
                  </button>
                </div>
                <pre v-if="crackResult.decryptedPreview" class="crack-preview">{{ crackResult.decryptedPreview }}</pre>
              </div>
            </section>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { Copy, Download, Loader2, RefreshCw, Shield, Sparkles, X, Zap } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { copyText } from '../../../composables/clipboard';
import { notifyError, notifySuccess } from '../../../composables/useToast';
import { useDialogFocus } from '../../../composables/useDialogFocus';
import UiSelect, { type UiSelectOption } from '../../../components/ui/UiSelect.vue';
import { saveTextWithDialog } from '../../../composables/export';
import { recordsToCsv } from '../../../utils/exportFormat';
import {
  caseIocsList,
  caseWordlistGet,
  crackerStart,
  type CaseIocsReport,
  type CaseWordlistReport,
  type CrackResult,
} from '../../../lib/ipc/caseIntel';

const props = defineProps<{
  visible: boolean;
  caseName?: string | null;
}>();

const emit = defineEmits<{ close: [] }>();

const dialogEl = ref<HTMLElement | null>(null);
useDialogFocus(() => props.visible, dialogEl);

const activeTab = ref<'iocs' | 'wordlist' | 'cracker'>('iocs');

// ── Tab 1: IOCs ──
const iocKind = ref('');
const iocQuery = ref('');
const iocLoading = ref(false);
const iocError = ref('');
const iocReport = ref<CaseIocsReport | null>(null);

const iocKindOptions: UiSelectOption<string>[] = [
  { value: '', get label() { return tr('全部 IOC 类型'); } },
  { value: 'ip', get label() { return tr('IP 地址 (ip)'); } },
  { value: 'mac', get label() { return tr('MAC 地址 (mac)'); } },
  { value: 'email', get label() { return tr('电子邮箱 (email)'); } },
  { value: 'domain', get label() { return tr('域名/主机 (domain)'); } },
  { value: 'account', get label() { return tr('账号 (account)'); } },
  { value: 'ssid', get label() { return tr('Wi-Fi SSID (ssid)'); } },
  { value: 'serial', get label() { return tr('设备序列号 (serial)'); } },
  { value: 'token', get label() { return tr('密钥/令牌 (token)'); } },
];

async function loadIocs() {
  iocLoading.value = true;
  iocError.value = '';
  try {
    iocReport.value = await caseIocsList({
      kind: iocKind.value || null,
      query: iocQuery.value.trim() || null,
    });
  } catch (err) {
    iocError.value = String(err);
  } finally {
    iocLoading.value = false;
  }
}

watch(() => props.visible, open => {
  if (open && activeTab.value === 'iocs' && !iocReport.value && !iocLoading.value) {
    void loadIocs();
  }
});

async function exportIocsCsv() {
  if (!iocReport.value?.items.length) return;
  const columns = ['Kind', 'Value', 'Count', 'CrossImageHit', 'Images', 'Sources'];
  const content = recordsToCsv(
    iocReport.value.items.map(item => ({
      Kind: item.kind,
      Value: item.value,
      Count: String(item.count),
      CrossImageHit: item.crossImageHit ? 'true' : 'false',
      Images: item.images.join('; '),
      Sources: item.sources.join('; '),
    })),
    columns,
  );
  await saveTextWithDialog({
    content,
    defaultPath: 'case-iocs.csv',
    title: tr('导出案件 IOC 汇总 CSV'),
    filters: [{ name: 'CSV', extensions: ['csv'] }],
  });
}

// ── Tab 2: Wordlist ──
const wlLoading = ref(false);
const wlError = ref('');
const wlReport = ref<CaseWordlistReport | null>(null);

async function generateWordlist(destination?: string | null) {
  wlLoading.value = true;
  wlError.value = '';
  try {
    wlReport.value = await caseWordlistGet({ destination: destination ?? null });
    if (destination && wlReport.value.exportedPath) {
      notifySuccess(tr('字典已导出'), wlReport.value.exportedPath);
    }
  } catch (err) {
    wlError.value = String(err);
  } finally {
    wlLoading.value = false;
  }
}

async function exportWordlistFile() {
  const path = await saveDialog({
    title: tr('导出案件字典'),
    defaultPath: 'case-wordlist.txt',
    filters: [{ name: tr('文本文件'), extensions: ['txt'] }],
  });
  if (typeof path === 'string' && path) {
    await generateWordlist(path);
  }
}

function copyWordlist() {
  if (!wlReport.value?.words.length) return;
  void copyText(wlReport.value.words.join('\n'), tr('案件字典'));
}

function sendWordlistToCracker() {
  if (!wlReport.value?.words.length) return;
  crackCandidatesRaw.value = wlReport.value.words.join('\n');
  if (wlReport.value.exportedPath) crackWordlistPath.value = wlReport.value.exportedPath;
  activeTab.value = 'cracker';
}

// ── Tab 3: Cracker ──
const crackType = ref('ntlm');
const crackTarget = ref('');
const crackWordlistPath = ref('');
const crackCandidatesRaw = ref('');
const enableRules = ref(true);
const pinLength = ref('4');
const crackLoading = ref(false);
const crackError = ref('');
const crackResult = ref<CrackResult | null>(null);

const crackTypeOptions: UiSelectOption<string>[] = [
  { value: 'ntlm', label: 'Windows NTLM (MD4 UTF-16LE)' },
  { value: 'md5', label: 'MD5' },
  { value: 'sha1', label: 'SHA-1' },
  { value: 'sha256', label: 'SHA-256' },
  { value: 'sha512', label: 'SHA-512' },
  { value: 'fernet', label: 'Fernet v1 Token (gAAAAA...)' },
  { value: 'metamask', label: 'MetaMask Vault JSON' },
  { value: 'zip', label: 'ZIP Archive (AES / ZipCrypto)' },
];

const pinLengthOptions: UiSelectOption<string>[] = [
  { value: '0', get label() { return tr('不扫描数字 PIN'); } },
  { value: '4', get label() { return tr('4 位数字 PIN (0000–9999)'); } },
  { value: '6', get label() { return tr('6 位常见数字 PIN'); } },
];

async function pickTargetFile() {
  const selected = await openDialog({
    multiple: false,
    title: tr('选择目标文件'),
  });
  if (typeof selected === 'string' && selected) {
    crackTarget.value = selected;
  }
}

async function pickCrackWordlist() {
  const selected = await openDialog({
    multiple: false,
    title: tr('选择密码字典文件'),
    filters: [{ name: tr('字典/文本'), extensions: ['txt', 'dic', 'lst'] }, { name: tr('所有文件'), extensions: ['*'] }],
  });
  if (typeof selected === 'string' && selected) {
    crackWordlistPath.value = selected;
  }
}

async function runCrack() {
  if (!crackTarget.value.trim()) return;
  crackLoading.value = true;
  crackError.value = '';
  crackResult.value = null;
  try {
    const candidates = crackCandidatesRaw.value
      .split(/\r?\n/)
      .map(s => s.trimEnd())
      .filter(Boolean);
    const pin = Number(pinLength.value);
    crackResult.value = await crackerStart({
      targetType: crackType.value,
      target: crackTarget.value.trim(),
      wordlistPath: crackWordlistPath.value.trim() || null,
      candidates,
      enableRules: enableRules.value,
      pinLength: pin > 0 ? pin : null,
    });
    if (crackResult.value.found && crackResult.value.password != null) {
      notifySuccess(tr('破解成功'), crackResult.value.password);
    }
  } catch (err) {
    crackError.value = String(err);
    notifyError(tr('破解失败'), crackError.value);
  } finally {
    crackLoading.value = false;
  }
}

function copyVal(value: string) {
  if (!value) return;
  void copyText(value, tr('内容'));
}
</script>

<style scoped>
.intel-modal {
  width: min(920px, 94vw);
  max-height: 88vh;
  display: flex;
  flex-direction: column;
}
.intel-tabs-bar {
  padding: 0 20px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
}
.intel-tabs-bar .ui-tabs {
  gap: 18px;
  border-bottom: 0;
}
.intel-body {
  padding: 16px 20px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow: hidden;
  flex: 1;
}
.intel-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  flex: 1;
}
.intel-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.ioc-select { width: 190px; }
.intel-search { flex: 1; min-width: 180px; }
.intel-stats {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  font-size: 11px;
}
.intel-table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}
.intel-table { width: 100%; font-size: 12px; }
.ioc-val { word-break: break-all; }
.ioc-sources { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-secondary); }
.wl-sources { font-size: 11px; color: var(--text-secondary); }
.wl-preview {
  flex: 1;
  min-height: 160px;
  max-height: 360px;
  overflow: auto;
  padding: 10px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-secondary);
  font-size: 12px;
}
.wl-preview pre { margin: 0; white-space: pre-wrap; word-break: break-all; }
.wl-trunc { margin: 8px 0 0; font-size: 11px; color: var(--text-secondary); }
.crack-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 12px;
}
.crack-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}
.crack-field.wide, .crack-controls.wide { grid-column: 1 / -1; }
.crack-input-row { display: flex; align-items: center; gap: 6px; }
.crack-input-row .ui-input { flex: 1; }
.crack-textarea { resize: vertical; min-height: 88px; }
.crack-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px;
}
.crack-result {
  padding: 10px 12px;
  display: grid;
  gap: 8px;
  font-size: 12px;
}
.crack-result.found { border-color: color-mix(in srgb, var(--sev-success-fg) 60%, var(--border-color)); }
.crack-result-head { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; }
.crack-pw-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.crack-preview { margin: 0; padding: 8px; border-radius: var(--radius-sm); background: var(--surface-secondary); white-space: pre-wrap; word-break: break-all; max-height: 180px; overflow: auto; }
.intel-empty { min-height: 160px; }
</style>
