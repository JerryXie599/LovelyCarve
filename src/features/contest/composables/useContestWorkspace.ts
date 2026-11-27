import { nextTick, ref } from 'vue';
import {
  cliSyncSet,
  cliSyncStatus,
  contestAssetRead,
  contestClear,
  contestExhibitBind,
  contestGet,
  contestImport,
  contestPinAdd,
  contestPinRemove,
  contestPinUpdate,
  contestQuestionNoteAdd,
  contestQuestionNoteRemove,
  contestQuestionUpsert,
  contestWpExport,
  contestWpPreview,
  contestWpUpdate,
  onCliSyncEvent,
  onCliSyncStatus,
  onContestUpdated,
  type CliSyncEvent,
  type CliSyncStatus,
  type ContestWorkspace,
  type EvidencePin,
  type ExportResult,
  type PinAddInput,
  type PinAnnotation,
  type PinTarget,
  type QuestionUpsertInput,
} from '../../../lib/ipc/contest';
import { captureDesktopWindow, type CaptureRegion } from '../../../lib/ipc/desktopCapture';
import { extractAssetRefs } from '../model/contest';

export interface QuickPinDraft {
  title: string;
  summary: string;
  target: PinTarget;
  exhibitId?: string | null;
}

export interface CaptureSessionState {
  pin: EvidencePin;
  returnToContest: boolean;
}

interface ContestNavigator {
  openPinTarget: (pin: EvidencePin) => Promise<void>;
  openExhibitEvidence: (imageId: string) => Promise<void>;
  showContestPanel: () => void;
}

const workspace = ref<ContestWorkspace | null>(null);
const wpMarkdown = ref('');
const loading = ref(false);
const cliSync = ref<CliSyncStatus | null>(null);
const recentCliEvent = ref<CliSyncEvent | null>(null);
const assetUrlCache = ref<Record<string, string>>({});
const quickPinDraft = ref<QuickPinDraft | null>(null);
const captureSession = ref<CaptureSessionState | null>(null);
const activeScopeExhibitId = ref<string>('all');

let navigatorImpl: ContestNavigator | null = null;

export function registerContestNavigator(handlers: ContestNavigator) {
  navigatorImpl = handlers;
}

export function openQuickPin(draft: QuickPinDraft) {
  quickPinDraft.value = draft;
}

export function closeQuickPin() {
  quickPinDraft.value = null;
}

async function hydrateWorkspaceAssets(ws: ContestWorkspace | null, md?: string) {
  const rels = new Set<string>();
  for (const pin of ws?.pins ?? []) {
    if (pin.screenshotRel) rels.add(pin.screenshotRel);
  }
  if (md) {
    for (const rel of extractAssetRefs(md)) {
      rels.add(rel);
    }
  }
  await Promise.all(
    Array.from(rels).map(async rel => {
      await ensureAssetUrl(rel);
    }),
  );
}

