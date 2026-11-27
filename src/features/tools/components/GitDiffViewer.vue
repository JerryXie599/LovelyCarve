<template>
  <div class="git-diff-viewer">
    <div v-if="loading" class="gdv-state">
      <Loader2 :size="16" class="spin" />
      <span>{{ tr("正在计算文件差异…") }}</span>
    </div>

    <div v-else-if="error" class="ui-alert danger">
      <span>{{ error }}</span>
    </div>

    <div v-else-if="!diff || diff.files.length === 0" class="gdv-empty">
      {{ tr("两个版本之间无文件差异") }}
    </div>

    <template v-else>
      <div class="gdv-summary">
        <span class="gdv-range mono" :title="`${diff.baseRev} → ${diff.targetRev}`">
          {{ shortRev(diff.baseRev) }} → {{ shortRev(diff.targetRev) }}
        </span>
        <span class="gdv-stat">{{ tr("{0} 个文件变更", { 0: diff.files.length }) }}</span>
        <span class="gdv-add mono">+{{ diff.totalAdditions }}</span>
        <span class="gdv-del mono">-{{ diff.totalDeletions }}</span>
      </div>

      <div class="gdv-files">
        <article v-for="file in diff.files" :key="file.path" class="gdv-file ui-card">
          <header
            class="gdv-file-head"
            :class="{ collapsed: collapsed.has(file.path) }"
            @click="toggleCollapse(file.path)"
          >
            <ChevronRight :size="14" class="gdv-chevron" :class="{ open: !collapsed.has(file.path) }" />
            <span class="ui-badge sm" :class="statusClass(file.status)">{{ file.status }}</span>
            <span class="gdv-path mono">{{ file.path }}</span>
            <span v-if="!file.isBinary" class="gdv-counts mono">
              <span class="gdv-add">+{{ file.additions }}</span>
              <span class="gdv-del">-{{ file.deletions }}</span>
            </span>
          </header>

          <div v-if="!collapsed.has(file.path)" class="gdv-file-body">
            <div v-if="file.isBinary" class="gdv-binary">{{ tr("二进制文件变更，无法显示文本 Diff") }}</div>
            <div v-else-if="file.hunks.length === 0" class="gdv-binary">{{ tr("无文本行变化") }}</div>
            <div v-else class="gdv-hunks mono">
              <template v-for="(hunk, hi) in file.hunks" :key="hi">
                <div class="gdv-hunk-hdr">{{ hunk.header }}</div>
                <table class="gdv-table">
                  <tbody>
                    <tr v-for="(line, li) in hunk.lines" :key="li" :class="`line-${line.kind}`">
                      <td class="gdv-ln">{{ line.oldLine ?? '' }}</td>
                      <td class="gdv-ln">{{ line.newLine ?? '' }}</td>
                      <td class="gdv-sig">{{ line.kind === 'add' ? '+' : line.kind === 'delete' ? '-' : ' ' }}</td>
                      <td class="gdv-txt">{{ line.content || ' ' }}</td>
                    </tr>
                  </tbody>
                </table>
              </template>
            </div>
          </div>
        </article>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { ChevronRight, Loader2 } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import type { GitDiffResult } from '../../../lib/ipc/gitWorkspace';

defineProps<{
  diff: GitDiffResult | null;
  loading?: boolean;
  error?: string | null;
}>();

const collapsed = ref(new Set<string>());

function toggleCollapse(path: string) {
  const next = new Set(collapsed.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  collapsed.value = next;
}

function shortRev(rev: string): string {
  return /^[0-9a-f]{40}$/i.test(rev) ? rev.slice(0, 8) : rev;
}

function statusClass(status: string): string {
  if (status === 'added') return 'success';
  if (status === 'deleted') return 'danger';
  if (status === 'modified') return 'warn';
  return '';
}
</script>

<style scoped>
.git-diff-viewer {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.gdv-state,
.gdv-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 24px;
  font-size: 12px;
  color: var(--text-secondary);
}

.gdv-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-secondary);
  font-size: 12px;
}

.gdv-range {
  font-weight: 600;
  color: var(--text-primary);
}

.gdv-stat {
  color: var(--text-secondary);
}

.gdv-add {
  color: var(--sev-success-fg);
  font-weight: 600;
}

.gdv-del {
  color: var(--sev-critical-fg);
  font-weight: 600;
}

.gdv-files {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.gdv-file {
  padding: 0;
  overflow: hidden;
}

.gdv-file-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--surface-primary);
  border-bottom: 1px solid var(--border-color);
  cursor: pointer;
}

.gdv-file-head.collapsed {
  border-bottom: 0;
}

.gdv-chevron {
  color: var(--text-secondary);
  transition: transform var(--motion-fast);
}

.gdv-chevron.open {
  transform: rotate(90deg);
}

.gdv-path {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gdv-counts {
  display: flex;
  gap: 6px;
  font-size: 11px;
}

.gdv-binary {
  padding: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.gdv-hunks {
  overflow-x: auto;
  font-size: 12px;
  line-height: 1.45;
}

.gdv-hunk-hdr {
  padding: 4px 10px;
  background: color-mix(in srgb, var(--primary-color) 8%, transparent);
  border-top: 1px solid var(--border-color);
  border-bottom: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 11px;
}

.gdv-hunk-hdr:first-child {
  border-top: 0;
}

.gdv-table {
  width: 100%;
  border-collapse: collapse;
}

.gdv-ln {
  width: 1%;
  min-width: 40px;
  padding: 1px 6px;
  text-align: right;
  color: var(--text-secondary);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  vertical-align: top;
  user-select: none;
  border-right: 1px solid var(--border-color);
}

.gdv-sig {
  width: 1%;
  min-width: 22px;
  padding: 1px 2px;
  text-align: center;
  vertical-align: top;
  user-select: none;
}

.gdv-txt {
  width: 98%;
  padding: 1px 10px 1px 4px;
  white-space: pre-wrap;
  word-break: break-word;
}

.line-add {
  background: color-mix(in srgb, var(--sev-success-fg) 13%, transparent);
}

.line-add .gdv-sig {
  color: var(--sev-success-fg);
}

.line-delete {
  background: color-mix(in srgb, var(--sev-critical-fg) 13%, transparent);
}

.line-delete .gdv-sig {
  color: var(--sev-critical-fg);
}
</style>
