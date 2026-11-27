import type { Translator } from '../../../i18n/translate';
import type { AnalysisResultNode } from '../../../lib/ipc/analysisResults';
import type { PluginPackageInfo } from '../../../lib/ipc/plugins';
import { resolveBrandLogo, resolveRecordLogo } from '../../../utils/appIcons';
import type { AnalysisResultGroup } from './analysisResultView';

export interface DirectoryLeafNode {
  kind: 'leaf';
  id: string;
  label: string;
  title: string;
  count: number;
  logo: string | null;
  node: AnalysisResultNode;
  facetColumn: string | null;
  stripAppPrefix?: string;
}

export interface DirectoryAppNode {
  kind: 'app';
  id: string;
  label: string;
  title: string;
  count: number;
  logo: string | null;
  leaves: DirectoryLeafNode[];
}

export interface DirectoryGroupNode {
  kind: 'group';
  id: string;
  label: string;
  title: string;
  count: number;
  logo: null;
  leaves: DirectoryLeafNode[];
}

export type DirectoryChild = DirectoryLeafNode | DirectoryAppNode | DirectoryGroupNode;

export interface DirectoryRunNode {
  kind: 'run';
  id: string;
  label: string;
  title: string;
  target: string;
  count: number;
  warningCount: number;
  legacy: boolean;
  logo: string | null;
  run: AnalysisResultNode | null;
  children: DirectoryChild[];
}

export interface DirectorySectionNode {
  kind: 'section';
  id: string;
  label: string;
  title: string;
  count: number;
  warningCount: number;
  logo: null;
  /** When the section has a single run, the section header itself is clickable to open the run overview. */
  run: AnalysisResultNode | null;
  singleRunTarget: string;
  legacy: boolean;
  runs: DirectoryRunNode[];
  /** Populated when collapsed (only 1 run), empty when multiple runs live under `runs`. */
  children: DirectoryChild[];
}

const OS_PREFIX_RE = /^(?:HarmonyOS\s*NEXT|HarmonyOS|鸿蒙原生|鸿蒙|Win(?:dows)?\s*(?:10\/11|11|10)?|iOS|Android|Linux)\s*/i;
const KEEP_PARENS_RE = /(?:分身|企业|个人|克隆|双开|多开)/;
const TRAILING_PARENS_RE = /\s*(?:\([^()]*\)|（[^（）]*）)\s*$/;
const TRAILING_ANALYZER_SUFFIX_RE = /(?:综合取证分析|深度取证分析|深度分析|深度解析|取证分析)$/;
const SOFTWARE_FACET_PRIORITY = ['source_app', 'app', 'browser', 'app_name', 'app_instance'] as const;

const CONCISE_PHRASES: ReadonlyArray<readonly [pattern: RegExp, replacement: string]> = [
  [/系统基础信息与痕迹(?:分析)?/g, '系统与痕迹'],
  [/事件日志分析与威胁.*/g, '事件日志'],
  [/本地用户与密码提示/g, '本地用户'],
  [/账户与密码策略/g, '账户策略'],
  [/网卡与网络配置/g, '网络配置'],
  [/用户配置与默认浏览器/g, '用户配置'],
  [/USB\s*设备与最近使用记录/gi, 'USB 记录'],
  [/ShimCache\s*兼容缓存/gi, 'ShimCache'],
  [/Wi-?Fi\s*与已知网络配置/gi, 'WiFi 记录'],
  [/应用程序运行记录/g, '程序运行'],
  [/用户操作与终端痕迹/g, '操作与终端'],
  [/快捷方式与最近访问/g, '快捷方式'],
  [/可执行程序缓存/g, '执行缓存'],
  [/记事本缓存/g, '记事本'],
  [/凭据文件与\s*Defender\s*检测.*/gi, '凭据与安全'],
  [/威胁狩猎检出/g, '威胁检出'],
  [/防线变更与日志清理/g, '日志清理'],
  [/设备与备份概览/g, '设备概览'],
  [/备份应用与模块清单/g, '应用清单'],
  [/系统通知策略配置/g, '通知策略'],
  [/系统设置与安全开关/g, '系统设置'],
  [/系统加密数据库密钥元数据/g, '数据库密钥'],
  [/文件快传与下载记录/g, '快传与下载'],
  [/浏览器历史、搜索与书签/g, '浏览器记录'],
  [/应用数据目录与系统记录/g, '应用数据'],
  [/地图搜索与导航记录/g, '地图与导航'],
  [/短信与软删短信/g, '短信'],
];

