import { tr } from '../../../composables/useLocale';
// 快速分析的数据组装:适用于选中系统的系统级功能与检材级功能,按插件归并、
// 系统级平级优先 + 软件专项其后分组与收藏持久化。
import { computed, ref } from 'vue';

import { usePersistedRef } from '../../../composables/usePersistedRef';
import type { DetectedSystem, SchedFunction } from '../../../lib/ipc/tasks';
import { declaredPluginText, pluginTextKey, type PluginTextLookup } from '../../../lib/models/pluginText';
import type { ImageId } from '../../../lib/ipc/ids';

/** longtime 耗时任务分类固定在侧栏末尾。 */
export const LONGTIME_TYPE = 'longtime';

/** 快速分析的勾选单位:一个插件在选中系统(与整个检材)上的全部功能。 */
export interface QuickPlugin {
  id: string;
  name: string;
  typeName: string;
  functions: SchedFunction[];
}

export interface PluginGroup {
  key: string;
  label: string;
  plugins: QuickPlugin[];
}

export interface PluginTierInfo {
  tier: 'os' | 'software' | 'longtime';
  osFamily: string | null;
  osRank: number;
}

const FAVOURITE_KEY = 'lovelycarve-quick-analysis-favourites';
const SYSTEM_MEMORY_PREFIX = 'lovelycarve-quick-analysis-system:';

const OS_FAMILIES = ['windows', 'linux', 'ios', 'harmony', 'macos', 'android'] as const;

export function normalizePlatformFamily(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const lower = raw.trim().toLowerCase();
  if (!lower) return null;
  if (lower.includes('harmony') || lower.includes('hmos') || lower.includes('huawei')) return 'harmony';
  if (lower.includes('win')) return 'windows';
  if (lower.includes('linux')) return 'linux';
  if (lower === 'ios' || lower.includes('iosbackup') || lower.includes('iphone') || lower.includes('ipad')) return 'ios';
  if (lower.includes('mac') || lower === 'darwin') return 'macos';
  if (lower.includes('android')) return 'android';
  return null;
}

export function classifyPluginTier(plugin: QuickPlugin): PluginTierInfo {
  if (plugin.typeName === LONGTIME_TYPE) {
    return { tier: 'longtime', osFamily: null, osRank: 99 };
  }
  const hintedById = normalizePlatformFamily(plugin.id) ?? normalizePlatformFamily(plugin.name);
  const systemPlatforms = plugin.functions
    .filter(fn => fn.scope === 'system')
    .flatMap(fn => fn.platforms.map(p => normalizePlatformFamily(p)))
    .filter((f): f is string => f !== null);
  const osFamily = systemPlatforms[0] ?? hintedById;
  const isExplicitOsPlugin =
    systemPlatforms.length > 0
    || /(winevtx|winsysteminfo|linuxinfo|linuxforensics|iosbackup|harmonyos|android|macos)/i.test(plugin.id);

  if (isExplicitOsPlugin && osFamily) {
    const rank = OS_FAMILIES.indexOf(osFamily as (typeof OS_FAMILIES)[number]);
    return { tier: 'os', osFamily, osRank: rank >= 0 ? rank : OS_FAMILIES.length };
  }
  return { tier: 'software', osFamily: null, osRank: 50 };
}

export function isPluginMatchingPlatform(plugin: QuickPlugin, platform: string | null | undefined): boolean {
  const normalized = normalizePlatformFamily(platform);
  if (!normalized) return false;
  const info = classifyPluginTier(plugin);
  return info.tier === 'os' && info.osFamily === normalized;
}

/** 模块级收藏列表(所有快速分析对话框共享,localStorage 持久化)。 */
export const favouriteNames = usePersistedRef<string[]>(FAVOURITE_KEY, []);

function namesPlugin(name: string, pluginId: string): boolean {
  return name === pluginId || name.startsWith(`${pluginId}/`);
}

export function isFavouritePlugin(pluginId: string, favourites: string[] = favouriteNames.value): boolean {
  return favourites.some((name) => namesPlugin(name, pluginId));
}

export function toggleFavouritePlugin(pluginId: string): void {
  favouriteNames.value = isFavouritePlugin(pluginId)
    ? favouriteNames.value.filter((name) => !namesPlugin(name, pluginId))
    : [...favouriteNames.value, pluginId];
}

/** Whether a system-scope function runs on systems of `family` (no platforms = every family). */
export function appliesToFamily(fn: SchedFunction, family: string): boolean {
  return fn.platforms.length === 0 || fn.platforms.some((platform) => platform.toLowerCase() === family.toLowerCase());
}

/** 列出可运行的功能:剔除内部功能与文件级功能;系统级功能只在选中系统适用时列出。 */
export function preprocessFunctions(
  functions: SchedFunction[],
  system: Pick<DetectedSystem, 'family'> | null,
): SchedFunction[] {
  return functions.filter((fn) => !fn.internal && (
    fn.scope === 'evidence' || (fn.scope === 'system' && system !== null && appliesToFamily(fn, system.family))
  ));
}

