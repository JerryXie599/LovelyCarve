<template>
  <div class="terminal-viewer" :class="{ 'color-on': ansiColorEnabled }">
    <header class="tv-header">
      <div class="tv-meta-row">
        <div class="tv-title-wrap">
          <Terminal :size="15" class="tv-icon" />
          <span class="tv-filename mono">{{ filename || tr("终端会话日志") }}</span>
          <span v-if="parsed.metadata.term" class="ui-badge sm mono">{{ parsed.metadata.term }}</span>
          <span v-if="parsed.metadata.tty" class="ui-badge sm mono">{{ parsed.metadata.tty }}</span>
          <span v-if="parsed.metadata.columns && parsed.metadata.rows" class="ui-badge sm mono">
            {{ parsed.metadata.columns }}×{{ parsed.metadata.rows }}
          </span>
          <span v-if="parsed.metadata.startedAt" class="ui-badge sm mono">{{ parsed.metadata.startedAt }}</span>
        </div>

        <div class="tv-actions">
          <button
            type="button"
            class="ui-btn sm mono tv-color-toggle"
            :class="{ active: ansiColorEnabled }"
            :aria-pressed="ansiColorEnabled"
            :title="tr('开启或关闭 Linux 终端 ANSI/SGR 颜色渲染')"
            @click="toggleAnsiColor"
          >
            <Palette :size="13" />
            <span>{{ tr("终端配色") }}: {{ ansiColorEnabled ? tr("开") : tr("关") }}</span>
          </button>

          <button type="button" class="ui-btn sm mono" :title="tr('复制还原后的纯文本')" @click="copyCleanText">
            <Copy :size="12" />
            <span>{{ tr("复制纯文本") }}</span>
          </button>

          <button type="button" class="ui-btn sm mono" :title="tr('转为日志表格视图（支持筛选/搜索）')" @click="emit('switch-log')">
            <Table2 :size="12" />
            <span>{{ tr("日志表格") }}</span>
          </button>

          <button type="button" class="ui-btn sm mono" :title="tr('查看含转义控制符的原始文本')" @click="emit('switch-code')">
            <Code2 :size="12" />
            <span>{{ tr("原始文本") }}</span>
          </button>
        </div>
      </div>

      <div class="tv-tabs-row">
        <div class="ui-seg" role="tablist">
          <button
            type="button"
            class="ui-seg-item"
            role="tab"
            :aria-selected="activeTab === 'screen'"
            @click="activeTab = 'screen'"
          >
            {{ tr("终端还原视图") }} ({{ filteredLines.length }})
          </button>
          <button
            type="button"
            class="ui-seg-item"
            role="tab"
            :aria-selected="activeTab === 'commands'"
            @click="activeTab = 'commands'"
          >
            {{ tr("命令提取") }} ({{ filteredCommands.length }})
          </button>
        </div>

        <div v-if="parsed.metadata.titles.length" class="tv-osc-title mono" :title="parsed.metadata.titles.join(' · ')">
          {{ parsed.metadata.titles[parsed.metadata.titles.length - 1] }}
        </div>
      </div>
    </header>

    <div v-if="activeTab === 'screen'" class="tv-screen mono">
      <div v-if="filteredLines.length === 0" class="tv-empty">
        {{ query ? tr("无匹配当前关键词的终端行") : tr("终端日志为空") }}
      </div>
      <div
        v-for="line in filteredLines"
        :key="line.lineNumber"
        class="tv-line"
        :class="{ 'is-alt': line.section === 'alt-screen', 'is-scrollback': line.section === 'scrollback' }"
      >
        <span class="tv-ln">{{ line.lineNumber }}</span>
        <span class="tv-content">
          <template v-if="line.spans.length === 0">&nbsp;</template>
          <template v-else-if="ansiColorEnabled">
            <template v-for="(span, sIdx) in line.spans" :key="sIdx">
              <span v-if="!trimmedQuery" :style="spanCss(span.style)">{{ span.text }}</span>
              <span v-else :style="spanCss(span.style)">
                <template v-for="(part, pIdx) in splitHighlight(span.text)" :key="pIdx">
                  <mark v-if="part.hit" class="tv-mark">{{ part.text }}</mark>
                  <template v-else>{{ part.text }}</template>
                </template>
              </span>
            </template>
          </template>
          <template v-else>
            <template v-if="!trimmedQuery">{{ line.plain }}</template>
            <template v-else>
              <template v-for="(part, pIdx) in splitHighlight(line.plain)" :key="pIdx">
                <mark v-if="part.hit" class="tv-mark">{{ part.text }}</mark>
                <template v-else>{{ part.text }}</template>
              </template>
            </template>
          </template>
        </span>
      </div>
    </div>

    <div v-else class="tv-commands">
      <div v-if="filteredCommands.length === 0" class="tv-empty mono">
        {{ tr("未识别到带 Shell 提示符的命令记录") }}
      </div>
      <table v-else class="tv-cmd-table mono">
        <thead>
          <tr>
            <th>#</th>
            <th>{{ tr("行号") }}</th>
            <th>{{ tr("工作目录 / 提示符") }}</th>
            <th>{{ tr("执行命令") }}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="cmd in filteredCommands" :key="`${cmd.index}-${cmd.lineNumber}`">
            <td class="tv-cmd-num">{{ cmd.index }}</td>
            <td class="tv-cmd-ln">L{{ cmd.lineNumber }}</td>
            <td class="tv-cmd-prompt">{{ cmd.cwd || cmd.prompt }}</td>
            <td class="tv-cmd-text">
              <template v-if="!trimmedQuery">{{ cmd.command }}</template>
              <template v-else>
                <template v-for="(part, pIdx) in splitHighlight(cmd.command)" :key="pIdx">
                  <mark v-if="part.hit" class="tv-mark">{{ part.text }}</mark>
                  <template v-else>{{ part.text }}</template>
                </template>
              </template>
            </td>
            <td>
              <button
                type="button"
                class="ui-icon-btn xs"
                :title="tr('复制命令')"
                :aria-label="tr('复制命令')"
                @click="copyCommand(cmd.command)"
              >
                <Copy :size="12" />
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, type CSSProperties } from 'vue';
import { Code2, Copy, Palette, Table2, Terminal } from 'lucide-vue-next';
import { tr } from '../../../../composables/useLocale';
import { copyText } from '../../../../composables/clipboard';
import { parseTerminalSession, type AnsiStyle } from '../../../../lib/forensics/terminalSessionParser';

