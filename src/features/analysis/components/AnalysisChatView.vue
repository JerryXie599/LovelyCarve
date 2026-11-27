<template>
  <div class="chat-shell">
    <aside class="chat-threads" :aria-label="tr('会话列表')">
      <div class="chat-thread-search">
        <Search :size="13" aria-hidden="true" />
        <input
          v-model="threadSearch"
          type="search"
          class="chat-search-input"
          :placeholder="tr('筛选会话名称或账号…')"
          :aria-label="tr('筛选会话名称或账号')"
        />
        <span v-if="threadsTotal > 0" class="chat-thread-count mono">{{ threadsTotal }}</span>
      </div>

      <div v-if="threadsLoading" class="ui-empty state-box" role="status">
        <span class="ui-empty-icon accent"><Loader2 :size="18" class="spin" /></span>
        <p class="ui-empty-desc">{{ tr('正在汇总会话…') }}</p>
      </div>
      <div v-else-if="threadsError" class="ui-empty state-box error" role="alert">
        <p class="ui-empty-desc">{{ tr('会话加载失败：{0}', { 0: threadsError }) }}</p>
        <div class="ui-empty-actions">
          <button class="ui-btn sm" type="button" @click="retryThreads">{{ tr('重试') }}</button>
        </div>
      </div>
      <div v-else-if="filteredThreads.length === 0" class="ui-empty state-box">
        <p class="ui-empty-desc">{{ tr('暂无匹配的会话') }}</p>
      </div>
      <div v-else class="chat-thread-list" role="listbox">
        <button
          v-for="thread in filteredThreads"
          :key="thread.threadId"
          type="button"
          class="chat-thread-item"
          role="option"
          :aria-selected="thread.threadId === selectedThreadId"
          :class="{ active: thread.threadId === selectedThreadId }"
          @click="selectThread(thread.threadId)"
        >
          <span class="thread-avatar">{{ avatarChar(thread.threadTitle || thread.threadId) }}</span>
          <div class="thread-main">
            <div class="thread-top">
              <span class="thread-name" :title="thread.threadTitle || thread.threadId">
                {{ thread.threadTitle || thread.threadId }}
              </span>
              <span v-if="thread.lastTime" class="thread-time mono">
                {{ shortThreadTime(thread.lastTime) }}
              </span>
            </div>
            <div class="thread-bottom">
              <span class="thread-snippet">
                <template v-if="thread.lastSender">{{ thread.lastSender }}: </template>{{ thread.lastMessage || tr('(无正文)') }}
              </span>
              <span class="thread-badge mono">{{ thread.messageCount }}</span>
            </div>
          </div>
        </button>
      </div>
    </aside>

    <section class="chat-pane" :aria-label="tr('对话消息流')">
      <header v-if="selectedThread" class="chat-pane-header">
        <div class="pane-title-wrap">
          <MessageSquare :size="15" aria-hidden="true" />
          <span class="pane-title">{{ selectedThread.threadTitle || selectedThread.threadId }}</span>
          <span class="pane-id mono">{{ selectedThread.threadId }}</span>
        </div>
        <span class="pane-count">
          {{ tr('{0} 条消息', { 0: messagesTotal || selectedThread.messageCount }) }}
        </span>
      </header>

      <div v-if="!selectedThreadId" class="ui-empty state-box">
        <span class="ui-empty-icon"><MessageSquare :size="24" /></span>
        <p class="ui-empty-title">{{ tr('请选择左侧会话') }}</p>
      </div>
      <div v-else-if="messagesLoading" class="ui-empty state-box" role="status">
        <span class="ui-empty-icon accent"><Loader2 :size="20" class="spin" /></span>
        <p class="ui-empty-desc">{{ tr('正在读取消息记录…') }}</p>
      </div>
      <div v-else-if="messagesError" class="ui-empty state-box error" role="alert">
        <p class="ui-empty-desc">{{ tr('消息加载失败：{0}', { 0: messagesError }) }}</p>
        <div class="ui-empty-actions">
          <button class="ui-btn sm" type="button" @click="retryMessages">{{ tr('重试') }}</button>
        </div>
      </div>
      <div v-else-if="messages.length === 0" class="ui-empty state-box">
        <p class="ui-empty-desc">{{ tr('该会话暂无可显示消息') }}</p>
      </div>
      <div v-else class="chat-messages-scroll">
        <template v-for="(item, index) in messageItems" :key="item.record.recordId">
          <div v-if="item.dateDivider" class="chat-date-divider">
            <span>{{ item.dateDivider }}</span>
          </div>

          <div
            class="chat-row"
            :class="{ outgoing: item.outgoing, selected: item.record.recordId === selectedRecordId }"
            :data-record-id="item.record.recordId"
          >
            <button
              type="button"
              class="chat-bubble"
              :class="{ outgoing: item.outgoing, selected: item.record.recordId === selectedRecordId }"
              :title="tr('点击在右侧查看已读回执、投票明细与原始字段')"
              @click="emit('openRecord', item.record.recordId)"
            >
              <div v-if="!item.outgoing && item.sender" class="bubble-sender">
                {{ item.sender }}
              </div>

              <div v-if="item.replyTo" class="bubble-reply">
                <Reply :size="12" aria-hidden="true" />
                <div class="bubble-reply-body">
                  <span v-if="item.replyTo.sender" class="reply-sender">{{ item.replyTo.sender }}</span>
                  <span class="reply-text">
                    {{ item.replyTo.content || item.replyTo.msgType || tr('引用消息') }}
                  </span>
                </div>
              </div>

              <div v-if="item.msgTypeLabel && (!item.content || item.hasMedia)" class="bubble-type-chip">
                <Paperclip v-if="item.hasMedia" :size="12" aria-hidden="true" />
                <span>{{ item.msgTypeLabel }}</span>
              </div>

              <div v-if="bubbleImageUrls[item.record.recordId]" class="bubble-image-wrap">
                <img
                  :src="bubbleImageUrls[item.record.recordId]"
                  :alt="item.content || mediaBasename(item.mediaPath)"
                  class="bubble-image"
                  loading="lazy"
                />
              </div>

              <p v-if="item.content" class="bubble-text">{{ item.content }}</p>

              <div v-if="item.mediaPath && !bubbleImageUrls[item.record.recordId]" class="bubble-media mono">
                <Paperclip :size="12" aria-hidden="true" />
                <span>{{ mediaBasename(item.mediaPath) }}</span>
              </div>

              <AnalysisPollCard v-if="item.poll" :poll="item.poll" />

              <div v-if="item.reactions.length" class="bubble-reactions">
                <span
                  v-for="(reaction, rIdx) in item.reactions"
                  :key="`${rIdx}:${reaction.emoji}:${reaction.sender}`"
                  class="reaction-chip"
                  :title="reaction.sender ? `${reaction.sender}${reaction.time ? ` · ${reaction.time}` : ''}` : undefined"
                >
                  <span>{{ reaction.emoji }}</span>
                  <span v-if="reaction.sender" class="reaction-who">{{ reaction.sender }}</span>
                </span>
              </div>

              <div class="bubble-footer">
                <span v-if="item.timeLabel" class="bubble-time mono">{{ item.timeLabel }}</span>
                <span
                  v-if="item.badge"
                  class="bubble-receipt"
                  :class="`tone-${item.badge.tone}`"
                >
                  <CheckCheck v-if="item.badge.tone === 'read'" :size="12" aria-hidden="true" />
                  <Check v-else-if="item.badge.tone === 'delivered'" :size="12" aria-hidden="true" />
                  <span>{{ item.badge.label }}</span>
                </span>
              </div>
            </button>
          </div>

          <div
            v-if="index === messageItems.length - 1 && hasMoreMessages"
            class="chat-load-more"
          >
            <button
              type="button"
              class="ui-btn ghost sm"
              :disabled="messagesLoadingMore"
              @click="loadMoreMessages"
            >
              <Loader2 v-if="messagesLoadingMore" :size="13" class="spin" />
              <span>{{ tr('加载更多消息 ({0}/{1})', { 0: messages.length, 1: messagesTotal }) }}</span>
            </button>
          </div>
        </template>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, toRef, watch } from 'vue';