export async function ensureAssetUrl(relPath: string, force = false): Promise<string | null> {
  const key = relPath.replace(/^\.\//, '');
  if (!key) return null;
  if (!force && assetUrlCache.value[key]) {
    return assetUrlCache.value[key];
  }
  try {
    const dataUrl = await contestAssetRead(key);
    assetUrlCache.value = { ...assetUrlCache.value, [key]: dataUrl };
    return dataUrl;
  } catch {
    return null;
  }
}

export async function refreshWorkspace(): Promise<ContestWorkspace | null> {
  loading.value = true;
  try {
    const ws = await contestGet();
    workspace.value = ws;
    const exhibitFilter =
      activeScopeExhibitId.value === 'all' ? null : activeScopeExhibitId.value;
    const preview = await contestWpPreview(exhibitFilter);
    wpMarkdown.value = preview;
    void hydrateWorkspaceAssets(ws, preview);
    return ws;
  } catch {
    workspace.value = null;
    wpMarkdown.value = '';
    return null;
  } finally {
    loading.value = false;
  }
}

export async function refreshWpPreviewOnly(exhibitId?: string | null): Promise<string> {
  const scope = exhibitId === undefined ? activeScopeExhibitId.value : exhibitId;
  const preview = await contestWpPreview(!scope || scope === 'all' ? null : scope);
  wpMarkdown.value = preview;
  void hydrateWorkspaceAssets(workspace.value, preview);
  return preview;
}

export async function refreshCliSync(): Promise<CliSyncStatus | null> {
  try {
    const st = await cliSyncStatus();
    cliSync.value = st;
    return st;
  } catch {
    cliSync.value = null;
    return null;
  }
}

export async function toggleCliSync(enabled: boolean): Promise<CliSyncStatus> {
  const st = await cliSyncSet(enabled);
  cliSync.value = st;
  return st;
}

export async function importContestContent(
  content: string,
  format = 'auto',
  title?: string,
  merge = false,
): Promise<ContestWorkspace> {
  const ws = await contestImport(content, format, title, merge);
  workspace.value = ws;
  await refreshWpPreviewOnly();
  return ws;
}

export async function clearContestWorkspace(): Promise<void> {
  workspace.value = await contestClear();
  await refreshWpPreviewOnly();
}

export async function bindContestExhibit(exhibitId: string, imageId: string | null) {
  await contestExhibitBind(exhibitId, imageId);
  await refreshWorkspace();
}

export async function upsertContestQuestion(input: QuestionUpsertInput) {
  const q = await contestQuestionUpsert(input);
  await refreshWorkspace();
  return q;
}

export async function addContestQuestionNote(params: {
  questionId: string;
  author?: string;
  kind?: string;
  command?: string;
  content: string;
}) {
  const note = await contestQuestionNoteAdd(params);
  await refreshWorkspace();
  return note;
}

export async function removeContestQuestionNote(questionId: string, noteId: string) {
  workspace.value = await contestQuestionNoteRemove(questionId, noteId);
  await refreshWpPreviewOnly();
}

export async function addContestPin(input: PinAddInput): Promise<EvidencePin> {
  const pin = await contestPinAdd(input);
  await refreshWorkspace();
  return pin;
}

export async function updateContestPin(params: {
  pinId: string;
  title?: string;
  summary?: string;
  questionId?: string | null;
  exhibitId?: string | null;
  command?: string | null;
  annotation?: PinAnnotation | null;
}): Promise<EvidencePin> {
  const pin = await contestPinUpdate(params);
  await refreshWorkspace();
  return pin;
}

export async function removeContestPin(pinId: string) {
  workspace.value = await contestPinRemove(pinId);
  await refreshWpPreviewOnly();
}

export async function saveWpMarkdown(params: {
  caseWpMd?: string;
  exhibitId?: string;
  exhibitWpMd?: string;
  questionId?: string;
  questionWpMd?: string;
}) {
  workspace.value = await contestWpUpdate(params);
  await refreshWpPreviewOnly();
}

export async function exportWriteupPackage(
  outputMdPath: string,
  exhibitId?: string | null,
): Promise<ExportResult> {
  return await contestWpExport(
    outputMdPath,
    !exhibitId || exhibitId === 'all' ? null : exhibitId,
  );
}

export async function openPinTargetInWorkspace(pin: EvidencePin): Promise<void> {
  if (!navigatorImpl) return;
  await navigatorImpl.openPinTarget(pin);
}

export async function openExhibitInWorkspace(imageId: string): Promise<void> {
  if (!navigatorImpl) return;
  await navigatorImpl.openExhibitEvidence(imageId);
}

export async function startPinCapture(
  pin: EvidencePin,
  options: { navigateFirst?: boolean; returnToContest?: boolean } = {},
) {
  if (options.navigateFirst && navigatorImpl && pin.target.kind !== 'custom') {
    await navigatorImpl.openPinTarget(pin);
    await nextTick();
    await new Promise(resolve => setTimeout(resolve, 180));
  }
  captureSession.value = {
    pin,
    returnToContest: options.returnToContest ?? false,
  };
}

export async function finishPinCapture(
  region: CaptureRegion | null,
  annotation: PinAnnotation | null,
) {
  const session = captureSession.value;
  if (!session) return null;
  const result = await captureDesktopWindow(session.pin.id, region);
  if (annotation) {
    await contestPinUpdate({ pinId: session.pin.id, annotation });
  }
  await ensureAssetUrl(result.screenshotRel, true);
  await refreshWorkspace();
  const returnToContest = session.returnToContest;
  captureSession.value = null;
  if (returnToContest && navigatorImpl) {
    navigatorImpl.showContestPanel();
  }
  return result;
}

export function cancelPinCapture() {
  captureSession.value = null;
}

export function bindContestEvents(onCliEvidenceChange?: () => void): () => void {
  const unlisteners: Array<() => void> = [];
  let disposed = false;

  void onContestUpdated(() => {
    void refreshWorkspace();
  }).then(fn => {
    if (disposed) fn();
    else unlisteners.push(fn);
  });

  void onCliSyncStatus(st => {
    cliSync.value = st;
  }).then(fn => {
    if (disposed) fn();
    else unlisteners.push(fn);
  });

  void onCliSyncEvent(ev => {
    recentCliEvent.value = ev;
    if (
      ev.command.startsWith('evidence ')
      || ev.command.startsWith('case ')
    ) {
      onCliEvidenceChange?.();
    }
  }).then(fn => {
    if (disposed) fn();
    else unlisteners.push(fn);
  });

  return () => {
    disposed = true;
    for (const fn of unlisteners) fn();
  };
}

export function useContestWorkspace() {
  return {
    workspace,
    wpMarkdown,
    loading,
    cliSync,
    recentCliEvent,
    assetUrlCache,
    quickPinDraft,
    captureSession,
    activeScopeExhibitId,
    refreshWorkspace,
    refreshWpPreviewOnly,
    refreshCliSync,
    toggleCliSync,
    importContestContent,
    clearContestWorkspace,
    bindContestExhibit,
    upsertContestQuestion,
    addContestQuestionNote,
    removeContestQuestionNote,
    addContestPin,
    updateContestPin,
    removeContestPin,
    saveWpMarkdown,
    exportWriteupPackage,
    ensureAssetUrl,
    openQuickPin,
    closeQuickPin,
    openPinTargetInWorkspace,
    openExhibitInWorkspace,
    startPinCapture,
    finishPinCapture,
    cancelPinCapture,
  };
}