function stripTechnicalTrailingParens(text: string): string {
  let current = text.trim();
  while (true) {
    const match = current.match(TRAILING_PARENS_RE);
    if (!match || match.index === undefined) break;
    if (KEEP_PARENS_RE.test(match[0])) break;
    const prefix = current.slice(0, match.index).trim();
    if (prefix.length < 2) break;
    current = prefix;
  }
  return current;
}

/** Strips technical parens, redundant OS prefixes, and verbose forensic phrasing while keeping a readable label. */
export function simplifyLabel(raw: string, opts?: { stripAppPrefix?: string }): string {
  let label = stripTechnicalTrailingParens(raw);

  const withoutOs = label.replace(OS_PREFIX_RE, '').trim();
  if (withoutOs.length >= 2) label = withoutOs;

  for (const [pattern, replacement] of CONCISE_PHRASES) {
    label = label.replace(pattern, replacement).trim();
  }

  const withoutSuffix = label.replace(TRAILING_ANALYZER_SUFFIX_RE, '').trim();
  if (withoutSuffix.length >= 2) label = withoutSuffix;

  if (opts?.stripAppPrefix) {
    const escaped = opts.stripAppPrefix.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const withoutApp = label.replace(new RegExp(`^${escaped}[\\s·_-]*`, 'i'), '').trim();
    if (withoutApp.length >= 2) label = withoutApp;
  }
  return label;
}

const APP_GROUP_RULES: ReadonlyArray<readonly [key: string, label: string, patterns: readonly string[]]> = [
  ['whatsapp', 'WhatsApp', ['whatsapp']],
  ['icloud', 'iCloud', ['icloud']],
  ['telegram', 'Telegram', ['telegram']],
  ['safari', 'Safari', ['safari']],
];

function detectMultiTableApp(node: AnalysisResultNode, label: string): { key: string; label: string } | null {
  const haystack = `${node.artifactType} ${node.analyzer} ${label}`.toLowerCase();
  for (const [key, appLabel, patterns] of APP_GROUP_RULES) {
    if (patterns.some(p => haystack.includes(p))) return { key, label: appLabel };
  }
  return null;
}

/** Finds the schema field best suited for per-software drill-down on a record type. */
export function detectSoftwareFacetColumn(
  node: AnalysisResultNode,
  packages: readonly PluginPackageInfo[],
): string | null {
  if (!node.pluginId || !node.artifactType) return null;
  const pkg = packages.find(item => item.id === node.pluginId);
  const schema = pkg?.records.find(record => record.recordType === node.artifactType);
  if (!schema) return null;
  for (const candidate of SOFTWARE_FACET_PRIORITY) {
    if (schema.fields.some(field => field.id === candidate)) return candidate;
  }
  return null;
}

const SECTION_RULES: ReadonlyArray<readonly [id: string, key: string, types: readonly string[]]> = [
  ['baseinfo', '基本信息与系统', ['baseinfo', 'abasic', 'dconnect', 'epower', 'glog', 'hios', 'system', 'device', 'hardware', 'keychain', 'security', 'bluetooth', 'wifi', 'sim']],
  ['communication', '通信与社交', ['communication', 'ccomm', 'ginteraction', 'iwhatsapp', 'comm', 'whatsapp', 'message', 'call', 'contact']],
  ['pim', '相册与个人数据', ['pim', 'fmedia', 'fphotos', 'media', 'photos', 'note', 'calendar']],
  ['apps', '应用与文件', ['apps', 'bappinfo', 'jicloud', 'kairdrop', 'application', 'app', 'browserinfo', 'icloud', 'airdrop', 'fileanalyzer', 'usbimage']],
];