import { Check, CheckCheck, Loader2, MessageSquare, Paperclip, Reply, Search } from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import type { AnalysisRecord, AnalysisResultNode, RecordChatLayout } from '../../../lib/ipc/analysisResults';
import { fsSourceLocate } from '../../../lib/ipc/fs/query';
import { fsRead } from '../../../lib/ipc/fs/read';
import { useAnalysisChat } from '../composables/useAnalysisChat';
import { analysisSources } from '../model/analysisSource';
import {
  extractDateKey,
  formatBubbleTime,
  isOutgoingMessage,
  parsePoll,
  parseReactions,
  parseReceipts,
  parseReplyTo,
  receiptBadgeInfo,
} from '../model/chatView';
import AnalysisPollCard from './AnalysisPollCard.vue';

const props = defineProps<{
  node: AnalysisResultNode | null;
  chatLayout: RecordChatLayout;
  active: boolean;
  keyword: string;
  filters: Record<string, string[]>;
  selectedRecordId: string | null;
}>();

const emit = defineEmits<{
  openRecord: [recordId: string];
}>();

const {
  filteredThreads,
  threadsTotal,
  threadsLoading,
  threadsError,
  threadSearch,
  selectedThreadId,
  selectedThread,
  messages,
  messagesTotal,
  messagesLoading,
  messagesLoadingMore,
  messagesError,
  hasMoreMessages,
  selectThread,
  loadMoreMessages,
  retryThreads,
  retryMessages,
} = useAnalysisChat({
  node: toRef(props, 'node'),
  chatLayout: toRef(props, 'chatLayout'),
  active: toRef(props, 'active'),
  keyword: toRef(props, 'keyword'),
  filters: toRef(props, 'filters'),
});

