import { describe, expect, it } from 'vitest';
import type { ContestWorkspace } from '../../../lib/ipc/contest';
import {
  extractAssetRefs,
  filterQuestions,
  formatPinTargetLabel,
  parseWriteupBlocks,
  replaceMarkdownAssetUrls,
  summarizeWorkspace,
} from './contest';

const sampleWorkspace: ContestWorkspace = {
  version: 1,
  title: '美亚杯示例',
  sourceFormat: 'markdown',
  caseWpMd: '',
  exhibits: [
    { id: 'VTM-1', name: 'VTM-1', aliases: ['vtm1'], imageId: 'img-1', notesMd: '', wpMd: '' },
    { id: 'VTM-2', name: 'VTM-2', aliases: [], imageId: null, notesMd: '', wpMd: '' },
  ],
  questions: [
    {
      id: 'Q01',
      exhibitId: 'VTM-1',
      number: 1,
      stem: '系统版本号是多少',
      options: ['A. 10.0.19045', 'B. 10.0.22631'],
      answer: '10.0.22631',
      status: 'answered',
      notes: [],
      wpSectionMd: '',
      updatedAt: '2026-11-26T00:00:00Z',
    },
    {
      id: 'Q02',
      exhibitId: 'VTM-2',
      number: 2,
      stem: '最后联系人的社交账号',
      options: [],
      answer: '',
      status: 'review',
      notes: [],
      wpSectionMd: '',
      updatedAt: '2026-11-26T00:00:00Z',
    },
  ],
  pins: [
    {
      id: 'pin-01',
      questionId: 'Q01',
      exhibitId: 'VTM-1',
      title: 'SAM 注册表',
      summary: '版本键',
      source: 'cli',
      target: { kind: 'file', imageId: 'img-1', path: '/VTM-1/part:0/Windows/SAM' },
      screenshotRel: 'assets/pin-01-abcd.png',
      createdAt: '2026-11-26T00:00:00Z',
    },
  ],
  updatedAt: '2026-11-26T00:00:00Z',
};

describe('contest model helpers', () => {
  it('summarizes workspace counts accurately', () => {
    const summary = summarizeWorkspace(sampleWorkspace);
    expect(summary).toEqual({
      totalQuestions: 2,
      answeredQuestions: 1,
      reviewQuestions: 1,
      stuckQuestions: 0,
      totalExhibits: 2,
      boundExhibits: 1,
      totalPins: 1,
      capturedPins: 1,
    });
  });

  it('filters questions by exhibit, status, and keyword', () => {
    expect(filterQuestions(sampleWorkspace, { exhibitId: 'VTM-1' })).toHaveLength(1);
    expect(filterQuestions(sampleWorkspace, { status: 'review' })[0]?.id).toBe('Q02');
    expect(filterQuestions(sampleWorkspace, { keyword: '社交' })[0]?.id).toBe('Q02');
  });

  it('extracts and resolves relative ./assets URLs cleanly', () => {
    const md = '正文\n\n![证据1](./assets/pin-01-abcd.png)\n![证据2](assets/pin-02.png)';
    expect(extractAssetRefs(md)).toEqual(['assets/pin-01-abcd.png', 'assets/pin-02.png']);
    const replaced = replaceMarkdownAssetUrls(md, {
      'assets/pin-01-abcd.png': 'data:image/png;base64,AAA',
    });
    expect(replaced).toContain('![证据1](data:image/png;base64,AAA)');
    expect(replaced).toContain('![证据2](assets/pin-02.png)');
  });

  it('parses writeup blocks with anchors, code fences, and screenshots', () => {
    const blocks = parseWriteupBlocks(
      '# Title\n\n<a id="wp-Q01"></a>\n\n```bash\nlovelycarve-cli ls /\n```\n\n![图](./assets/a.png)',
    );
    expect(blocks.map(b => b.kind)).toEqual(['heading', 'anchor', 'code', 'image']);
  });

  it('formats Pin targets concisely', () => {
    expect(
      formatPinTargetLabel({
        kind: 'hex_range',
        imageId: 'img-1',
        path: '/nas/a.bin',
        offset: 0x20,
        length: 16,
      }),
    ).toBe('/nas/a.bin @ 0x20+0x10');
  });
});
