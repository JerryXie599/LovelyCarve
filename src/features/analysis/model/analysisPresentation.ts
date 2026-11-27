import type { RecordChatLayout, RecordColumn, RecordFieldKind, RecordPropertyLayout } from '../../../lib/ipc/analysisResults';

export type AnalysisPresentation = 'chat' | 'properties' | 'table';

/** Narrowest share of a flexible (free text) table column. */
export const FLEXIBLE_COLUMN_WIDTH = 200;

/** Table column width (px) by field kind, never by record contents: counts and
 *  flags stay narrow, a time with its zone (`… UTC+08:00`) fits whole and
 *  paths get room. Free text, lists and JSON return null: they share whatever
 *  width is left, at least {@link FLEXIBLE_COLUMN_WIDTH} each. */
export function columnWidth(kind: RecordFieldKind): number | null {
  switch (kind) {
    case 'bool': return 88;
    case 'int':
    case 'float':
    case 'duration': return 112;
    case 'size': return 120;
    case 'enum': return 140;
    case 'ip': return 152;
    case 'timestamp':
    case 'hash': return 240;
    case 'path': return 280;
    case 'text':
    case 'list':
    case 'json': return null;
  }
}

/** Resolve only complete, distinct, visible roles; malformed or outdated hints fall back to a table. */
export function resolvePropertyLayout(layout: RecordPropertyLayout | null | undefined, columns: RecordColumn[]) {
  if (!layout) return null;
  const visible = columns.filter(column => !column.hidden);
  const byId = new Map(visible.map(column => [column.id, column]));
  const ids = [layout.value, layout.label, layout.secondary].filter((id): id is string => typeof id === 'string');
  if (ids.length !== visible.length || new Set(ids).size !== ids.length || ids.some(id => !byId.has(id))) return null;
  const value = byId.get(layout.value);
  if (!value) return null;
  return { value, label: layout.label ? byId.get(layout.label) ?? null : null, secondary: layout.secondary ? byId.get(layout.secondary) ?? null : null };
}

/** A new installed plugin may improve presentation of an older run only when its visible shape still matches. */
export function compatiblePropertyLayout(columns: RecordColumn[], declared: RecordColumn[], layout: RecordPropertyLayout | null | undefined) {
  const visible = columns.filter(column => !column.hidden);
  const current = declared.filter(column => !column.hidden);
  if (visible.length !== current.length || visible.some(column => !current.some(field => field.id === column.id && field.kind === column.kind))) return null;
  return resolvePropertyLayout(layout, columns) ? layout ?? null : null;
}

/** Resolve plugin-declared chat field bindings; optional roles missing from older schemas are omitted rather than failing the whole view. */
export function resolveChatLayout(
  layout: RecordChatLayout | null | undefined,
  columns: RecordColumn[],
): RecordChatLayout | null {
  if (!layout || !layout.threadId || !layout.content) return null;
  const byId = new Set(columns.map(column => column.id));
  if (!byId.has(layout.threadId) || !byId.has(layout.content)) return null;
  const opt = (id: string | null | undefined) => (id && byId.has(id) ? id : null);
  return {
    threadId: layout.threadId,
    threadTitle: opt(layout.threadTitle),
    sender: opt(layout.sender),
    direction: opt(layout.direction),
    outgoingValues: layout.outgoingValues ? [...layout.outgoingValues] : [],
    content: layout.content,
    time: opt(layout.time),
    msgType: opt(layout.msgType),
    media: opt(layout.media),
    status: opt(layout.status),
    receipts: opt(layout.receipts),
    poll: opt(layout.poll),
    reactions: opt(layout.reactions),
    replyTo: opt(layout.replyTo),
  };
}

/** Upgrade older stored chat results with the currently installed plugin's ChatLayout when key fields match. */
export function compatibleChatLayout(
  columns: RecordColumn[],
  declared: RecordColumn[],
  layout: RecordChatLayout | null | undefined,
): RecordChatLayout | null {
  if (!layout) return null;
  const byId = new Map(columns.map(column => [column.id, column]));
  const currentById = new Map(declared.map(column => [column.id, column]));
  for (const req of [layout.threadId, layout.content]) {
    const col = byId.get(req);
    const cur = currentById.get(req);
    if (!col || !cur || col.kind !== cur.kind) return null;
  }
  return resolveChatLayout(layout, columns);
}
