// @vitest-environment happy-dom
import { createApp, h, nextTick, ref, type App } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AnalysisResultsNav from './AnalysisResultsNav.vue';
import type { AnalysisResultNode } from '../../../lib/ipc/analysisResults';

const state = vi.hoisted(() => ({
  selectNode: vi.fn(), reload: vi.fn(), setImageId: vi.fn(), ensureFresh: vi.fn(), openPage: vi.fn(), recordValues: vi.fn(),
}));
const tree = ref<AnalysisResultNode[]>([]);
const selectedNode = ref<AnalysisResultNode | null>(null);
const filters = ref<Record<string, string[]>>({});
const treeLoading = ref(false);
const treeError = ref<string | null>(null);

vi.mock('../composables/useAnalysisResults', () => ({
  useAnalysisResults: () => ({ tree, selectedNode, filters, treeLoading, treeError, ...state }),
}));
vi.mock('../composables/useAnalysisLabels', () => ({
  useAnalysisLabels: () => {
    const nodeLabel = (node: AnalysisResultNode) => node.label;
    return { nodeLabel, nodeLabels: { node: nodeLabel, category: (node: AnalysisResultNode) => node.typeName } };
  },
}));
vi.mock('../../../lib/ipc/analysisResults', async importOriginal => ({
  ...(await importOriginal<typeof import('../../../lib/ipc/analysisResults')>()),
  analysisRecordValues: state.recordValues,
}));
vi.mock('../../tasks/api', () => ({
  usePluginCatalog: () => ({
    packages: ref([
      {
        id: 'org.lovelycarve.harmonyos',
        analyzers: [],
        records: [
          { recordType: 'harmony.file_transfer', fields: [{ id: 'file_name' }, { id: 'source_app' }] },
        ],
      },
    ]),
  }),
}));

let app: App | null = null;
let host: HTMLElement;
const imageId = ref('one');
function result(nodeId: string, imageId: string, label: string, artifactType = nodeId): AnalysisResultNode {
  return {
    nodeId, imageId, label, kind: 'result', pluginId: 'org.lovelycarve.harmonyos', analyzer: 'linuxinfo', function: 'p/linuxinfo',
    artifactType, typeName: 'baseinfo', sourceNodeId: null, targetLabel: 'Kali · part:2', runId: 'run', completedAt: '', childCount: 3, warningCount: 0, legacy: false, children: [],
  };
}
const users = result('users', 'one', '用户列表');
const logins = result('logins', 'one', '登录记录');
const run = { ...users, nodeId: 'linuxinfo', kind: 'function' as const, label: 'Linux 系统信息', warningCount: 2, children: [users, logins] };

beforeEach(() => {
  tree.value = [run, result('other', 'two', '其他检材结果')];
  selectedNode.value = users;
  filters.value = {};
  treeLoading.value = false;
  treeError.value = null;
  vi.clearAllMocks();
  imageId.value = 'one';
  host = document.createElement('div');
  document.body.append(host);
  app = createApp({ render: () => h(AnalysisResultsNav, { imageId: imageId.value, onSelect: state.openPage }) });
  app.mount(host);
});
afterEach(() => { app?.unmount(); app = null; document.body.replaceChildren(); });

