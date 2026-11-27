import { describe, expect, it } from 'vitest';
import {
  bookmarkDedupKey,
  mergeBookmarkLists,
  normalizeBookmarkPath,
  normalizeLogicalRootId,
  parseStoredBookmarks,
  resolveBookmarkTargetFromStack,
} from './bookmarks';

describe('files/model/bookmarks', () => {
  it('resolves partitioned stack targets including nested LVM/APFS subvolumes', () => {
    const target = resolveBookmarkTargetFromStack([
      { id: 'root', name: '/' },
      { id: 'part:1', name: '分区1' },
      { id: 'etc', name: 'etc' },
      { id: 'nginx', name: 'nginx' },
    ]);
    expect(target).toEqual({
      rootId: 'part:1',
      path: 'etc/nginx',
      nodeId: 'nginx',
      kind: 'dir',
      label: 'nginx',
    });

    const nestedLvm = resolveBookmarkTargetFromStack(
      [
        { id: 'root', name: '/' },
        { id: 'part:2', name: '分区2' },
        { id: 'lvm:vg0-root', name: 'vg0-root' },
        { id: 'var', name: 'var' },
      ],
      { id: 'auth-log', name: 'auth.log', kind: 'file' },
    );
    expect(nestedLvm).toEqual({
      rootId: 'lvm:vg0-root',
      path: 'var/auth.log',
      nodeId: 'auth-log',
      kind: 'file',
      label: 'auth.log',
    });
  });

  it('normalizes logical container roots to rootId="root" instead of folder:<hash>', () => {
    expect(normalizeLogicalRootId('folder:1234abcd')).toBe('root');
    expect(normalizeLogicalRootId('zip:deadbeef')).toBe('root');
    expect(normalizeLogicalRootId('part:0')).toBe('part:0');

    const logical = resolveBookmarkTargetFromStack([
      { id: 'root', name: '/' },
      { id: 'folder:9999', name: 'backup' },
      { id: 'n1', name: 'HomeDomain' },
      { id: 'n2', name: 'Library' },
      { id: 'n3', name: 'SMS' },
    ]);
    expect(logical).toEqual({
      rootId: 'root',
      path: 'HomeDomain/Library/SMS',
      nodeId: 'n3',
      kind: 'dir',
      label: 'SMS',
    });
  });

  it('normalizes paths, deduplicates, and merges stored + case bookmarks', () => {
    expect(normalizeBookmarkPath('/etc\\nginx//')).toBe('etc/nginx');
    expect(bookmarkDedupKey('img1', 'part:0', '/etc/passwd')).toBe('img1\0part:0\0etc/passwd');

    const stored = parseStoredBookmarks(
      JSON.stringify({
        img1: [
          {
            id: '1',
            imageId: 'img1',
            rootId: 'part:0',
            path: 'etc/passwd',
            nodeId: 'n1',
            kind: 'file',
            label: 'passwd',
            createdAt: '2026-08-06T00:00:00Z',
          },
        ],
      }),
    );
    expect(stored.img1).toHaveLength(1);

    const merged = mergeBookmarkLists(stored.img1, [
      {
        id: '2',
        imageId: 'img1',
        rootId: 'part:0',
        path: 'etc/passwd',
        nodeId: 'n1',
        kind: 'file',
        label: 'duplicate',
        createdAt: '2026-08-06T01:00:00Z',
      },
      {
        id: '3',
        imageId: 'img1',
        rootId: 'part:0',
        path: 'root/terminal-logs',
        nodeId: 'n2',
        kind: 'dir',
        label: 'terminal-logs',
        createdAt: '2026-08-06T02:00:00Z',
      },
    ]);
    expect(merged.map(b => b.id)).toEqual(['1', '3']);
  });
});
