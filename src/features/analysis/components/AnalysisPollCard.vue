<template>
  <div class="poll-card" :class="{ compact: !expanded }">
    <div class="poll-header">
      <div class="poll-title-row">
        <BarChart3 :size="14" class="poll-icon" aria-hidden="true" />
        <span class="poll-question">{{ poll.question || tr('投票') }}</span>
      </div>
      <div class="poll-meta">
        <span class="poll-tag">
          {{ poll.allowMultiple ? tr('多选投票') : tr('单选投票') }}
        </span>
        <span class="poll-count">
          {{ poll.totalVoters != null
            ? tr('{0} 人参与 · {1} 票', { 0: poll.totalVoters, 1: poll.totalVotes })
            : tr('共 {0} 票', { 0: poll.totalVotes }) }}
        </span>
      </div>
    </div>

    <div class="poll-options">
      <div v-for="(opt, idx) in poll.options" :key="opt.id ?? `${idx}:${opt.text}`" class="poll-option">
        <div class="poll-option-bar-wrap">
          <div
            class="poll-option-bar"
            :style="{ width: `${Math.min(100, Math.max(0, opt.percent ?? 0))}%` }"
          />
          <div class="poll-option-row">
            <span class="poll-option-text">{{ opt.text || tr('选项 {0}', { 0: idx + 1 }) }}</span>
            <span class="poll-option-stat mono">
              {{ opt.votes }} {{ tr('票') }}
              <template v-if="opt.percent != null">({{ opt.percent.toFixed(1) }}%)</template>
            </span>
          </div>
        </div>

        <div v-if="opt.voters.length" class="poll-voters">
          <span v-if="!expanded" class="poll-voters-inline">
            {{ voterSummary(opt.voters) }}
          </span>
          <ul v-else class="poll-voters-list">
            <li v-for="(voter, vIdx) in opt.voters" :key="voter.jid ?? `${vIdx}:${voter.name}`" class="poll-voter-item">
              <span class="poll-voter-name">{{ voter.name || voter.jid || tr('未知成员') }}</span>
              <span v-if="voter.time" class="poll-voter-time mono">{{ voter.time }}</span>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { BarChart3 } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import type { ParsedPoll, PollVoter } from '../model/chatView';

defineProps<{
  poll: ParsedPoll;
  expanded?: boolean;
}>();

function voterSummary(voters: PollVoter[]): string {
  const names = voters.map(v => v.name || v.jid || tr('未知成员'));
  if (names.length <= 4) return names.join('、');
  return `${names.slice(0, 4).join('、')} ${tr('等 {0} 人', { 0: names.length })}`;
}
</script>

<style scoped>
.poll-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: var(--surface-secondary);
  border: 1px solid var(--border-color);
  min-width: 220px;
}

.poll-card.compact {
  margin-top: 6px;
}

.poll-header {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.poll-title-row {
  display: flex;
  align-items: flex-start;
  gap: 6px;
}

.poll-icon {
  flex: 0 0 auto;
  margin-top: 2px;
  color: var(--primary-color);
}

.poll-question {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.35;
  word-break: break-word;
}

.poll-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--text-secondary);
}

.poll-tag {
  padding: 1px 6px;
  border-radius: 999px;
  border: 1px solid var(--border-color);
  font-size: 10px;
}

.poll-options {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.poll-option {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.poll-option-bar-wrap {
  position: relative;
  overflow: hidden;
  border-radius: 6px;
  border: 1px solid var(--border-color);
  background: var(--surface-tertiary);
}

.poll-option-bar {
  position: absolute;
  inset: 0 auto 0 0;
  background: var(--primary-soft);
  pointer-events: none;
  transition: width 0.2s ease;
}

.poll-option-row {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  font-size: 12px;
}

.poll-option-text {
  color: var(--text-primary);
  font-weight: 500;
  word-break: break-word;
}

.poll-option-stat {
  flex: 0 0 auto;
  font-size: 11px;
  color: var(--text-secondary);
}

.poll-voters-inline {
  display: block;
  padding-left: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.35;
}

.poll-voters-list {
  list-style: none;
  margin: 2px 0 0;
  padding: 4px 8px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-left: 1px solid var(--border-color);
}

.poll-voter-item {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  font-size: 11.5px;
}

.poll-voter-name {
  color: var(--text-primary);
}

.poll-voter-time {
  font-size: 11px;
  color: var(--text-secondary);
}
</style>
