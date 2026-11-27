import { describe, expect, it } from 'vitest';
import type { AnalysisResultNode } from '../../../lib/ipc/analysisResults';
import type { PluginPackageInfo } from '../../../lib/ipc/plugins';
import type { AnalysisResultGroup } from './analysisResultView';
import { buildResultDirectorySections, detectSoftwareFacetColumn, simplifyLabel } from './resultDirectoryTree';

const tr = (text: string) => text;

function leaf(artifactType: string, label: string, count = 3, analyzer = 'harmony_apps', pluginId = 'org.lovelycarve.harmonyos'): AnalysisResultNode {
  return {
    nodeId: `n:${artifactType}`,
    imageId: 'img-1',
    label,
    kind: 'result',
    pluginId,
    analyzer,
    function: `${pluginId}/${analyzer}`,
    artifactType,
    typeName: analyzer,
    sourceNodeId: null,
    targetLabel: 'VYG-AL30',
    runId: `run:${analyzer}`,
    completedAt: '',
    childCount: count,
    warningCount: 0,
    legacy: false,
    children: [],
  };
}

function group(
  id: string,
  label: string,
  category: string,
  typeName: string,
  results: AnalysisResultNode[],
  warningCount = 0,
  target = 'VYG-AL30',
): AnalysisResultGroup {
  return {
    id,
    label,
    category,
    target,
    run: { ...results[0], kind: 'function', nodeId: `fn:${id}`, label, typeName, warningCount, children: results },
    warningCount,
    legacy: false,
    results,
  };
}

describe('simplifyLabel', () => {
  it('strips OS prefixes, trailing technical parens, and verbose phrasing while keeping editions', () => {
    expect(simplifyLabel('iOS应用信息（清单/安装/时长/浏览器）')).toBe('应用信息');
    expect(simplifyLabel('iOS相册')).toBe('相册');
    expect(simplifyLabel('HarmonyOS NEXT 设备与备份概览')).toBe('设备概览');
    expect(simplifyLabel('文件快传与下载记录 (FSend)')).toBe('快传与下载');
    expect(simplifyLabel('短信与软删短信 (SMS/RCS)')).toBe('短信');
    expect(simplifyLabel('本地用户与密码提示 (SAM)')).toBe('本地用户');
    expect(simplifyLabel('Win11 记事本缓存')).toBe('记事本');
    expect(simplifyLabel('Win10/11 系统通知')).toBe('系统通知');
    expect(simplifyLabel('凭据文件与 Defender 检测 (含 Vault)')).toBe('凭据与安全');
    expect(simplifyLabel('抖音(分身版)')).toBe('抖音(分身版)');
  });

  it('strips app prefix inside app sub-folders', () => {
    expect(simplifyLabel('WhatsApp 账号', { stripAppPrefix: 'WhatsApp' })).toBe('账号');
    expect(simplifyLabel('WhatsApp 聊天消息', { stripAppPrefix: 'WhatsApp' })).toBe('聊天消息');
  });
});