const messageItems = computed(() => {
  const layout = props.chatLayout;
  let prevDate = '';
  return messages.value.map(record => {
    const rawTime = layout.time ? record.values[layout.time] : undefined;
    const dateKey = extractDateKey(rawTime);
    const dateDivider = dateKey && dateKey !== prevDate ? dateKey : '';
    if (dateKey) prevDate = dateKey;

    const outgoing = isOutgoingMessage(record, layout);
    const senderRaw = layout.sender ? record.values[layout.sender] : undefined;
    const sender = typeof senderRaw === 'string' && senderRaw.trim() ? senderRaw.trim() : '';
    const contentRaw = record.values[layout.content];
    const content = typeof contentRaw === 'string' ? contentRaw.trim() : contentRaw != null ? String(contentRaw) : '';
    const msgTypeRaw = layout.msgType ? record.values[layout.msgType] : undefined;
    const msgTypeLabel = typeof msgTypeRaw === 'string' && msgTypeRaw.trim() ? msgTypeRaw.trim() : '';
    const mediaRaw = layout.media ? record.values[layout.media] : undefined;
    const fallbackMedia =
      typeof record.values.media_path === 'string' && record.values.media_path.trim()
        ? record.values.media_path.trim()
        : typeof record.values.fmedia === 'string' && record.values.fmedia.trim()
          ? record.values.fmedia.trim()
          : '';
    const mediaPath =
      typeof mediaRaw === 'string' && mediaRaw.trim() ? mediaRaw.trim() : fallbackMedia;
    const receipts = layout.receipts ? parseReceipts(record.values[layout.receipts]) : null;
    const statusRaw = layout.status ? record.values[layout.status] : undefined;
    const badge = receiptBadgeInfo(receipts, statusRaw, outgoing, tr);
    const poll = layout.poll ? parsePoll(record.values[layout.poll]) : null;
    const reactions = layout.reactions ? parseReactions(record.values[layout.reactions]) : [];
    const replyTo = layout.replyTo ? parseReplyTo(record.values[layout.replyTo]) : null;

    return {
      record,
      dateDivider,
      outgoing,
      sender,
      content,
      msgTypeLabel,
      hasMedia: Boolean(mediaPath),
      mediaPath,
      timeLabel: formatBubbleTime(rawTime),
      badge,
      poll,
      reactions,
      replyTo,
    };
  });
});

const CHAT_IMAGE_EXTS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'heic', 'heif', 'thumb']);
const bubbleImageUrls = reactive<Record<string, string>>({});
const attemptedRecordIds = new Set<string>();

function cleanPath(p: string): string {
  return p
    .replace(/\\/g, '/')
    .replace(/^(\/|root\/|root\\)+/i, '')
    .replace(/^\/+/g, '')
    .trim();
}

function isImagePath(p: string): boolean {
  const cp = cleanPath(p);
  if (!cp) return false;
  const ext = cp.split('.').pop()?.toLowerCase() ?? '';
  return CHAT_IMAGE_EXTS.has(ext);
}

function imageMime(p: string): string {
  const ext = cleanPath(p).split('.').pop()?.toLowerCase() ?? '';
  if (ext === 'png') return 'image/png';
  if (ext === 'gif') return 'image/gif';
  if (ext === 'webp') return 'image/webp';
  if (ext === 'svg') return 'image/svg+xml';
  if (ext === 'bmp') return 'image/bmp';
  if (ext === 'heic' || ext === 'heif') return 'image/heic';
  return 'image/jpeg';
}

