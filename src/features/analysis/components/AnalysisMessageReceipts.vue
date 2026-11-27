<template>
  <div class="receipts-panel">
    <div class="receipts-stats">
      <span v-if="receipts.readCount > 0 || receipts.read.length" class="stat-pill is-read">
        <CheckCheck :size="12" aria-hidden="true" />
        {{ tr('已读 {0}', { 0: receipts.readCount || receipts.read.length }) }}
      </span>
      <span v-if="receipts.playedCount > 0 || receipts.played.length" class="stat-pill is-read">
        <Play :size="12" aria-hidden="true" />
        {{ tr('已播放 {0}', { 0: receipts.playedCount || receipts.played.length }) }}
      </span>
      <span v-if="receipts.deliveredCount > 0 || receipts.delivered.length" class="stat-pill is-delivered">
        <Check :size="12" aria-hidden="true" />
        {{ tr('已送达未读 {0}', { 0: receipts.deliveredCount || receipts.delivered.length }) }}
      </span>
      <span v-if="receipts.unreadCount > 0 || receipts.unread.length" class="stat-pill is-unread">
        <Clock :size="12" aria-hidden="true" />
        {{ tr('尚未读取 {0}', { 0: receipts.unreadCount || receipts.unread.length }) }}
      </span>
      <span v-if="receipts.summary && !hasDetailedLists" class="stat-pill">
        {{ receipts.summary }}
      </span>
    </div>

    <div v-if="receipts.played.length" class="receipt-group">
      <div class="receipt-group-title is-read">{{ tr('已播放成员') }} ({{ receipts.played.length }})</div>
      <div v-for="(item, idx) in receipts.played" :key="entryKey('played', item, idx)" class="receipt-row">
        <div class="receipt-person">
          <span class="receipt-name">{{ displayPerson(item) }}</span>
          <span v-if="item.jid && item.jid !== displayPerson(item)" class="receipt-jid mono">{{ item.jid }}</span>
        </div>
        <div class="receipt-times mono">
          <span v-if="item.playedAt || item.time">{{ item.playedAt || item.time }}</span>
          <span v-if="item.deliveredAt" class="sub-time">{{ tr('送达：{0}', { 0: item.deliveredAt }) }}</span>
        </div>
      </div>
    </div>

    <div v-if="receipts.read.length" class="receipt-group">
      <div class="receipt-group-title is-read">{{ tr('已读成员') }} ({{ receipts.read.length }})</div>
      <div v-for="(item, idx) in receipts.read" :key="entryKey('read', item, idx)" class="receipt-row">
        <div class="receipt-person">
          <span class="receipt-name">{{ displayPerson(item) }}</span>
          <span v-if="item.jid && item.jid !== displayPerson(item)" class="receipt-jid mono">{{ item.jid }}</span>
        </div>
        <div class="receipt-times mono">
          <span v-if="item.readAt || item.time">{{ item.readAt || item.time }}</span>
          <span v-else>{{ item.status || tr('已读（无精确时间）') }}</span>
          <span v-if="item.deliveredAt" class="sub-time">{{ tr('送达：{0}', { 0: item.deliveredAt }) }}</span>
        </div>
      </div>
    </div>

    <div v-if="receipts.delivered.length" class="receipt-group">
      <div class="receipt-group-title is-delivered">{{ tr('已送达但未读') }} ({{ receipts.delivered.length }})</div>
      <div v-for="(item, idx) in receipts.delivered" :key="entryKey('delivered', item, idx)" class="receipt-row">
        <div class="receipt-person">
          <span class="receipt-name">{{ displayPerson(item) }}</span>
          <span v-if="item.jid && item.jid !== displayPerson(item)" class="receipt-jid mono">{{ item.jid }}</span>
        </div>
        <div class="receipt-times mono">
          <span>{{ item.deliveredAt || item.time || tr('已送达') }}</span>
        </div>
      </div>
    </div>

    <div v-if="receipts.unread.length" class="receipt-group">
      <div class="receipt-group-title is-unread">{{ tr('未读成员') }} ({{ receipts.unread.length }})</div>
      <div v-for="(item, idx) in receipts.unread" :key="entryKey('unread', item, idx)" class="receipt-row">
        <div class="receipt-person">
          <span class="receipt-name">{{ displayPerson(item) }}</span>
          <span v-if="item.jid && item.jid !== displayPerson(item)" class="receipt-jid mono">{{ item.jid }}</span>
        </div>
        <div class="receipt-times mono">
          <span>{{ item.deliveredAt ? tr('已送达 {0}', { 0: item.deliveredAt }) : tr('未读取/待送达') }}</span>
        </div>
      </div>
    </div>

    <p v-if="!hasDetailedLists" class="receipt-empty">
      {{ tr('此消息仅含回执汇总状态，未包含逐成员明细。') }}
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { Check, CheckCheck, Clock, Play } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import type { ParsedReceipts, ReceiptEntry } from '../model/chatView';

const props = defineProps<{
  receipts: ParsedReceipts;
}>();

const hasDetailedLists = computed(() =>
  props.receipts.played.length > 0
  || props.receipts.read.length > 0
  || props.receipts.delivered.length > 0
  || props.receipts.unread.length > 0
);

function displayPerson(item: ReceiptEntry): string {
  if (item.name && item.phone && !item.name.includes(item.phone)) {
    return `${item.name} (${item.phone})`;
  }
  return item.name || item.phone || item.jid || tr('未知成员');
}

function entryKey(kind: string, item: ReceiptEntry, idx: number): string {
  return `${kind}:${item.jid ?? item.name ?? ''}:${idx}`;
}
</script>

<style scoped>
.receipts-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.receipts-stats {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.stat-pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 11.5px;
  font-weight: 500;
  border: 1px solid var(--border-color);
  background: var(--surface-secondary);
  color: var(--text-primary);
}

.stat-pill.is-read {
  color: var(--sev-info-fg);
  border-color: var(--sev-info-border);
  background: var(--sev-info-bg);
}

.stat-pill.is-delivered {
  color: var(--text-secondary);
}

.stat-pill.is-unread {
  color: var(--sev-warn-fg);
  border-color: var(--sev-warn-border);
  background: var(--sev-warn-bg);
}

.receipt-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
  background: var(--surface-secondary);
}

.receipt-group-title {
  font-size: 11.5px;
  font-weight: 600;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--border-color);
}

.receipt-group-title.is-read {
  color: var(--sev-info-fg);
}

.receipt-group-title.is-delivered {
  color: var(--text-secondary);
}

.receipt-group-title.is-unread {
  color: var(--sev-warn-fg);
}

.receipt-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 4px 0;
  border-bottom: 1px dashed var(--border-color);
}

.receipt-row:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

.receipt-person {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.receipt-name {
  font-size: 12.5px;
  color: var(--text-primary);
  word-break: break-word;
}

.receipt-jid {
  font-size: 10.5px;
  color: var(--text-secondary);
  word-break: break-all;
}

.receipt-times {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  font-size: 11px;
  color: var(--text-primary);
}

.sub-time {
  font-size: 10.5px;
  color: var(--text-secondary);
}

.receipt-empty {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