/** 按插件归并功能,插件与其功能都保持清单顺序;名称按界面语言取插件译文。 */
export function groupByPlugin(functions: SchedFunction[], text: PluginTextLookup = declaredPluginText): QuickPlugin[] {
  const plugins = new Map<string, QuickPlugin>();
  for (const fn of functions) {
    const named = { ...fn, displayName: text(fn.pluginId, pluginTextKey.analyzer(fn.analyzer), fn.displayName) };
    const plugin = plugins.get(fn.pluginId);
    if (plugin) {
      plugin.functions.push(named);
    } else {
      plugins.set(fn.pluginId, {
        id: fn.pluginId,
        name: text(fn.pluginId, pluginTextKey.plugin(), fn.pluginName),
        typeName: fn.typeName,
        functions: [named],
      });
    }
  }
  return [...plugins.values()];
}

function pluginMatches(plugin: QuickPlugin, keyword: string): boolean {
  return (
    plugin.name.toLowerCase().includes(keyword) ||
    plugin.id.toLowerCase().includes(keyword) ||
    plugin.functions.some(
      (fn) => fn.displayName.toLowerCase().includes(keyword) || fn.function.toLowerCase().includes(keyword),
    )
  );
}

/**
 * 按 收藏 → 系统取证（Windows/Linux/iOS/HarmonyOS 等平级展示，命中当前平台优先）
 * → 软件与专项分析 → longtime 组装分组。
 */
export function buildPluginGroups(
  plugins: QuickPlugin[],
  args: {
    favourites: string[];
    search: string;
    text?: PluginTextLookup;
    detectedPlatform?: string | null;
  },
): PluginGroup[] {
  const keyword = args.search.trim().toLowerCase();
  const matched = keyword ? plugins.filter((plugin) => pluginMatches(plugin, keyword)) : plugins;
  const favour = matched.filter((plugin) => isFavouritePlugin(plugin.id, args.favourites));

  const osPlugins: QuickPlugin[] = [];
  const softwarePlugins: QuickPlugin[] = [];
  const longtimePlugins: QuickPlugin[] = [];

  for (const plugin of matched) {
    const info = classifyPluginTier(plugin);
    if (info.tier === 'longtime') longtimePlugins.push(plugin);
    else if (info.tier === 'os') osPlugins.push(plugin);
    else softwarePlugins.push(plugin);
  }

  const detected = normalizePlatformFamily(args.detectedPlatform);
  osPlugins.sort((left, right) => {
    const lInfo = classifyPluginTier(left);
    const rInfo = classifyPluginTier(right);
    const lHit = detected && lInfo.osFamily === detected ? 0 : 1;
    const rHit = detected && rInfo.osFamily === detected ? 0 : 1;
    if (lHit !== rHit) return lHit - rHit;
    return lInfo.osRank - rInfo.osRank;
  });

  const groups: PluginGroup[] = [];
  if (favour.length > 0) {
    groups.push({ key: 'favour', get label() { return tr("收藏"); }, plugins: favour });
  }
  if (osPlugins.length > 0) {
    groups.push({ key: 'os', get label() { return tr("系统取证"); }, plugins: osPlugins });
  }
  if (softwarePlugins.length > 0) {
    groups.push({ key: 'software', get label() { return tr("软件与专项分析"); }, plugins: softwarePlugins });
  }
  if (longtimePlugins.length > 0) {
    groups.push({ key: LONGTIME_TYPE, get label() { return tr("耗时任务"); }, plugins: longtimePlugins });
  }
  return groups;
}

/** 记忆检材上次选择的系统。 */
export function useSystemMemory(imageId: ImageId) {
  return usePersistedRef<string | null>(`${SYSTEM_MEMORY_PREFIX}${imageId}`, null);
}

/** 勾选的插件 id,按勾选顺序提交。 */
export function useQuickAnalysisSelection() {
  const selected = ref<Set<string>>(new Set());
  const selectionList = computed(() => [...selected.value]);

  function isSelected(id: string): boolean {
    return selected.value.has(id);
  }

  function toggle(id: string): void {
    const next = new Set(selected.value);
    if (!next.delete(id)) next.add(id);
    selected.value = next;
  }

  function selectMany(ids: string[]): void {
    const next = new Set(selected.value);
    for (const id of ids) next.add(id);
    selected.value = next;
  }

  function deselectMany(ids: string[]): void {
    const next = new Set(selected.value);
    for (const id of ids) next.delete(id);
    selected.value = next;
  }

  function clear(): void {
    selected.value = new Set();
  }

  return { selectionList, isSelected, toggle, selectMany, deselectMany, clear };
}