const COLOR_PREF_KEY = 'lc.terminalAnsiColor';

const props = defineProps<{
  text: string;
  filename?: string;
  query?: string;
}>();

const emit = defineEmits<{
  'switch-code': [];
  'switch-log': [];
}>();

const ansiColorEnabled = ref<boolean>((() => {
  try {
    return localStorage.getItem(COLOR_PREF_KEY) !== 'false';
  } catch {
    return true;
  }
})());

function toggleAnsiColor() {
  ansiColorEnabled.value = !ansiColorEnabled.value;
  try {
    localStorage.setItem(COLOR_PREF_KEY, ansiColorEnabled.value ? 'true' : 'false');
  } catch {
    // ignore storage errors
  }
}

const activeTab = ref<'screen' | 'commands'>('screen');
const parsed = computed(() => parseTerminalSession(props.text));
const trimmedQuery = computed(() => (props.query ?? '').trim());

const filteredLines = computed(() => {
  const q = trimmedQuery.value.toLowerCase();
  if (!q) return parsed.value.lines;
  return parsed.value.lines.filter(l => l.plain.toLowerCase().includes(q));
});

const filteredCommands = computed(() => {
  const q = trimmedQuery.value.toLowerCase();
  if (!q) return parsed.value.commands;
  return parsed.value.commands.filter(
    c => c.command.toLowerCase().includes(q) || (c.cwd ?? '').toLowerCase().includes(q),
  );
});