async function resolveBubbleImage(record: AnalysisRecord, rawMediaPath: string): Promise<void> {
  const recordId = record.recordId;
  if (attemptedRecordIds.has(recordId)) return;
  attemptedRecordIds.add(recordId);

  const imageId = record.imageId || props.node?.imageId || '';
  if (!imageId) return;

  const recSources = analysisSources(record, imageId);
  const rootId = recSources[0]?.target.rootId || 'root';

  const sourcePrefixes: string[] = [];
  const candidatePaths: string[] = [];

  for (const src of recSources) {
    const st = src?.target as ({ path?: string } | undefined);
    const sp = cleanPath(st?.path || '');
    if (!sp) continue;
    if (isImagePath(sp)) {
      candidatePaths.push(sp);
    }
    const parts = sp.split('/');
    if (parts.length > 1) {
      parts.pop();
      sourcePrefixes.push(parts.join('/'));
      if (parts.length > 1) {
        parts.pop();
        sourcePrefixes.push(parts.join('/'));
      }
    }
  }

  const rawPaths = [
    rawMediaPath,
    typeof record.values.media_path === 'string' ? record.values.media_path : '',
    typeof record.values.fmedia === 'string' ? record.values.fmedia : '',
    typeof record.values.path === 'string' ? record.values.path : '',
  ].filter(Boolean);

  for (const rp of rawPaths) {
    const cp = cleanPath(rp);
    if (!cp || !isImagePath(cp)) continue;
    candidatePaths.push(cp);
    for (const sp of sourcePrefixes) {
      candidatePaths.push(`${sp}/${cp}`);
      candidatePaths.push(`${sp}/Message/${cp}`);
    }
    candidatePaths.push(
      `AppDomainGroup-group.net.whatsapp.WhatsApp.shared/Message/${cp}`,
      `AppDomainGroup-group.net.whatsapp.WhatsApp.shared/${cp}`,
      `Message/${cp}`,
      `Library/${cp}`,
    );
  }

  const uniqueCandidates = Array.from(new Set(candidatePaths.map(cleanPath).filter(Boolean)));
  if (uniqueCandidates.length === 0) return;

  for (const path of uniqueCandidates) {
    try {
      const loc = await fsSourceLocate({ imageId, rootId, path });
      const nodeId = loc?.file_id || loc?.node_id;
      if (!nodeId) continue;
      const buf = await fsRead({ imageId, nodeId });
      if (!buf || buf.byteLength === 0) continue;
      const mime = imageMime(path);
      const rawBlob = new Blob([new Uint8Array(buf)], { type: mime });
      if (mime === 'image/heic') {
        try {
          const heic2any = (await import('heic2any')).default;
          const converted = await heic2any({ blob: rawBlob, toType: 'image/jpeg', quality: 0.82 });
          const jpegBlob = Array.isArray(converted) ? converted[0] : converted;
          bubbleImageUrls[recordId] = URL.createObjectURL(jpegBlob);
          return;
        } catch {
          bubbleImageUrls[recordId] = URL.createObjectURL(rawBlob);
          return;
        }
      }
      bubbleImageUrls[recordId] = URL.createObjectURL(rawBlob);
      return;
    } catch {
      // Try next candidate path
    }
  }
}

watch(
  messageItems,
  items => {
    for (const item of items) {
      if (
        (item.mediaPath && isImagePath(item.mediaPath)) ||
        item.msgTypeLabel === '图片' ||
        item.msgTypeLabel === '贴纸'
      ) {
        void resolveBubbleImage(item.record, item.mediaPath);
      }
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  for (const url of Object.values(bubbleImageUrls)) {
    URL.revokeObjectURL(url);
  }
});

function avatarChar(title: string): string {
  const trimmed = title.trim();
  if (!trimmed) return '?';
  return [...trimmed][0].toUpperCase();
}

function shortThreadTime(iso: string): string {
  const m = iso.match(/^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2})/);
  if (!m) return iso;
  return `${m[1].slice(5)} ${m[2]}`;
}

function mediaBasename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}
</script>

<style scoped>
.chat-shell {
  display: grid;
  grid-template-columns: 290px minmax(0, 1fr);
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: var(--surface-primary);
}

.chat-threads {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--border-color);
  background: var(--surface-tertiary);
}

.chat-thread-search {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.chat-search-input {
  flex: 1 1 auto;
  min-width: 0;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 12.5px;
  outline: none;
}

.chat-thread-count {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--chip-count-bg);
}

