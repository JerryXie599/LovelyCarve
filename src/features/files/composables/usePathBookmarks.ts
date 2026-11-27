import { computed, ref } from 'vue';
import {
  caseBookmarksAdd,
  caseBookmarksList,
  caseBookmarksRemove,
  type PathBookmark,
} from '../../../lib/ipc/bookmarks';
import type { ImageId } from '../../../lib/ipc/ids';
import {
  bookmarkDedupKey,
  mergeBookmarkLists,
  normalizeBookmarkPath,
  parseStoredBookmarks,
  type BookmarkDraft,
} from '../model/bookmarks';

const STORAGE_KEY = 'lc.pathBookmarks.v1';

const store = ref<Record<string, PathBookmark[]>>(loadFromLocalStorage());
let activeImageSync: ImageId | null = null;

function loadFromLocalStorage(): Record<string, PathBookmark[]> {
  try {
    return parseStoredBookmarks(localStorage.getItem(STORAGE_KEY));
  } catch {
    return {};
  }
}

function persistLocalStorage() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(store.value));
  } catch {
    // ignore storage quota errors
  }
}

export function usePathBookmarks(imageId?: () => ImageId | null) {
  const currentList = computed<PathBookmark[]>(() => {
    const id = imageId?.() ?? '';
    return id ? store.value[id] ?? [] : [];
  });

  function isBookmarked(imgId: ImageId, rootId: string, path: string): boolean {
    const list = store.value[imgId] ?? [];
    const targetKey = bookmarkDedupKey(imgId, rootId, path);
    return list.some(b => bookmarkDedupKey(b.imageId, b.rootId, b.path) === targetKey);
  }

  async function refreshCaseBookmarks(imgId?: ImageId | null) {
    const target = imgId ?? imageId?.() ?? null;
    if (!target) return;
    if (activeImageSync === target && (store.value[target]?.length ?? 0) > 0) {
      // still fetch quietly to stay synced with CLI mutations
    }
    activeImageSync = target;
    try {
      const fromCase = await caseBookmarksList({ imageId: target });
      const local = store.value[target] ?? [];
      store.value = {
        ...store.value,
        [target]: mergeBookmarkLists(fromCase, local),
      };
      persistLocalStorage();
    } catch {
      // No open case or offline: localStorage bookmarks still apply
    }
  }

  async function addBookmark(imgId: ImageId, draft: BookmarkDraft): Promise<PathBookmark> {
    const normPath = normalizeBookmarkPath(draft.path);
    const optimistic: PathBookmark = {
      id: `local:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
      imageId: imgId,
      rootId: draft.rootId || 'root',
      path: normPath,
      nodeId: draft.nodeId,
      kind: draft.kind,
      label: draft.label,
      createdAt: new Date().toISOString(),
    };

    const dedup = bookmarkDedupKey(imgId, optimistic.rootId, normPath);
    const prevList = store.value[imgId] ?? [];
    const nextList = [
      optimistic,
      ...prevList.filter(b => bookmarkDedupKey(b.imageId, b.rootId, b.path) !== dedup),
    ];
    store.value = { ...store.value, [imgId]: nextList };
    persistLocalStorage();

    try {
      const saved = await caseBookmarksAdd({
        imageId: imgId,
        rootId: optimistic.rootId,
        path: normPath,
        nodeId: optimistic.nodeId,
        kind: optimistic.kind,
        label: optimistic.label,
      });
      store.value = {
        ...store.value,
        [imgId]: [
          saved,
          ...(store.value[imgId] ?? []).filter(
            b => bookmarkDedupKey(b.imageId, b.rootId, b.path) !== dedup && b.id !== optimistic.id,
          ),
        ],
      };
      persistLocalStorage();
      return saved;
    } catch {
      return optimistic;
    }
  }

  async function removeBookmark(imgId: ImageId, bookmarkId: string): Promise<void> {
    const list = store.value[imgId] ?? [];
    const target = list.find(b => b.id === bookmarkId);
    store.value = {
      ...store.value,
      [imgId]: list.filter(b => b.id !== bookmarkId),
    };
    persistLocalStorage();

    if (target && !target.id.startsWith('local:')) {
      try {
        await caseBookmarksRemove({ bookmarkId: target.id });
      } catch {
        // Case may be locked or closed; keep local removal state
      }
    }
  }

  async function toggleBookmark(imgId: ImageId, draft: BookmarkDraft): Promise<boolean> {
    const list = store.value[imgId] ?? [];
    const key = bookmarkDedupKey(imgId, draft.rootId || 'root', draft.path);
    const existing = list.find(b => bookmarkDedupKey(b.imageId, b.rootId, b.path) === key);
    if (existing) {
      await removeBookmark(imgId, existing.id);
      return false;
    }
    await addBookmark(imgId, draft);
    return true;
  }

  return {
    bookmarks: currentList,
    isBookmarked,
    refreshCaseBookmarks,
    addBookmark,
    removeBookmark,
    toggleBookmark,
  };
}
