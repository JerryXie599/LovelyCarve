// @vitest-environment happy-dom
import { createApp, h, nextTick, ref, type App } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  GitDiffResult,
  GitRepoOverview,
  GitTreeListing,
} from '../../../lib/ipc/gitWorkspace';

const mockOverview = vi.fn();
const mockTree = vi.fn();
const mockDiff = vi.fn();
const mockFileRead = vi.fn();
const mockSandboxGit = vi.fn();
const mockSandboxFs = vi.fn();
const mockExport = vi.fn();

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

vi.mock('../../../lib/ipc/gitWorkspace', () => ({
  gitWorkspaceOverview: (...args: unknown[]) => mockOverview(...args),
  gitWorkspaceTree: (...args: unknown[]) => mockTree(...args),
  gitWorkspaceDiff: (...args: unknown[]) => mockDiff(...args),
  gitWorkspaceFileRead: (...args: unknown[]) => mockFileRead(...args),
  gitWorkspaceSandboxGit: (...args: unknown[]) => mockSandboxGit(...args),
  gitWorkspaceSandboxFs: (...args: unknown[]) => mockSandboxFs(...args),
  gitWorkspaceExport: (...args: unknown[]) => mockExport(...args),
}));

vi.mock('../composables/usePluginTools', () => ({
  usePluginTools: () => ({
    itemsByTool: ref({
      git_workspace: [
        {
          toolId: 'git_workspace',
          key: 'part:1/home/kali/wifi-deauth',
          label: 'wifi-deauth',
          sublabel: 'part:1/home/kali/wifi-deauth',
          recordId: 'r1',
          runId: 'run1',
          values: {},
        },
        {
          toolId: 'git_workspace',
          key: 'part:1/usr/src/utils',
          label: 'utils',
          sublabel: 'part:1/usr/src/utils',
          recordId: 'r2',
          runId: 'run1',
          values: {},
        },
      ],
    }),
    loadingByTool: ref({ git_workspace: false }),
    scanningByTool: ref({ git_workspace: false }),
    selectedKeyByTool: ref({ git_workspace: 'part:1/home/kali/wifi-deauth' }),
    selectToolItem: vi.fn(),
    triggerToolScan: vi.fn(),
  }),
}));

import GitCodeBrowser from './GitCodeBrowser.vue';
import GitCommitsPanel from './GitCommitsPanel.vue';
import GitDiffViewer from './GitDiffViewer.vue';
import GitRepoWorkspacePanel from './GitRepoWorkspacePanel.vue';

const BASE_SHA = '87c34a9900000000000000000000000000000000';
const HEAD_SHA = '3f2e60837eaf722198920ecca1e83642465931ba';

function makeOverview(): GitRepoOverview {
  return {
    repoPath: 'part:1/home/kali/wifi-deauth',
    repoName: 'wifi-deauth',
    sourceMode: 'evidence',
    headRef: 'refs/heads/main',
    headSha: HEAD_SHA,
    isDetached: false,
    remotes: [
      {
        name: 'origin',
        url: 'https://github.com/flashnuke/wifi-deauth.git',
        githubRepo: 'flashnuke/wifi-deauth',
        credentialLeak: null,
      },
    ],
    userName: 'flashnuke',
    userEmail: 'dev@example.com',
    branches: [
      {
        name: 'main',
        fullRef: 'refs/heads/main',
        sha: HEAD_SHA,
        isHead: true,
        isRemote: false,
      },
    ],
    tags: [{ name: 'v1.45.1', sha: HEAD_SHA, targetSha: HEAD_SHA }],
    commits: [
      {
        sha: HEAD_SHA,
        shortSha: '3f2e6083',
        parents: [BASE_SHA],
        treeSha: 'f3dd217dce91f3bd81c2f21eb8823244c7824667',
        status: 'reachable',
        statusLabel: 'Reachable',
        author: 'flashnuke <dev@example.com>',
        committer: 'flashnuke',
        timestamp: '2026-02-26 18:56:15',
        timestampUnix: 1772103375,
        summary: 'Merge pull request #39',
        message: 'Merge pull request #39\n\nBody',
        refs: ['HEAD -> main'],
      },
    ],
    reflogs: [],
    recoveries: [],
    statusEntries: [],
    indexCount: 4,
    sandbox: {
      initialized: false,
      path: 'C:/case/sandbox',
      gitAvailable: true,
      dirtyCount: 0,
      stagedCount: 0,
      untrackedCount: 0,
    },
  };
}