describe('AnalysisResultsNav', () => {
  it('groups BCP records by application and account with icons and deletion totals', async () => {
    const messages = {
      ...result('messages', 'one', '群聊消息', 'bcp.group_messages'),
      pluginId: 'org.lovelycarve.bcp',
      childCount: 18,
      bcpGroups: [
        { application: '1230005', account: 'owner', accountLabel: '测试账号', total: 3, deleted: 1 },
        { application: '1260004', account: 'owner', accountLabel: null, total: 4, deleted: 1 },
        { application: '1030063', account: 'owner', accountLabel: null, total: 5, deleted: 0 },
        { application: '1039999', identifiedApplication: '1070109', account: 'maimai', accountLabel: null, total: 4, deleted: 1 },
        { application: '1039999', account: 'unknown', accountLabel: null, total: 2, deleted: 0 },
      ],
    };
    const bcpRun = { ...run, label: 'BCP 取证结果', pluginId: messages.pluginId, children: [messages] };
    tree.value = [{
      ...bcpRun, nodeId: 'evidence', kind: 'evidence', label: '采集结果.zip',
      children: [{ ...bcpRun, nodeId: 'category', kind: 'class', children: [bcpRun] }],
    }];
    selectedNode.value = null;
    await nextTick();
    const button = (label: string) =>
      [...host.querySelectorAll<HTMLButtonElement>('.bcp-row-button')]
        .find(item => item.querySelector('.bcp-name')?.textContent === label)!;
    const ctrip = button('携程旅行');
    expect(ctrip.querySelector('img')?.getAttribute('src')).toContain('ctrip.jpg');
    expect(ctrip.querySelector('.bcp-count')?.textContent?.trim()).toBe('2/7');
    expect(button('Telegram').querySelector('.bcp-count')?.textContent?.trim()).toBe('5');
    expect(button('采集结果.zip')).toBeUndefined();
    expect(button('脉脉').querySelector('img')?.getAttribute('src')).toContain('maimai.jpg');
    ctrip.click();
    await nextTick();
    button('测试账号').click();
    await nextTick();
    button('群聊消息').click();
    expect(state.selectNode).toHaveBeenCalledWith(expect.objectContaining({
      childCount: 7,
      bcpPath: ['采集结果.zip', '携程旅行', '测试账号'],
      bcpScope: { applications: ['1230005', '1260004'], account: 'owner' },
    }), undefined);
  });

  it('collapses single-run categories into one section header and selects clicked results', () => {
    const sections = [...host.querySelectorAll('.dir-section-name')].map(el => el.textContent);
    expect(sections).toEqual(['基本信息与系统']);
    expect(host.querySelectorAll('.run-open')).toHaveLength(0);
    expect(host.querySelector('.dir-section-open .ui-badge.warn')?.textContent).toBe('2');

    const items = [...host.querySelectorAll<HTMLButtonElement>('.result-item')];
    expect(items.map(item => item.querySelector('.result-name')?.textContent)).toEqual(['用户列表', '登录记录']);
    expect(items[0].getAttribute('aria-current')).toBe('true');
    items[1].click();
    expect(state.selectNode).toHaveBeenCalledWith(logins, undefined);
    expect(state.openPage).toHaveBeenCalledOnce();
  });

  it('collapses secondary runs and semantic sub-groups by default, and expands on click without clutter icons', async () => {
    const mk = (id: string, label: string, artifactType: string, runId = 'win_base'): AnalysisResultNode => ({
      ...result(id, 'one', label, artifactType),
      pluginId: 'org.lovelycarve.winevtx',
      analyzer: runId,
      runId,
      targetLabel: 'Windows 11 · part:0',
    });
    const baseChildren = [
      mk('sysinfo', '系统基础信息', 'wininfo.sysinfo'),
      mk('user', '本地用户与密码提示 (SAM)', 'wininfo.user'),
      mk('svc', '系统服务', 'wininfo.service'),
      mk('soft', '安装软件', 'wininfo.software'),
      mk('net', '网卡与网络配置', 'wininfo.network'),
      mk('wifi', 'WiFi 与已知网络配置', 'wininfo.wifi'),
      mk('shim', 'ShimCache 兼容缓存', 'wininfo.shimcache'),
      mk('pref', '可执行程序缓存 (Prefetch)', 'wininfo.prefetch'),
    ];
    const baseRun = { ...baseChildren[0], nodeId: 'run_base', kind: 'function' as const, label: 'Windows 系统基础信息与痕迹分析', children: baseChildren };
    const threatLeaf = mk('threat', '威胁狩猎检出 (ATT&CK)', 'winevtx.threat', 'win_evtx');
    const evtxRun = { ...threatLeaf, nodeId: 'run_evtx', kind: 'function' as const, label: '事件日志分析与威胁狩猎', children: [threatLeaf] };
    tree.value = [baseRun, evtxRun];
    selectedNode.value = null;
    await nextTick();

    const runNames = [...host.querySelectorAll('.run-name')].map(el => el.textContent);
    expect(runNames).toEqual(['系统与痕迹', '事件日志']);
    // Primary run is expanded showing sub-group folders collapsed by default; secondary run is collapsed.
    expect(host.querySelectorAll('.dir-app-open')).toHaveLength(4);
    expect(host.querySelectorAll('.dir-sub-list')).toHaveLength(0);
    expect(host.querySelectorAll('.result-item')).toHaveLength(0);
    // No OS logos repeated on run rows or sub-group headers.
    expect(host.querySelectorAll('.dir-logo')).toHaveLength(0);

    // Click first sub-group folder to expand concise leaves without icons inside sub-list.
    host.querySelector<HTMLButtonElement>('.dir-app-open')!.click();
    await nextTick();
    expect([...host.querySelectorAll('.dir-sub-list .result-name')].map(el => el.textContent)).toEqual([
      '系统基础信息',
      '本地用户',
    ]);
    expect(host.querySelectorAll('.dir-sub-list img')).toHaveLength(0);
  });

  it('drills down into software facets with real logos when expanding a leaf node', async () => {
    const fsend = result('fsend', 'one', '文件快传与下载记录 (FSend)', 'harmony.file_transfer');
    tree.value = [{ ...run, typeName: 'apps', label: '应用与文件', children: [fsend] }];
    state.recordValues.mockResolvedValue({
      total: 2,
      truncated: false,
      values: [
        { value: 'WPS云文档', count: 8 },
        { value: '小红书下载', count: 12 },
      ],
    });
    await nextTick();

    const leafToggle = host.querySelector<HTMLButtonElement>('.dir-leaf-line .run-toggle')!;
    expect(leafToggle).toBeTruthy();
    leafToggle.click();
    await nextTick();
    await nextTick();

    const facetItems = [...host.querySelectorAll<HTMLButtonElement>('.result-item.facet')];
    expect(facetItems.map(i => i.querySelector('.result-name')?.textContent)).toEqual(['WPS云文档', '小红书下载']);
    expect(facetItems[0].querySelector('img')?.getAttribute('src')).toContain('wps.jpg');
    expect(facetItems[1].querySelector('img')?.getAttribute('src')).toContain('xiaohongshu.jpg');

    facetItems[0].click();
    expect(state.selectNode).toHaveBeenCalledWith(fsend, { source_app: ['WPS云文档'] });
  });

  it('collapses a section from its chevron and reopens it when selection changes', async () => {
    const toggle = host.querySelector<HTMLButtonElement>('.dir-section-row .run-toggle')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    toggle.click();
    await nextTick();
    await nextTick();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(host.querySelectorAll('.result-item')).toHaveLength(0);

    selectedNode.value = logins;
    await nextTick();
    await nextTick();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(host.querySelector('.result-item[aria-current="true"] .result-name')?.textContent).toBe('登录记录');
  });

  it('narrows entries with search and clears with Escape', async () => {
    const input = host.querySelector<HTMLInputElement>('input')!;
    input.focus();
    input.value = '登录';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await nextTick();
    expect([...host.querySelectorAll('.result-item .result-name')].map(item => item.textContent)).toEqual(['登录记录']);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await nextTick();
    expect(input.value).toBe('');
    expect(host.querySelectorAll('.result-item')).toHaveLength(2);
  });

  it('announces directory loading, errors and empty states', async () => {
    tree.value = [];
    treeLoading.value = true;
    await nextTick();
    expect(host.querySelector('[role="status"]')?.textContent).toContain('正在读取');
    treeLoading.value = false;
    treeError.value = '读取失败';
    await nextTick();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('读取失败');
    treeError.value = null;
    await nextTick();
    expect(host.querySelector('[role="status"]')?.textContent).toContain('暂无分析结果');
  });
});