const SEMANTIC_SUBGROUPS: ReadonlyArray<readonly [id: string, key: string, patterns: readonly string[]]> = [
  [
    'network_devices',
    '网络与外设',
    ['network', 'wifi', 'wlan', 'nic', 'bluetooth', 'usb', 'net_conn', 'firewall', '网卡', '网络配置', 'wifi', '无线', '蓝牙', 'usb', 'u盘', '防火墙'],
  ],
  [
    'sys_accounts',
    '系统与账户',
    ['sysinfo', 'system', 'device', 'hardware', 'user', 'account', 'profile', 'pwd_policy', 'sam', 'notif', 'setting', 'sim', 'battery', 'dbkey', '系统基础', '设备概览', '硬件', '本地用户', '账户策略', '用户配置', '系统通知', '通知策略', '系统设置', '数据库密钥', 'sim'],
  ],
  [
    'software_services',
    '软件与服务',
    ['service', 'software', 'startup', 'autostart', 'scheduled', 'task', 'assoc', 'open_with', 'applist', 'appdata', '系统服务', '安装软件', '自启动', '计划任务', '默认打开方式', '应用清单', '应用数据'],
  ],
  [
    'user_execution',
    '用户与执行痕迹',
    ['shimcache', 'prefetch', 'amcache', 'userassist', 'exec_cache', 'user_trace', 'lnk', 'recent', 'desktop', 'recycle', 'notepad', 'timeline', 'activity', 'activities', 'terminal', 'shell', 'browser', 'fsend', 'download', 'map', '程序运行', '执行缓存', '操作与终端', '快捷方式', '桌面', '回收站', '记事本', '时间轴', '浏览器', '快传', '地图'],
  ],
  [
    'security_logs',
    '安全与日志',
    ['threat', 'hunting', 'attack', 'att&ck', 'evtx', 'security', 'defender', 'cred', 'clear', 'logon', 'login', 'audit', 'journal', '威胁', '登录会话', '安全事件', '进程与服务执行', '日志清理', '防线', '凭据'],
  ],
];

function classifySemanticSubgroup(leaf: DirectoryLeafNode): string | null {
  const haystack = `${leaf.node.artifactType} ${leaf.title} ${leaf.label}`.toLowerCase();
  for (const [id, _key, patterns] of SEMANTIC_SUBGROUPS) {
    if (patterns.some(p => haystack.includes(p))) return id;
  }
  return null;
}

function logicalSectionOf(group: AnalysisResultGroup, tr: Translator): { id: string; label: string; rawType: string } {
  const rawType = (group.run?.typeName || group.results[0]?.typeName || '').toLowerCase();
  for (const [id, zhLabel, types] of SECTION_RULES) {
    if (types.includes(rawType)) {
      return { id, label: tr(zhLabel), rawType };
    }
  }
  const catLower = group.category.toLowerCase();
  for (const [id, zhLabel, types] of SECTION_RULES) {
    if (types.some(t => catLower.includes(t))) {
      return { id, label: tr(zhLabel), rawType };
    }
  }
  return { id: rawType || group.category, label: simplifyLabel(group.category), rawType };
}

function makeLeafNode(
  node: AnalysisResultNode,
  full: string,
  packages: readonly PluginPackageInfo[],
  stripAppPrefix?: string,
): DirectoryLeafNode {
  return {
    kind: 'leaf',
    id: node.nodeId,
    label: simplifyLabel(full, stripAppPrefix ? { stripAppPrefix } : undefined),
    title: full,
    count: node.childCount,
    logo: resolveRecordLogo(node, full),
    node,
    facetColumn: detectSoftwareFacetColumn(node, packages),
    ...(stripAppPrefix ? { stripAppPrefix } : {}),
  };
}