function makeDiff(): GitDiffResult {
  return {
    repoPath: 'part:1/home/kali/wifi-deauth',
    sourceMode: 'evidence',
    baseRev: BASE_SHA,
    targetRev: HEAD_SHA,
    baseSha: BASE_SHA,
    targetSha: HEAD_SHA,
    totalAdditions: 1,
    totalDeletions: 0,
    files: [
      {
        path: 'README.md',
        status: 'modified',
        oldSha: null,
        newSha: null,
        additions: 1,
        deletions: 0,
        isBinary: false,
        hunks: [
          {
            oldStart: 1,
            oldLines: 1,
            newStart: 1,
            newLines: 2,
            header: '@@ -1,1 +1,2 @@',
            lines: [
              { kind: 'context', oldLine: 1, newLine: 1, content: '# Title' },
              { kind: 'add', oldLine: null, newLine: 2, content: '' },
            ],
          },
        ],
      },
    ],
  };
}

function makeTree(): GitTreeListing {
  return {
    repoPath: 'part:1/home/kali/wifi-deauth',
    sourceMode: 'evidence',
    rev: 'WORKTREE',
    resolvedSha: HEAD_SHA,
    dirPath: '',
    entries: [
      {
        name: 'README.md',
        path: 'README.md',
        kind: 'file',
        mode: '100644',
        sha: 'abc1234',
        size: 120,
        status: null,
      },
    ],
    readme: {
      path: 'README.md',
      name: 'README.md',
      content: '# Title\n\n> quoted note\n\n- bullet',
    },
  };
}

let app: App | null = null;
let host: HTMLElement | null = null;

async function settle() {
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
    await nextTick();
  }
}

function mountComp(renderFn: () => ReturnType<typeof h>) {
  host = document.createElement('div');
  document.body.append(host);
  app = createApp({ render: renderFn });
  app.mount(host);
}

beforeEach(() => {
  mockOverview.mockReset();
  mockTree.mockReset();
  mockDiff.mockReset();
  mockFileRead.mockReset();
  mockSandboxGit.mockReset();
  mockSandboxFs.mockReset();
  mockExport.mockReset();
  mockOverview.mockResolvedValue(makeOverview());
  mockTree.mockResolvedValue(makeTree());
  mockDiff.mockResolvedValue(makeDiff());
});

afterEach(() => {
  app?.unmount();
  host?.remove();
  app = null;
  host = null;
});

describe('Git UI panels layout and primitives', () => {
  it('renders GitRepoWorkspacePanel with UiSelect, ui-seg, and ui-tabs primitives', async () => {
    mountComp(() =>
      h(GitRepoWorkspacePanel, {
        imageId: 'img-1',
        group: {
          toolId: 'git_workspace',
          pluginId: 'git',
          label: 'Git 仓库',
          icon: 'GitBranch',
          view: 'git_repo',
          itemRecord: 'git_repo',
          itemKey: 'repo_path',
          itemLabel: 'repo_name',
          itemSublabel: 'repo_path',
          primaryFunction: 'git.scan',
          functions: ['git.scan'],
        },
      }),
    );
    await settle();

    expect(host?.querySelector('button.ui-select[role="combobox"]')).not.toBeNull();
    expect(host?.querySelectorAll('.ui-seg .ui-seg-item')).toHaveLength(2);
    expect(host?.querySelectorAll('.ui-tabs .ui-tab')).toHaveLength(5);
  });

  it('renders GitCommitsPanel primary action button and shortened diff SHA range', async () => {
    mountComp(() =>
      h(GitCommitsPanel, {
        imageId: 'img-1',
        overview: makeOverview(),
        useSandbox: false,
      }),
    );
    await settle();

    const primaryBtn = host?.querySelector('.gcp-actions button.ui-btn.sm.primary');
    expect(primaryBtn).not.toBeNull();
    expect(primaryBtn?.textContent).toContain('浏览此版本文件');
    expect(host?.querySelector('.gcp-row-author')?.textContent).toContain('flashnuke');
    expect(host?.querySelector('.gdv-range')?.textContent?.trim()).toBe('87c34a99 → 3f2e6083');
  });

  it('toggles collapsed class cleanly on GitDiffViewer file headers', async () => {
    mountComp(() => h(GitDiffViewer, { diff: makeDiff(), loading: false, error: null }));
    await settle();

    const head = host?.querySelector('.gdv-file-head') as HTMLElement | null;
    expect(head).not.toBeNull();
    expect(head?.classList.contains('collapsed')).toBe(false);
    head?.click();
    await settle();
    expect(head?.classList.contains('collapsed')).toBe(true);
    expect(host?.querySelector('.gdv-file-body')).toBeNull();
  });

  it('renders GitCodeBrowser table cells cleanly without display:flex on td', async () => {
    mountComp(() =>
      h(GitCodeBrowser, {
        imageId: 'img-1',
        overview: makeOverview(),
        useSandbox: false,
        rev: 'WORKTREE',
      }),
    );
    await settle();

    expect(host?.querySelector('select.ui-select.sm.gcb-rev-select')).not.toBeNull();
    expect(host?.querySelector('td.gcb-col-name > .gcb-name-cell')).not.toBeNull();
    expect(host?.querySelector('.ui-icon-btn.xs')).toBeNull();
  });
});
