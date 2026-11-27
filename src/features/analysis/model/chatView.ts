import type { AnalysisRecord, RecordChatLayout } from '../../../lib/ipc/analysisResults';

export interface ReceiptEntry {
  jid?: string;
  name?: string;
  phone?: string;
  time?: string;
  deliveredAt?: string;
  readAt?: string;
  playedAt?: string;
  status?: string;
}

export interface ParsedReceipts {
  summary?: string;
  mode?: string;
  isGroup: boolean;
  totalMembers?: number;
  readCount: number;
  playedCount: number;
  deliveredCount: number;
  unreadCount: number;
  read: ReceiptEntry[];
  played: ReceiptEntry[];
  delivered: ReceiptEntry[];
  unread: ReceiptEntry[];
}

export interface PollVoter {
  jid?: string;
  name?: string;
  time?: string;
}

export interface PollOption {
  id?: string;
  text: string;
  votes: number;
  percent?: number;
  voters: PollVoter[];
}

export interface ParsedPoll {
  question: string;
  allowMultiple: boolean;
  totalVotes: number;
  totalVoters?: number;
  options: PollOption[];
}

export interface ChatReaction {
  emoji: string;
  sender?: string;
  jid?: string;
  time?: string;
}

export interface ReplyContext {
  sender?: string;
  content?: string;
  msgType?: string;
  stanzaId?: string;
}

export function parseStructuredField(raw: unknown): unknown {
  if (raw == null || raw === '') return null;
  if (typeof raw === 'object') return raw;
  if (typeof raw === 'string') {
    const trimmed = raw.trim();
    if (!trimmed) return null;
    if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
      try {
        return JSON.parse(trimmed);
      } catch {
        return null;
      }
    }
  }
  return null;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function asString(value: unknown): string | undefined {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return trimmed ? trimmed : undefined;
  }
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return undefined;
}

function asNumber(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim()) {
    const num = Number(value);
    if (Number.isFinite(num)) return num;
  }
  return undefined;
}

function parseReceiptEntries(raw: unknown): ReceiptEntry[] {
  if (!Array.isArray(raw)) return [];
  const entries: ReceiptEntry[] = [];
  for (const item of raw) {
    const obj = asRecord(item);
    if (!obj) {
      const label = asString(item);
      if (label) entries.push({ name: label });
      continue;
    }
    entries.push({
      jid: asString(obj.jid),
      name: asString(obj.name) ?? asString(obj.display_name) ?? asString(obj.displayName),
      phone: asString(obj.phone),
      time: asString(obj.time) ?? asString(obj.read_at) ?? asString(obj.readAt)
        ?? asString(obj.played_at) ?? asString(obj.playedAt)
        ?? asString(obj.delivered_at) ?? asString(obj.deliveredAt),
      deliveredAt: asString(obj.delivered_at) ?? asString(obj.deliveredAt),
      readAt: asString(obj.read_at) ?? asString(obj.readAt),
      playedAt: asString(obj.played_at) ?? asString(obj.playedAt),
      status: asString(obj.status),
    });
  }
  return entries;
}

export function parseReceipts(raw: unknown): ParsedReceipts | null {
  const obj = asRecord(parseStructuredField(raw));
  if (!obj) return null;
  const read = parseReceiptEntries(obj.read);
  const played = parseReceiptEntries(obj.played);
  const delivered = parseReceiptEntries(obj.delivered);
  const unread = parseReceiptEntries(obj.unread ?? obj.pending);
  const readCount = asNumber(obj.read_count ?? obj.readCount) ?? read.length;
  const playedCount = asNumber(obj.played_count ?? obj.playedCount) ?? played.length;
  const deliveredCount = asNumber(obj.delivered_count ?? obj.deliveredCount) ?? delivered.length;
  const unreadCount = asNumber(obj.unread_count ?? obj.unreadCount) ?? unread.length;
  const totalMembers = asNumber(obj.total_members ?? obj.totalMembers);
  const summary = asString(obj.summary);
  const mode = asString(obj.mode);
  const isGroup = Boolean(obj.is_group ?? obj.isGroup ?? (totalMembers != null && totalMembers > 2));
  if (
    !summary &&
    read.length === 0 &&
    played.length === 0 &&
    delivered.length === 0 &&
    unread.length === 0 &&
    readCount === 0 &&
    playedCount === 0 &&
    deliveredCount === 0 &&
    unreadCount === 0
  ) {
    return null;
  }
  return {
    summary,
    mode,
    isGroup,
    totalMembers,
    readCount,
    playedCount,
    deliveredCount,
    unreadCount,
    read,
    played,
    delivered,
    unread,
  };
}

