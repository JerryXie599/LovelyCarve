import type { PathBookmark } from '../../../lib/ipc/bookmarks';
import type { ImageId, NodeId } from '../../../lib/ipc/ids';
import type { FileNavigationFrame } from './navigation';

export interface BookmarkDraft {
  rootId: string;
  path: string;
  nodeId: NodeId | null;
  kind: 'dir' | 'file';
  label: string;
}

export function normalizeBookmarkPath(raw: string): string {
  return raw.replace(/\\/g, '/').trim().replace(/^\/+|\/+$/g, '');
}

export function isExplicitVolumeRootId(id: string): boolean {
  return (
    id.startsWith('part:') ||
    id.startsWith('lvm:') ||
    id.startsWith('apfs:part:') ||
    id.startsWith('host-docker:')
  );
}

/**
 * Normalizes a top-level `fs_children('root')` node ID to the `rootId` accepted
 * by `fs_source_locate` (`part:N` / `lvm:...` / `apfs:part:...` / `host-docker:...`,
 * or `'root'` for logical Folder/ZIP/TAR/7z/AD1/iOS backup containers).
 */
export function normalizeLogicalRootId(id: string): string {
  const trimmed = id.trim();
  if (!trimmed) return 'root';
  return isExplicitVolumeRootId(trimmed) ? trimmed : 'root';
}

export function bookmarkDedupKey(imageId: ImageId, rootId: string, path: string): string {
  return `${imageId}\0${rootId || 'root'}\0${normalizeBookmarkPath(path)}`;
}

export function defaultBookmarkLabel(rootId: string, path: string, stackRootName?: string): string {
  const clean = normalizeBookmarkPath(path);
  if (clean) {
    const base = clean.split('/').pop();
    if (base) return base;
  }
  if (stackRootName && stackRootName !== '/') return stackRootName;
  return rootId === 'root' ? '/' : rootId;
}

/**
 * Converts a navigation `stack` (plus optional clicked child node) into a canonical
 * `(rootId, path)` target suitable for `fs_source_locate` and `case_bookmarks_add`.
 */
export function resolveBookmarkTargetFromStack(
  stack: FileNavigationFrame[],
  child?: { id: string; name: string; kind: 'dir' | 'file' } | null,
): BookmarkDraft | null {
  if (!stack.length) return null;

  if (child && stack.length === 1 && isExplicitVolumeRootId(child.id)) {
    return {
      rootId: child.id,
      path: '',
      nodeId: child.id,
      kind: child.kind,
      label: child.name || child.id,
    };
  }

  let volIdx = -1;
  for (let i = 1; i < stack.length; i += 1) {
    if (isExplicitVolumeRootId(stack[i].id)) {
      volIdx = i;
    }
  }

  let rootId = 'root';
  let relSegments: string[];
  let fallbackName: string | undefined;

  if (volIdx >= 1) {
    rootId = stack[volIdx].id;
    fallbackName = stack[volIdx].name;
    relSegments = stack.slice(volIdx + 1).map(f => f.name);
  } else if (stack.length >= 2) {
    // Logical container frame at index 1 (`folder:...`, `zip:...`, `ios:...`, etc.)
    const top = stack[1];
    rootId = 'root';
    fallbackName = top.name;
    const isSyntheticContainer =
      top.name === '/' ||
      /^(?:folder|zip|tar|sevenz|ad1|docker):/i.test(top.id) ||
      top.id === 'directory_root';
    relSegments = (isSyntheticContainer ? stack.slice(2) : stack.slice(1)).map(f => f.name);
  } else {
    relSegments = [];
  }

  if (child) {
    relSegments.push(child.name);
  }

  const path = normalizeBookmarkPath(relSegments.filter(Boolean).join('/'));
  const nodeId = child?.id ?? (stack.length > 1 ? stack[stack.length - 1].id : null);
  const kind: 'dir' | 'file' = child?.kind ?? 'dir';
  const label = defaultBookmarkLabel(rootId, path, fallbackName);

  return { rootId, path, nodeId, kind, label };
}

export function formatBookmarkSubpath(bookmark: Pick<PathBookmark, 'rootId' | 'path'>): string {
  const clean = normalizeBookmarkPath(bookmark.path);
  const prefix = bookmark.rootId && bookmark.rootId !== 'root' ? `[${bookmark.rootId}] ` : '';
  return `${prefix}/${clean}`;
}

export function parseStoredBookmarks(raw: string | null): Record<string, PathBookmark[]> {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object') return {};
    const out: Record<string, PathBookmark[]> = {};
    for (const [imgId, list] of Object.entries(parsed)) {
      if (!Array.isArray(list)) continue;
      out[imgId] = list.filter(
        (item): item is PathBookmark =>
          Boolean(item) &&
          typeof item.id === 'string' &&
          typeof item.imageId === 'string' &&
          typeof item.path === 'string' &&
          typeof item.label === 'string',
      );
    }
    return out;
  } catch {
    return {};
  }
}

export function mergeBookmarkLists(primary: PathBookmark[], secondary: PathBookmark[]): PathBookmark[] {
  const map = new Map<string, PathBookmark>();
  for (const item of primary) {
    map.set(bookmarkDedupKey(item.imageId, item.rootId, item.path), item);
  }
  for (const item of secondary) {
    const key = bookmarkDedupKey(item.imageId, item.rootId, item.path);
    if (!map.has(key)) {
      map.set(key, item);
    }
  }
  return Array.from(map.values());
}