describe('buildResultDirectorySections', () => {
  it('collapses 1-to-1 single-run categories, groups multi-table apps, and omits generic logos', () => {
    const groups: AnalysisResultGroup[] = [
      group('hm_ov', 'HarmonyOS NEXT 系统与备份概览', '基本信息', 'baseinfo', [
        leaf('harmony.device', 'HarmonyOS NEXT 设备与备份概览', 1, 'harmony_overview'),
        leaf('harmony.sim', 'SIM 卡信息', 2, 'harmony_overview'),
      ]),
      group('hm_comm', 'HarmonyOS NEXT 通信与社交取证', '通信与社交', 'communication', [
        leaf('harmony.call', '通话记录', 34, 'harmony_comm'),
        leaf('ios.wa_account', 'WhatsApp 账号', 1, 'ios_whatsapp'),
        leaf('ios.wa_contact', 'WhatsApp 联系人', 11, 'ios_whatsapp'),
        leaf('ios.wa_msg', 'WhatsApp 聊天消息', 48, 'ios_whatsapp'),
      ]),
    ];

    const sections = buildResultDirectorySections(groups, node => node.label, [], tr);
    expect(sections.map(s => [s.label, s.logo, s.runs.length, s.children.length])).toEqual([
      ['基本信息与系统', null, 0, 2],
      ['通信与社交', null, 0, 2],
    ]);
    const commChildren = sections[1].children;
    expect(commChildren[0]).toMatchObject({ kind: 'leaf', label: '通话记录', count: 34, logo: null });
    expect(commChildren[1]).toMatchObject({ kind: 'app', label: 'WhatsApp', count: 60 });
    if (commChildren[1].kind === 'app') {
      expect(commChildren[1].logo).toContain('whatsapp.jpg');
      expect(commChildren[1].leaves.map(l => [l.label, l.count])).toEqual([
        ['账号', 1],
        ['联系人', 11],
        ['聊天消息', 48],
      ]);
    }
  });

  it('clusters dense runs (>= 8 leaves) into semantic sub-groups and suppresses identical targets', () => {
    const winLeaves = [
      leaf('wininfo.sysinfo', '系统基础信息', 27, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.user', '本地用户与密码提示 (SAM)', 8, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.pwd_policy', '账户与密码策略 (SAM)', 11, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.service', '系统服务', 727, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.software', '安装软件', 136, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.network', '网卡与网络配置', 8, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.wifi', 'WiFi 与已知网络配置', 10, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.shimcache', 'ShimCache 兼容缓存', 679, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.prefetch', '可执行程序缓存 (Prefetch)', 352, 'win_baseinfo', 'org.lovelycarve.winevtx'),
      leaf('wininfo.defender', '凭据文件与 Defender 检测', 22, 'win_baseinfo', 'org.lovelycarve.winevtx'),
    ];
    const groups: AnalysisResultGroup[] = [
      group('win_base', 'Windows 系统基础信息与痕迹分析', '基本信息', 'baseinfo', winLeaves, 0, 'Windows 11 · part:0'),
      group('win_evtx', '事件日志分析与威胁狩猎', '基本信息', 'baseinfo', [
        leaf('winevtx.threat', '威胁狩猎检出 (ATT&CK)', 10, 'winevtx', 'org.lovelycarve.winevtx'),
      ], 0, 'Windows 11 · part:0'),
    ];

    const sections = buildResultDirectorySections(groups, node => node.label, [], tr);
    expect(sections).toHaveLength(1);
    expect(sections[0].runs.map(r => [r.label, r.target, r.logo])).toEqual([
      ['系统与痕迹', '', null],
      ['事件日志', '', null],
    ]);
    const baseChildren = sections[0].runs[0].children;
    const groupNodes = baseChildren.filter(c => c.kind === 'group');
    expect(groupNodes.map(g => [g.label, g.kind === 'group' ? g.leaves.map(l => l.label) : []])).toEqual([
      ['系统与账户', ['系统基础信息', '本地用户', '账户策略']],
      ['软件与服务', ['系统服务', '安装软件']],
      ['网络与外设', ['网络配置', 'WiFi 记录']],
      ['用户与执行痕迹', ['ShimCache', '执行缓存']],
    ]);
    const securityLeaf = baseChildren.find(c => c.kind === 'leaf');
    expect(securityLeaf).toMatchObject({ kind: 'leaf', label: '凭据与安全', logo: null });
  });

  it('keeps multi-run sections expanded to level-2 runs with concise names', () => {
    const groups: AnalysisResultGroup[] = [
      group('ios_media', 'iOS媒体（相册/笔记）', '媒体信息', 'fmedia', [
        leaf('ios.photos', '相册资产', 19, 'ios_media', 'org.lovelycarve.iosbackup'),
      ]),
      group('ios_photos', '照片库深度解析（Photos 完整分析）', '照片库', 'fphotos', [
        leaf('ios.photos_detail', '照片资产明细', 19, 'ios_photos', 'org.lovelycarve.iosbackup'),
      ]),
    ];
    const sections = buildResultDirectorySections(groups, node => node.label, [], tr);
    expect(sections).toHaveLength(1);
    expect(sections[0].label).toBe('相册与个人数据');
    expect(sections[0].runs.map(r => r.label)).toEqual(['媒体', '照片库']);
  });
});

describe('detectSoftwareFacetColumn', () => {
  it('picks source_app / app / browser in priority order from declared plugin schema', () => {
    const packages = [{
      id: 'org.lovelycarve.harmonyos',
      records: [
        { recordType: 'harmony.file_transfer', fields: [{ id: 'file_name' }, { id: 'source_app' }] },
        { recordType: 'harmony.browser', fields: [{ id: 'url' }, { id: 'browser' }] },
        { recordType: 'harmony.call', fields: [{ id: 'number' }] },
      ],
    }] as unknown as PluginPackageInfo[];

    expect(detectSoftwareFacetColumn(leaf('harmony.file_transfer', '快传'), packages)).toBe('source_app');
    expect(detectSoftwareFacetColumn(leaf('harmony.browser', '浏览器'), packages)).toBe('browser');
    expect(detectSoftwareFacetColumn(leaf('harmony.call', '通话'), packages)).toBeNull();
  });
});