function buildChildren(
  results: AnalysisResultNode[],
  nodeLabel: (node: AnalysisResultNode) => string,
  packages: readonly PluginPackageInfo[],
  tr: Translator,
  runId: string,
): DirectoryChild[] {
  const appBuckets = new Map<string, { label: string; nodes: AnalysisResultNode[] }>();
  const singleOrAppKey = new Map<string, string>();
  for (const node of results) {
    const raw = nodeLabel(node);
    const app = detectMultiTableApp(node, raw);
    if (app) {
      singleOrAppKey.set(node.nodeId, app.key);
      const bucket = appBuckets.get(app.key) ?? { label: app.label, nodes: [] };
      bucket.nodes.push(node);
      appBuckets.set(app.key, bucket);
    }
  }

  const emittedApps = new Set<string>();
  const appFolders: DirectoryAppNode[] = [];
  const plainLeaves: DirectoryLeafNode[] = [];

  for (const node of results) {
    const appKey = singleOrAppKey.get(node.nodeId);
    const bucket = appKey ? appBuckets.get(appKey) : undefined;
    if (appKey && bucket && bucket.nodes.length >= 2) {
      if (emittedApps.has(appKey)) continue;
      emittedApps.add(appKey);
      const appLogo = resolveBrandLogo(bucket.label);
      const leaves = bucket.nodes.map(child => {
        const full = nodeLabel(child);
        const leaf = makeLeafNode(child, full, packages, bucket.label);
        return { ...leaf, logo: leaf.logo ?? appLogo };
      });
      appFolders.push({
        kind: 'app',
        id: `app:${appKey}:${bucket.nodes[0].runId}`,
        label: bucket.label,
        title: bucket.label,
        count: leaves.reduce((sum, leaf) => sum + leaf.count, 0),
        logo: appLogo,
        leaves,
      });
      continue;
    }
    plainLeaves.push(makeLeafNode(node, nodeLabel(node), packages));
  }

  if (plainLeaves.length < 8) {
    return [...plainLeaves, ...appFolders];
  }

  const subgroupBuckets = new Map<string, DirectoryLeafNode[]>();
  const ungrouped: DirectoryLeafNode[] = [];
  for (const leaf of plainLeaves) {
    const groupId = classifySemanticSubgroup(leaf);
    if (!groupId) {
      ungrouped.push(leaf);
      continue;
    }
    const list = subgroupBuckets.get(groupId) ?? [];
    list.push(leaf);
    subgroupBuckets.set(groupId, list);
  }

  const children: DirectoryChild[] = [];
  const subgroupOrder = ['sys_accounts', 'software_services', 'network_devices', 'user_execution', 'security_logs'] as const;
  for (const groupId of subgroupOrder) {
    const list = subgroupBuckets.get(groupId);
    if (!list?.length) continue;
    if (list.length === 1) {
      ungrouped.push(list[0]);
      continue;
    }
    const zhKey = SEMANTIC_SUBGROUPS.find(([id]) => id === groupId)![1];
    const label = tr(zhKey);
    children.push({
      kind: 'group',
      id: `grp:${runId}:${groupId}`,
      label,
      title: label,
      count: list.reduce((sum, item) => sum + item.count, 0),
      logo: null,
      leaves: list,
    });
  }

  children.push(...ungrouped, ...appFolders);
  return children;
}

function sumChildCount(children: DirectoryChild[]): number {
  return children.reduce((sum, child) => sum + child.count, 0);
}

function isSpecificTarget(target: string): boolean {
  const trimmed = target.trim();
  return Boolean(trimmed) && trimmed !== '整个检材' && trimmed !== 'Entire evidence';
}

/**
 * Builds the calm multi-level directory tree:
 * - Collapses single-run categories to avoid duplicate level-1/level-2 headers
 * - Clusters dense flat runs (>= 8 leaves) into semantic sub-groups
 * - Groups multi-table apps (e.g. Douyin) into brand App folders
 * - Suppresses redundant identical target subtitles
 */