.chat-thread-list {
  flex: 1 1 auto;
  overflow-y: auto;
  padding: 4px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.chat-thread-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 8px 9px;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  background: transparent;
  text-align: left;
  cursor: pointer;
  color: var(--text-primary);
}

.chat-thread-item:hover {
  background: var(--hover-bg);
}

.chat-thread-item.active {
  background: var(--primary-soft);
  border-color: color-mix(in srgb, var(--primary-color) 35%, transparent);
}

.thread-avatar {
  flex: 0 0 auto;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 600;
  background: var(--primary-soft);
  color: var(--primary-color);
}

.thread-main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.thread-top,
.thread-bottom {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}

.thread-name {
  font-size: 12.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.thread-time {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--text-secondary);
}

.thread-snippet {
  font-size: 11.5px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.thread-badge {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--text-secondary);
  padding: 0 5px;
  border-radius: 999px;
  background: var(--chip-count-bg);
}

.chat-pane {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.chat-pane-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 9px 14px;
  border-bottom: 1px solid var(--border-color);
  background: var(--surface-tertiary);
}

.pane-title-wrap {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  color: var(--text-primary);
}

.pane-title {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pane-id {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pane-count {
  flex: 0 0 auto;
  font-size: 11.5px;
  color: var(--text-secondary);
}

.chat-messages-scroll {
  flex: 1 1 auto;
  overflow-y: auto;
  padding: 14px 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.chat-date-divider {
  display: flex;
  justify-content: center;
  margin: 8px 0 4px;
}

.chat-date-divider span {
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--chip-count-bg);
}

.chat-row {
  display: flex;
  justify-content: flex-start;
}

.chat-row.outgoing {
  justify-content: flex-end;
}

.chat-bubble {
  max-width: min(74%, 640px);
  min-width: 140px;
  padding: 8px 12px;
  border-radius: 12px 12px 12px 4px;
  border: 1px solid var(--border-color);
  background: var(--surface-secondary);
  color: var(--text-primary);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.14s ease, box-shadow 0.14s ease;
}

.chat-bubble.outgoing {
  border-radius: 12px 12px 4px 12px;
  background: color-mix(in srgb, var(--primary-color) 11%, var(--surface-secondary));
  border-color: color-mix(in srgb, var(--primary-color) 28%, var(--border-color));
}

.chat-bubble:hover {
  border-color: var(--primary-color);
}

.chat-bubble.selected {
  border-color: var(--primary-color);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--primary-color) 22%, transparent);
}

.bubble-sender {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--primary-color);
  margin-bottom: 4px;
}

.bubble-reply {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 5px 8px;
  margin-bottom: 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  font-size: 11.5px;
  color: var(--text-secondary);
}

.bubble-reply-body {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.reply-sender {
  font-weight: 600;
  color: var(--text-primary);
}

.reply-text {
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.bubble-type-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: 3px;
}

.bubble-image-wrap {
  margin: 4px 0 6px;
  border-radius: var(--radius-md);
  overflow: hidden;
  max-width: 260px;
  background: var(--surface-tertiary);
}

.bubble-image {
  display: block;
  max-width: 260px;
  max-height: 240px;
  width: auto;
  height: auto;
  object-fit: cover;
  border-radius: var(--radius-md);
}

.bubble-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.48;
  white-space: pre-wrap;
  word-break: break-word;
}

.bubble-media {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-top: 5px;
  padding: 3px 7px;
  border-radius: var(--radius-sm);
  background: var(--surface-tertiary);
  font-size: 11px;
  color: var(--text-secondary);
}

.bubble-reactions {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}

.reaction-chip {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 6px;
  border-radius: 999px;
  border: 1px solid var(--border-color);
  background: var(--surface-primary);
  font-size: 11.5px;
}

.reaction-who {
  font-size: 10.5px;
  color: var(--text-secondary);
}

.bubble-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 5px;
  font-size: 10.5px;
}

.bubble-time {
  color: var(--text-secondary);
}

.bubble-receipt {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-weight: 500;
}

.bubble-receipt.tone-read {
  color: var(--sev-info-fg);
}

.bubble-receipt.tone-delivered {
  color: var(--text-secondary);
}

.bubble-receipt.tone-pending {
  color: var(--sev-warn-fg);
}

.bubble-receipt.tone-info {
  color: var(--text-secondary);
}

.chat-load-more {
  display: flex;
  justify-content: center;
  padding-top: 6px;
}
</style>
