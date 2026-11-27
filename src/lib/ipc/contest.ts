// Contest workspace, Pin evidence markers, Writeup export & CLI Sync IPC wrappers.
import { invokeCmd } from './client';
import { onEvent, type UnlistenFn } from './events';

export type QuestionStatus = 'unanswered' | 'answered' | 'review' | 'stuck';

export interface QuestionNote {
  id: string;
  author: string;
  kind: string;
  command?: string | null;
  content: string;
  createdAt: string;
}

export interface ContestExhibit {
  id: string;
  name: string;
  holder?: string | null;
  role?: string | null;
  deviceType?: string | null;
  category?: string | null;
  aliases: string[];
  imageId?: string | null;
  sha256?: string | null;
  notesMd: string;
  wpMd: string;
}

export interface ContestQuestion {
  id: string;
  exhibitId: string;
  number: number;
  stem: string;
  options: string[];
  score?: number | null;
  formatHint?: string | null;
  answer: string;
  status: QuestionStatus;
  notes: QuestionNote[];
  wpSectionMd: string;
  updatedAt: string;
}

export type PinTarget =
  | {
      kind: 'file';
      imageId: string;
      volumeKey?: string | null;
      nodeId?: number | null;
      path: string;
      metaAddr?: string | null;
    }
  | {
      kind: 'analysis_record';
      imageId: string;
      categoryId: string;
      categoryName?: string | null;
      recordId: number;
      previewText?: string | null;
    }
  | {
      kind: 'hex_range';
      imageId: string;
      path: string;
      offset: number;
      length: number;
      asciiPreview?: string | null;
    }
  | {
      kind: 'sqlite_row';
      imageId: string;
      path: string;
      table: string;
      rowId?: string | null;
      previewText?: string | null;
    }
  | {
      kind: 'custom';
      detail: string;
    };

export interface PinAnnotation {
  kind: 'box' | 'arrow' | 'box_arrow' | 'none' | string;
  x: number;
  y: number;
  w: number;
  h: number;
  arrowFrom?: [number, number] | null;
  arrowTo?: [number, number] | null;
  color?: string | null;
  label?: string | null;
}

export interface EvidencePin {
  id: string;
  questionId?: string | null;
  exhibitId?: string | null;
  title: string;
  summary: string;
  command?: string | null;
  source: string;
  target: PinTarget;
  annotation?: PinAnnotation | null;
  screenshotRel?: string | null;
  screenshotAt?: string | null;
  createdAt: string;
}

export interface ContestWorkspace {
  version: number;
  title: string;
  sourceFormat: string;
  caseWpMd: string;
  exhibits: ContestExhibit[];
  questions: ContestQuestion[];
  pins: EvidencePin[];
  updatedAt: string;
}

export interface ExportResult {
  markdownPath: string;
  assetsDir: string;
  exportedAssets: number;
}

export interface PinAddInput {
  questionId?: string | null;
  exhibitId?: string | null;
  title: string;
  summary: string;
  command?: string | null;
  source?: string | null;
  target: PinTarget;
  annotation?: PinAnnotation | null;
}

export interface QuestionUpsertInput {
  id?: string | null;
  exhibitId?: string | null;
  number?: number | null;
  stem?: string | null;
  options?: string[] | null;
  score?: number | null;
  formatHint?: string | null;
  answer?: string | null;
  status?: QuestionStatus | null;
  wpSectionMd?: string | null;
}

export interface CliSyncStatus {
  enabled: boolean;
  active: boolean;
  caseDir?: string | null;
  port?: number | null;
  pid: number;
  lastError?: string | null;
}

export interface CliSyncEvent {
  command: string;
  caseDir: string;
  at: string;
}

export function contestGet(): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_get');
}

export function contestImport(
  content: string,
  format?: string,
  title?: string,
  merge?: boolean,
): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_import', {
    content,
    format: format ?? null,
    title: title ?? null,
    merge: merge ?? false,
  });
}

export function contestClear(): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_clear');
}