export function parsePoll(raw: unknown): ParsedPoll | null {
  const obj = asRecord(parseStructuredField(raw));
  if (!obj) return null;
  const question = asString(obj.question) ?? asString(obj.name) ?? asString(obj.title);
  const rawOptions = Array.isArray(obj.options) ? obj.options : [];
  if (!question && rawOptions.length === 0) return null;
  const options: PollOption[] = [];
  for (const opt of rawOptions) {
    const rec = asRecord(opt);
    if (!rec) {
      const text = asString(opt);
      if (text) options.push({ text, votes: 0, voters: [] });
      continue;
    }
    const voters: PollVoter[] = [];
    if (Array.isArray(rec.voters)) {
      for (const voter of rec.voters) {
        const vObj = asRecord(voter);
        if (vObj) {
          voters.push({
            jid: asString(vObj.jid),
            name: asString(vObj.name),
            time: asString(vObj.time) ?? asString(vObj.timestamp),
          });
        } else {
          const name = asString(voter);
          if (name) voters.push({ name });
        }
      }
    }
    const votes = asNumber(rec.votes ?? rec.count) ?? voters.length;
    options.push({
      id: asString(rec.id) ?? asString(rec.hash),
      text: asString(rec.text) ?? asString(rec.name) ?? '',
      votes,
      percent: asNumber(rec.percent ?? rec.percentage),
      voters,
    });
  }
  const sumVotes = options.reduce((acc, opt) => acc + opt.votes, 0);
  const totalVotes = asNumber(obj.total_votes ?? obj.totalVotes) ?? sumVotes;
  for (const opt of options) {
    if (opt.percent === undefined) {
      opt.percent = totalVotes > 0 ? Math.round((opt.votes * 1000) / totalVotes) / 10 : 0;
    }
  }
  return {
    question: question ?? '',
    allowMultiple: Boolean(obj.allow_multiple ?? obj.allowMultiple),
    totalVotes,
    totalVoters: asNumber(obj.total_voters ?? obj.totalVoters),
    options,
  };
}

export function parseReactions(raw: unknown): ChatReaction[] {
  const parsed = parseStructuredField(raw);
  if (!Array.isArray(parsed)) return [];
  const items: ChatReaction[] = [];
  for (const item of parsed) {
    const obj = asRecord(item);
    if (!obj) continue;
    const emoji = asString(obj.emoji) ?? asString(obj.reaction);
    if (!emoji) continue;
    items.push({
      emoji,
      sender: asString(obj.sender) ?? asString(obj.name),
      jid: asString(obj.jid),
      time: asString(obj.time),
    });
  }
  return items;
}

export function parseReplyTo(raw: unknown): ReplyContext | null {
  if (raw == null || raw === '') return null;
  const parsed = parseStructuredField(raw);
  const obj = asRecord(parsed);
  if (obj) {
    const sender = asString(obj.sender);
    const content = asString(obj.content) ?? asString(obj.text);
    const msgType = asString(obj.msg_type ?? obj.msgType);
    const stanzaId = asString(obj.stanza_id ?? obj.stanzaId);
    if (!sender && !content && !msgType && !stanzaId) return null;
    return { sender, content, msgType, stanzaId };
  }
  const plain = asString(raw);
  return plain ? { content: plain } : null;
}

