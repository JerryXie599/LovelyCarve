import { describe, expect, it } from 'vitest';
import type { AnalysisRecord, RecordChatLayout, RecordColumn } from '../../../lib/ipc/analysisResults';
import { compatibleChatLayout, resolveChatLayout } from './analysisPresentation';
import {
  extractDateKey,
  formatBubbleTime,
  isOutgoingMessage,
  parsePoll,
  parseReactions,
  parseReceipts,
  parseReplyTo,
  receiptBadgeInfo,
} from './chatView';

const identityTr = (text: string, vars?: Record<string, string | number>) =>
  vars ? text.replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? '')) : text;

describe('chat presentation layout resolution', () => {
  const columns: RecordColumn[] = [
    { id: 'chat_id', label: '会话ID', kind: 'text', key: true, hidden: false, timeline: null, choices: [] },
    { id: 'chat_name', label: '会话名', kind: 'text', key: false, hidden: false, timeline: null, choices: [] },
    { id: 'direction', label: '方向', kind: 'text', key: false, hidden: false, timeline: null, choices: [] },
    { id: 'content', label: '内容', kind: 'text', key: false, hidden: false, timeline: null, choices: [] },
    { id: 'receipts', label: '回执', kind: 'json', key: false, hidden: true, timeline: null, choices: [] },
    { id: 'poll', label: '投票', kind: 'json', key: false, hidden: true, timeline: null, choices: [] },
  ];

  const layout: RecordChatLayout = {
    threadId: 'chat_id',
    threadTitle: 'chat_name',
    direction: 'direction',
    outgoingValues: ['发出'],
    content: 'content',
    receipts: 'receipts',
    poll: 'poll',
    reactions: 'missing_reactions',
  };

  it('resolves valid required and hidden json fields while dropping absent optional ones', () => {
    const resolved = resolveChatLayout(layout, columns);
    expect(resolved).not.toBeNull();
    expect(resolved?.threadId).toBe('chat_id');
    expect(resolved?.receipts).toBe('receipts');
    expect(resolved?.poll).toBe('poll');
    expect(resolved?.reactions).toBeNull();
    expect(resolveChatLayout({ threadId: 'nope', content: 'content' }, columns)).toBeNull();
    expect(compatibleChatLayout(columns, columns, layout)?.receipts).toBe('receipts');
  });
});

describe('chatView structured field parsers', () => {
  it('parses receipts JSON with read/played/delivered/unread members and computes badges', () => {
    const receipts = parseReceipts(JSON.stringify({
      is_group: true,
      total_members: 4,
      read_count: 1,
      played_count: 0,
      delivered_count: 1,
      unread_count: 1,
      read: [{ jid: '852123@s.whatsapp.net', name: '张三', time: '2026-08-01T10:02:00+08:00' }],
      delivered: [{ jid: '852456@s.whatsapp.net', name: '李四', delivered_at: '2026-08-01T10:01:00+08:00' }],
      unread: [{ jid: '852789@s.whatsapp.net', name: '王五' }],
    }));
    expect(receipts?.isGroup).toBe(true);
    expect(receipts?.read).toHaveLength(1);
    expect(receipts?.read[0].name).toBe('张三');
    expect(receipts?.delivered[0].deliveredAt).toBe('2026-08-01T10:01:00+08:00');
    expect(receiptBadgeInfo(receipts, null, true, identityTr)).toEqual({
      label: '已读 1/3',
      tone: 'read',
    });
  });

  it('parses poll definitions with voters and percentage calculation', () => {
    const poll = parsePoll({
      question: '今晚几点开会？',
      allow_multiple: false,
      total_voters: 3,
      options: [
        { id: 'h1', text: '20:00', votes: 2, voters: [{ name: 'Alice', time: '2026-08-01T09:00:00+08:00' }, { name: 'Bob' }] },
        { id: 'h2', text: '21:00', votes: 1, voters: [{ name: 'Carol' }] },
      ],
    });
    expect(poll?.question).toBe('今晚几点开会？');
    expect(poll?.totalVotes).toBe(3);
    expect(poll?.options[0].percent).toBeCloseTo(66.7, 1);
    expect(poll?.options[0].voters).toHaveLength(2);
  });

  it('parses reactions, reply context, direction, and date/time formatting', () => {
    expect(parseReactions('[{"emoji":"👍","sender":"Alice"}]')).toEqual([{ emoji: '👍', sender: 'Alice', jid: undefined, time: undefined }]);
    expect(parseReplyTo('{"sender":"Bob","content":"上一条文本"}')).toEqual({ sender: 'Bob', content: '上一条文本', msgType: undefined, stanzaId: undefined });
    expect(extractDateKey('2026-08-01T10:05:09.123+08:00')).toBe('2026-08-01');
    expect(formatBubbleTime('2026-08-01T10:05:09.123+08:00')).toBe('10:05:09');

    const record: AnalysisRecord = {
      recordId: '1',
      runId: 'r1',
      recordType: 'ios.wa_msg',
      title: 'msg',
      severity: null,
      values: { chat_id: 'c1', direction: '发出', content: 'hi' },
      sources: [],
      imageId: 'img',
      legacy: false,
    };
    expect(isOutgoingMessage(record, { threadId: 'chat_id', content: 'content', direction: 'direction', outgoingValues: ['发出'] })).toBe(true);
    expect(isOutgoingMessage({ ...record, values: { chat_id: 'c1', direction: '收到', content: 'hi' } }, { threadId: 'chat_id', content: 'content', direction: 'direction', outgoingValues: ['发出'] })).toBe(false);
  });
});
