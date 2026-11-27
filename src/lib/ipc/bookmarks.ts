import { invokeCmd } from './client';
import type { ImageId, NodeId } from './ids';

export interface PathBookmark {
  id: string;
  imageId: ImageId;
  rootId: string;
  path: string;
  nodeId: NodeId | null;
  kind: 'dir' | 'file';
  label: string;
  createdAt: string;
}

export function caseBookmarksList(args?: { imageId?: ImageId | null }): Promise<PathBookmark[]> {
  return invokeCmd<PathBookmark[]>('case_bookmarks_list', {
    imageId: args?.imageId ?? null,
  });
}

export function caseBookmarksAdd(args: {
  imageId: ImageId;
  rootId?: string | null;
  path: string;
  nodeId?: NodeId | null;
  kind?: 'dir' | 'file';
  label?: string | null;
}): Promise<PathBookmark> {
  return invokeCmd<PathBookmark>('case_bookmarks_add', {
    imageId: args.imageId,
    rootId: args.rootId ?? null,
    path: args.path,
    nodeId: args.nodeId ?? null,
    kind: args.kind ?? 'dir',
    label: args.label ?? null,
  });
}

export function caseBookmarksRemove(args: { bookmarkId: string }): Promise<boolean> {
  return invokeCmd<boolean>('case_bookmarks_remove', args);
}