const DEFAULT_OUTGOING_TOKENS = new Set(['outgoing', 'sent', 'true', '1', '发出', '已发送']);

export function isOutgoingMessage(record: AnalysisRecord, layout: RecordChatLayout): boolean {
  if (!layout.direction) return false;
  const raw = record.values[layout.direction];
  if (typeof raw === 'boolean') return raw;
  const text = asString(raw);
  if (!text) return false;
  if (layout.outgoingValues && layout.outgoingValues.length > 0) {
    if (layout.outgoingValues.some(candidate => candidate.toLowerCase() === text.toLowerCase())) {
      return true;
    }
  }
  return DEFAULT_OUTGOING_TOKENS.has(text.toLowerCase());
}

export function extractDateKey(timeValue: unknown): string {
  const text = asString(timeValue);
  if (!text) return '';
  const match = text.match(/^(\d{4}-\d{2}-\d{2})/);
  return match ? match[1] : '';
}

export function formatBubbleTime(timeValue: unknown): string {
  const text = asString(timeValue);
  if (!text) return '';
  const match = text.match(/^\d{4}-\d{2}-\d{2}[T ](\d{2}:\d{2}(?::\d{2})?)/);
  return match ? match[1] : text;
}

export function receiptBadgeInfo(
  receipts: ParsedReceipts | null,
  statusRaw: unknown,
  outgoing: boolean,
  tr: (text: string, vars?: Record<string, string | number>) => string,
): { label: string; tone: 'read' | 'delivered' | 'pending' | 'info' } | null {
  if (receipts) {
    if (receipts.mode === 'incoming_local') {
      return null;
    }
    const seen = receipts.playedCount + receipts.readCount;
    if (receipts.isGroup && (receipts.totalMembers != null || seen + receipts.unreadCount > 0)) {
      const denom = receipts.totalMembers != null
        ? Math.max(0, receipts.totalMembers - (outgoing ? 1 : 0))
        : seen + receipts.deliveredCount + receipts.unreadCount;
      if (receipts.playedCount > 0) {
        return { label: denom > 0 ? tr('已播放 {0}/{1}', { 0: seen, 1: denom }) : tr('已播放 {0}', { 0: seen }), tone: 'read' };
      }
      if (receipts.readCount > 0) {
        return { label: denom > 0 ? tr('已读 {0}/{1}', { 0: seen, 1: denom }) : tr('已读 {0}', { 0: seen }), tone: 'read' };
      }
      if (receipts.deliveredCount > 0) {
        return { label: tr('已送达 {0}', { 0: receipts.deliveredCount }), tone: 'delivered' };
      }
      if (receipts.unreadCount > 0 && outgoing) {
        return { label: tr('未读 {0}', { 0: receipts.unreadCount }), tone: 'pending' };
      }
    }
    if (receipts.playedCount > 0) return { label: tr('已播放'), tone: 'read' };
    if (receipts.readCount > 0) return { label: tr('已读'), tone: 'read' };
    if (receipts.deliveredCount > 0) return { label: tr('已送达'), tone: 'delivered' };
    if (receipts.unreadCount > 0 && outgoing) return { label: tr('未读'), tone: 'pending' };
    if (receipts.summary && outgoing) return { label: receipts.summary, tone: 'info' };
  }
  const status = asString(statusRaw);
  if (!status) return null;
  if (!outgoing && status === '本机已读') {
    return null;
  }
  if (/已播放|已读|played|read/i.test(status) && !/未读|unread/i.test(status)) {
    return { label: status, tone: 'read' };
  }
  if (/已送达|delivered/i.test(status)) {
    return { label: status, tone: 'delivered' };
  }
  if (/未读|待发送|发送中|unread|pending/i.test(status)) {
    return { label: status, tone: 'pending' };
  }
  return { label: status, tone: 'info' };
}
