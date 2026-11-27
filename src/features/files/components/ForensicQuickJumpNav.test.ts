// @vitest-environment happy-dom
import { createApp, h, nextTick, ref, type App } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FsNode } from '../../../lib/ipc/fs/types';
import type { FileLocationTarget } from '../../../lib/models/fileLocation';

const mockFsChildren = vi.fn();
const mockCaseBookmarksList = vi.fn();
const mockCaseBookmarksAdd = vi.fn();
const mockCaseBookmarksRemove = vi.fn();

vi.mock('../../../lib/ipc/fs/query', () => ({
  fsChildren: (...args: unknown[]) => mockFsChildren(...args),
}));

vi.mock('../../../lib/ipc/bookmarks', () => ({
  caseBookmarksList: (...args: unknown[]) => mockCaseBookmarksList(...args),
  caseBookmarksAdd: (...args: unknown[]) => mockCaseBookmarksAdd(...args),
  caseBookmarksRemove: (...args: unknown[]) => mockCaseBookmarksRemove(...args),
}));

vi.mock('../../cases/api', () => ({
  useCaseContext: () => ({
    activeCase: ref({ id: 'case-1', name: 'Test Case' }),
  }),
}));

import ForensicQuickJumpNav from './ForensicQuickJumpNav.vue';
import { usePathBookmarks } from '../composables/usePathBookmarks';

let app: App | null = null;
let host: HTMLElement;
const jumps: FileLocationTarget[] = [];

async function settle() {
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
    await nextTick();
  }
}

function mountNav(props: { imageId: string; platform?: string | null; isIosBackup?: boolean }) {
  host = document.createElement('div');
  document.body.append(host);
  app = createApp({
    render: () =>
      h(ForensicQuickJumpNav, {
        ...props,
        onJump: (target: FileLocationTarget) => {
          jumps.push(target);
        },
      }),
  });
  app.mount(host);
}

beforeEach(() => {
  localStorage.clear();
  jumps.length = 0;
  mockFsChildren.mockReset();
  mockCaseBookmarksList.mockReset().mockResolvedValue([]);
  mockCaseBookmarksAdd.mockReset();
  mockCaseBookmarksRemove.mockReset().mockResolvedValue(true);
});

afterEach(() => {
  app?.unmount();
  app = null;
  document.body.replaceChildren();
});

describe('ForensicQuickJumpNav', () => {
  it('shows iOS full-filesystem shortcuts (never Linux /etc) when platform is ios', async () => {
    mockFsChildren.mockImplementation(async ({ parentId }: { parentId: string }) => {
      if (parentId === 'root') {
        return [{ id: 'folder:2f', name: '/', kind: 'dir', size: 0 } as FsNode];
      }
      if (parentId === 'folder:2f') {
        return [
          { id: 'dir:priv', name: 'private', kind: 'dir', size: 0 } as FsNode,
          { id: 'dir:var', name: 'var', kind: 'dir', size: 0 } as FsNode,
        ];
      }
      return [];
    });

    mountNav({ imageId: 'img-ios-fs', platform: 'ios', isIosBackup: false });
    await settle();

    const labels = [...host.querySelectorAll('.qj-group-label')].map(el => el.textContent?.trim());
    expect(labels).toEqual(['iOS 文件系统直达']);

    const names = [...host.querySelectorAll('.qj-name')].map(el => el.textContent?.trim());
    expect(names.some(n => n?.includes('SMS'))).toBe(true);
    expect(names.some(n => n?.includes('/etc'))).toBe(false);

    const smsBtn = [...host.querySelectorAll<HTMLButtonElement>('.qj-groups .qj-btn')][0]!;
    smsBtn.click();
    expect(jumps[0]).toEqual({
      imageId: 'img-ios-fs',
      rootId: 'root',
      path: 'private/var/mobile/Library/SMS',
    });
  });

  it('shows iTunes backup shortcuts when platform is ios with HomeDomain', async () => {
    mockFsChildren.mockImplementation(async ({ parentId }: { parentId: string }) => {
      if (parentId === 'root') {
        return [{ id: 'ios:root', name: 'Backup', kind: 'dir', size: 0 } as FsNode];
      }
      if (parentId === 'ios:root') {
        return [
          { id: 'dom:home', name: 'HomeDomain', kind: 'dir', size: 0 } as FsNode,
          { id: 'dom:cam', name: 'CameraRollDomain', kind: 'dir', size: 0 } as FsNode,
        ];
      }
      return [];
    });

    mountNav({ imageId: 'img-ios-backup', platform: 'ios', isIosBackup: true });
    await settle();

    const labels = [...host.querySelectorAll('.qj-group-label')].map(el => el.textContent?.trim());
    expect(labels).toEqual(['iOS 备份取证直达']);

    const smsBtn = [...host.querySelectorAll<HTMLButtonElement>('.qj-groups .qj-btn')][0]!;
    smsBtn.click();
    expect(jumps[0]).toEqual({
      imageId: 'img-ios-backup',
      rootId: 'root',
      path: 'HomeDomain/Library/SMS',
    });
  });

  it('renders bookmarked paths at the top and emits exact jump target on click', async () => {
    mockFsChildren.mockResolvedValue([]);
    mockCaseBookmarksList.mockResolvedValue([
      {
        id: 'bm-1',
        imageId: 'img-bm',
        rootId: 'part:0',
        path: 'root/terminal-logs',
        nodeId: 'ext4:123',
        kind: 'dir',
        label: 'terminal-logs',
        createdAt: '2026-11-19T00:00:00Z',
      },
    ]);

    const mgr = usePathBookmarks(() => 'img-bm');
    await mgr.refreshCaseBookmarks('img-bm');

    mountNav({ imageId: 'img-bm', platform: 'linux' });
    await settle();

    const bmBtns = [...host.querySelectorAll<HTMLButtonElement>('.qj-bookmark-btn')];
    expect(bmBtns.length).toBeGreaterThanOrEqual(1);
    expect(bmBtns[0]!.textContent).toContain('terminal-logs');

    bmBtns[0]!.click();
    expect(jumps[0]).toEqual({
      imageId: 'img-bm',
      nodeId: 'ext4:123',
    });
  });

  it('shows HarmonyOS NEXT paths (not Android /data/data) for huaweihmos', async () => {
    mockFsChildren.mockImplementation(async ({ parentId }: { parentId: string }) => {
      if (parentId === 'root') {
        return [
          { id: 'dir:mods', name: 'modules', kind: 'dir', size: 0 } as FsNode,
          { id: 'dir:bk', name: '__backup', kind: 'dir', size: 0 } as FsNode,
        ];
      }
      return [];
    });

    mountNav({ imageId: 'img-hmos', platform: 'huaweihmos' });
    await settle();

    const labels = [...host.querySelectorAll('.qj-group-label')].map(el => el.textContent?.trim());
    expect(labels).toEqual(['HarmonyOS NEXT 取证直达']);

    const names = [...host.querySelectorAll('.qj-name')].map(el => el.textContent?.trim());
    expect(names.some(n => n?.includes('Aweme'))).toBe(true);
    expect(names.some(n => n?.includes('/data/data'))).toBe(false);
  });
});