export function contestExhibitBind(
  exhibitId: string,
  imageId: string | null,
): Promise<ContestExhibit> {
  return invokeCmd<ContestExhibit>('contest_exhibit_bind', {
    exhibitId,
    imageId,
  });
}

export function contestQuestionUpsert(input: QuestionUpsertInput): Promise<ContestQuestion> {
  return invokeCmd<ContestQuestion>('contest_question_upsert', { input });
}

export function contestQuestionNoteAdd(params: {
  questionId: string;
  author?: string;
  kind?: string;
  command?: string;
  content: string;
}): Promise<QuestionNote> {
  return invokeCmd<QuestionNote>('contest_question_note_add', {
    questionId: params.questionId,
    author: params.author ?? null,
    kind: params.kind ?? null,
    command: params.command ?? null,
    content: params.content,
  });
}

export function contestQuestionNoteRemove(
  questionId: string,
  noteId: string,
): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_question_note_remove', {
    questionId,
    noteId,
  });
}

export function contestPinAdd(input: PinAddInput): Promise<EvidencePin> {
  return invokeCmd<EvidencePin>('contest_pin_add', { input });
}

export function contestPinUpdate(params: {
  pinId: string;
  title?: string;
  summary?: string;
  questionId?: string | null;
  exhibitId?: string | null;
  command?: string | null;
  annotation?: PinAnnotation | null;
}): Promise<EvidencePin> {
  return invokeCmd<EvidencePin>('contest_pin_update', {
    pinId: params.pinId,
    title: params.title ?? null,
    summary: params.summary ?? null,
    questionId: params.questionId ?? null,
    exhibitId: params.exhibitId ?? null,
    command: params.command ?? null,
    annotation: params.annotation ?? null,
  });
}

export function contestPinRemove(pinId: string): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_pin_remove', { pinId });
}

export function contestWpUpdate(params: {
  caseWpMd?: string;
  exhibitId?: string;
  exhibitWpMd?: string;
  questionId?: string;
  questionWpMd?: string;
}): Promise<ContestWorkspace> {
  return invokeCmd<ContestWorkspace>('contest_wp_update', {
    caseWpMd: params.caseWpMd ?? null,
    exhibitId: params.exhibitId ?? null,
    exhibitWpMd: params.exhibitWpMd ?? null,
    questionId: params.questionId ?? null,
    questionWpMd: params.questionWpMd ?? null,
  });
}

export function contestAssetRead(relPath: string): Promise<string> {
  return invokeCmd<string>('contest_asset_read', { relPath });
}

export function contestWpPreview(exhibitId?: string | null): Promise<string> {
  return invokeCmd<string>('contest_wp_preview', { exhibitId: exhibitId ?? null });
}

export function contestWpExport(
  outputMdPath: string,
  exhibitId?: string | null,
): Promise<ExportResult> {
  return invokeCmd<ExportResult>('contest_wp_export', {
    outputMdPath,
    exhibitId: exhibitId ?? null,
  });
}

export function cliSyncStatus(): Promise<CliSyncStatus> {
  return invokeCmd<CliSyncStatus>('cli_sync_status');
}

export function cliSyncSet(enabled: boolean): Promise<CliSyncStatus> {
  return invokeCmd<CliSyncStatus>('cli_sync_set', { enabled });
}

export function onContestUpdated(
  handler: (payload: { source: string; command?: string }) => void,
): Promise<UnlistenFn> {
  return onEvent<{ source: string; command?: string }>('contest-updated', event =>
    handler(event.payload),
  );
}

export function onCliSyncStatus(handler: (status: CliSyncStatus) => void): Promise<UnlistenFn> {
  return onEvent<CliSyncStatus>('cli-sync-status', event => handler(event.payload));
}

export function onCliSyncEvent(handler: (event: CliSyncEvent) => void): Promise<UnlistenFn> {
  return onEvent<CliSyncEvent>('cli-sync-event', event => handler(event.payload));
}