function spanCss(style: AnsiStyle): CSSProperties {
  const css: CSSProperties = {};
  let fg = style.fg;
  let bg = style.bg;
  if (style.inverse) {
    const tmp = fg;
    fg = bg ?? 'var(--terminal-bg)';
    bg = tmp ?? 'var(--terminal-fg)';
  }
  if (fg) css.color = fg;
  if (bg) {
    css.backgroundColor = bg;
    css.borderRadius = '2px';
  }
  if (style.bold) css.fontWeight = '700';
  if (style.dim) css.opacity = '0.68';
  if (style.italic) css.fontStyle = 'italic';
  const deco: string[] = [];
  if (style.underline) deco.push('underline');
  if (style.strikethrough) deco.push('line-through');
  if (deco.length) css.textDecoration = deco.join(' ');
  return css;
}

function splitHighlight(text: string): Array<{ text: string; hit: boolean }> {
  const q = trimmedQuery.value;
  if (!q) return [{ text, hit: false }];
  const lower = text.toLowerCase();
  const needle = q.toLowerCase();
  const parts: Array<{ text: string; hit: boolean }> = [];
  let pos = 0;
  while (pos < text.length) {
    const idx = lower.indexOf(needle, pos);
    if (idx < 0) {
      parts.push({ text: text.slice(pos), hit: false });
      break;
    }
    if (idx > pos) parts.push({ text: text.slice(pos, idx), hit: false });
    parts.push({ text: text.slice(idx, idx + needle.length), hit: true });
    pos = idx + needle.length;
  }
  return parts;
}

function copyCleanText() {
  void copyText(parsed.value.plainText, tr('终端会话文本'));
}

function copyCommand(cmd: string) {
  void copyText(cmd, tr('终端命令'));
}
</script>

<style scoped>
.terminal-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-primary);
}
.tv-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-secondary);
  flex-shrink: 0;
}
.tv-meta-row,
.tv-tabs-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
}
.tv-title-wrap {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
}
.tv-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}
.tv-filename {
  font-weight: 600;
  font-size: 12px;
  color: var(--text-primary);
}
.tv-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}
.tv-color-toggle.active {
  border-color: color-mix(in srgb, var(--primary-color) 65%, var(--border-color));
  color: var(--primary-color);
  background: color-mix(in srgb, var(--primary-color) 10%, var(--surface-primary));
}
.tv-osc-title {
  font-size: 11px;
  color: var(--text-secondary);
  max-width: 340px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tv-screen {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 10px 14px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-primary);
  background: var(--surface-primary);
  user-select: text;
}
.terminal-viewer.color-on .tv-screen {
  background: var(--terminal-bg);
  color: var(--terminal-fg);
}
.tv-line {
  display: flex;
  align-items: baseline;
  gap: 12px;
  white-space: pre-wrap;
  word-break: break-all;
}
.tv-line:hover {
  background: var(--hover-bg);
}
.tv-ln {
  flex-shrink: 0;
  min-width: 36px;
  text-align: right;
  font-size: 11px;
  color: var(--text-secondary);
  user-select: none;
}
.terminal-viewer.color-on .tv-ln {
  color: var(--terminal-muted);
}
.tv-content {
  flex: 1;
  min-width: 0;
}
.tv-mark {
  background: var(--sev-warn-tag-bg);
  color: var(--sev-warn-tag-fg);
  border-radius: 2px;
  padding: 0 1px;
}
.tv-commands {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 8px 14px;
}
.tv-cmd-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.tv-cmd-table th,
.tv-cmd-table td {
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-color);
  text-align: left;
  vertical-align: top;
}
.tv-cmd-table th {
  color: var(--text-secondary);
  font-weight: 600;
}
.tv-cmd-num,
.tv-cmd-ln {
  width: 48px;
  color: var(--text-secondary);
}
.tv-cmd-prompt {
  max-width: 200px;
  color: var(--text-secondary);
  word-break: break-all;
}
.tv-cmd-text {
  color: var(--text-primary);
  word-break: break-all;
}
.tv-empty {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary);
}
</style>
