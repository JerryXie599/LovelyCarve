import { describe, expect, it } from 'vitest';
import type { AnalysisRecord } from '../../../lib/ipc/analysisResults';
import type { PluginPackageInfo } from '../../../lib/ipc/plugins';
import {
  collectPluginToolGroups,
  extractToolItemsFromRecords,
  inferCodeLang,
  isMarkdownFilename,
  parseRepoEvidenceLocation,
  shortSha,
} from './pluginTools';

function makePkg(id: string, analyzers: PluginPackageInfo['analyzers']): PluginPackageInfo {
  return {
    id,
    name: id,
    version: '1.0.0',
    directory: `/plugins/${id}`,
    library: `${id}.dll`,
    libraryPresent: true,
    removable: false,
    memoryLimit: 0,
    fingerprint: 'fp',
    state: 'running',
    error: null,
    permissions: [],
    sdk: null,
    schemaHash: null,
    analyzers,
    filesystems: [],
    records: [],
    locales: {},
  };
}

describe('pluginTools model', () => {
  it('groups analyzers sharing a tool id and prefers evidence scope as primary', () => {
    const pkg = makePkg('org.forensicflow.collector', [
      {
        id: 'git_forensics_files',
        name: 'Single',
        scope: 'files',
        platforms: [],
        category: 'application',
        trigger: 'manually',
        provides: [],
        requires: [],
        tool: {
          id: 'git_workspace',
          label: 'Git 仓库',
          icon: 'GitBranch',
          view: 'git_repo',
          itemRecord: 'git.repo',
          itemKey: 'repo_path',
          itemLabel: 'repo_name',
          itemSublabel: 'repo_path',
        },
      },
      {
        id: 'git_forensics',
        name: 'All',
        scope: 'evidence',
        platforms: [],
        category: 'application',
        trigger: 'manually',
        provides: [],
        requires: [],
        tool: {
          id: 'git_workspace',
          label: 'Git 仓库',
          icon: 'GitBranch',
          view: 'git_repo',
          itemRecord: 'git.repo',
          itemKey: 'repo_path',
          itemLabel: 'repo_name',
          itemSublabel: 'repo_path',
        },
      },
    ]);

    const groups = collectPluginToolGroups([pkg], (_p, _k, fb) => (fb === 'Git 仓库' ? 'Git Repositories' : fb));
    expect(groups).toHaveLength(1);
    expect(groups[0].toolId).toBe('git_workspace');
    expect(groups[0].label).toBe('Git Repositories');
    expect(groups[0].primaryFunction).toBe('org.forensicflow.collector/git_forensics');
    expect(groups[0].functions).toEqual([
      'org.forensicflow.collector/git_forensics_files',
      'org.forensicflow.collector/git_forensics',
    ]);
  });

  it('extracts and deduplicates tool items from analysis records', () => {
    const records: AnalysisRecord[] = [
      {
        recordId: '1',
        runId: 'run-1',
        recordType: 'git.repo',
        title: 'wifi-deauth',
        severity: null,
        values: { repo_path: 'part:1/home/kali/wifi-deauth', repo_name: 'wifi-deauth' },
        sources: [],
        imageId: 'img-1',
        legacy: false,
      },
      {
        recordId: '2',
        runId: 'run-2',
        recordType: 'git.repo',
        title: 'wifi-deauth',
        severity: null,
        values: { repo_path: 'part:1/home/kali/wifi-deauth', repo_name: 'wifi-deauth' },
        sources: [],
        imageId: 'img-1',
        legacy: false,
      },
      {
        recordId: '3',
        runId: 'run-1',
        recordType: 'git.repo',
        title: 'utils',
        severity: null,
        values: { repo_path: 'part:1/usr/src/utils', repo_name: 'utils' },
        sources: [],
        imageId: 'img-1',
        legacy: false,
      },
    ];
    const items = extractToolItemsFromRecords(
      { itemKey: 'repo_path', itemLabel: 'repo_name', itemSublabel: 'repo_path' },
      records,
    );
    expect(items.map(i => [i.key, i.label, i.sublabel])).toEqual([
      ['part:1/home/kali/wifi-deauth', 'wifi-deauth', 'part:1/home/kali/wifi-deauth'],
      ['part:1/usr/src/utils', 'utils', 'part:1/usr/src/utils'],
    ]);
  });

  it('parses repo evidence paths and formats helpers cleanly', () => {
    expect(parseRepoEvidenceLocation('img-1', '/part:1/home/kali/wifi-deauth/')).toEqual({
      imageId: 'img-1',
      rootId: 'part:1',
      path: 'home/kali/wifi-deauth',
    });
    expect(parseRepoEvidenceLocation('img-1', 'root')).toEqual({
      imageId: 'img-1',
      rootId: 'root',
      path: '',
    });
    expect(shortSha('abcdef1234567890')).toBe('abcdef12');
    expect(isMarkdownFilename('README.md')).toBe(true);
    expect(inferCodeLang('main.py')).toBe('Python');
  });
});