export function buildResultDirectorySections(
  groups: AnalysisResultGroup[],
  nodeLabel: (node: AnalysisResultNode) => string,
  packages: readonly PluginPackageInfo[],
  tr: Translator,
): DirectorySectionNode[] {
  const globalTargets = new Set(groups.map(g => g.target.trim()).filter(isSpecificTarget));
  const hideAllTargets = globalTargets.size <= 1;

  const bySection = new Map<string, { id: string; label: string; runs: DirectoryRunNode[] }>();
  for (const group of groups) {
    const sec = logicalSectionOf(group, tr);
    const children = buildChildren(group.results, nodeLabel, packages, tr, group.id);
    const count = sumChildCount(children);
    const runLabel = simplifyLabel(group.label);
    const target = !hideAllTargets && isSpecificTarget(group.target) ? group.target.trim() : '';
    const runNode: DirectoryRunNode = {
      kind: 'run',
      id: group.id,
      label: runLabel,
      title: [group.category, group.label, group.target].filter(Boolean).join(' · '),
      target,
      count,
      warningCount: group.warningCount,
      legacy: group.legacy,
      logo: resolveBrandLogo(group.label) ?? resolveBrandLogo(group.run?.pluginId ?? ''),
      run: group.run,
      children,
    };
    const existing = bySection.get(sec.id);
    if (existing) existing.runs.push(runNode);
    else bySection.set(sec.id, { id: sec.id, label: sec.label, runs: [runNode] });
  }

  const ordered = [...bySection.values()].sort((left, right) => {
    const li = SECTION_RULES.findIndex(([id]) => id === left.id);
    const ri = SECTION_RULES.findIndex(([id]) => id === right.id);
    return (li < 0 ? SECTION_RULES.length : li) - (ri < 0 ? SECTION_RULES.length : ri);
  });

  return ordered.map(sec => {
    const sectionTargets = new Set(sec.runs.map(r => r.target).filter(Boolean));
    const normalizedRuns = sectionTargets.size <= 1
      ? sec.runs.map(run => ({ ...run, target: '' }))
      : sec.runs;
    const totalCount = normalizedRuns.reduce((sum, r) => sum + r.count, 0);
    const totalWarnings = normalizedRuns.reduce((sum, r) => sum + r.warningCount, 0);
    if (normalizedRuns.length === 1) {
      const only = normalizedRuns[0];
      return {
        kind: 'section',
        id: sec.id,
        label: sec.label,
        title: only.title,
        count: totalCount,
        warningCount: totalWarnings,
        logo: null,
        run: only.run,
        singleRunTarget: only.target,
        legacy: only.legacy,
        runs: [],
        children: only.children,
      };
    }
    return {
      kind: 'section',
      id: sec.id,
      label: sec.label,
      title: sec.label,
      count: totalCount,
      warningCount: totalWarnings,
      logo: null,
      run: null,
      singleRunTarget: '',
      legacy: false,
      runs: normalizedRuns,
      children: [],
    };
  });
}

function childContainsNode(child: DirectoryChild, nodeId: string): string | null {
  if (child.kind === 'leaf') return child.node.nodeId === nodeId ? '' : null;
  if (child.leaves.some(leaf => leaf.node.nodeId === nodeId)) return child.id;
  return null;
}

/** Collects ancestor group ids containing `nodeId` under a section for reveal / selection checks. */
export function sectionContainsNode(
  section: DirectorySectionNode,
  nodeId: string,
): { runId?: string; groupId?: string } | null {
  if (section.run?.nodeId === nodeId) return {};
  for (const child of section.children) {
    const hit = childContainsNode(child, nodeId);
    if (hit !== null) return hit ? { groupId: hit } : {};
  }
  for (const run of section.runs) {
    if (run.run?.nodeId === nodeId) return { runId: run.id };
    for (const child of run.children) {
      const hit = childContainsNode(child, nodeId);
      if (hit !== null) return hit ? { runId: run.id, groupId: hit } : { runId: run.id };
    }
  }
  return null;
}
