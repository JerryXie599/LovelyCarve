import { beforeEach, describe, expect, it } from 'vitest';

import type { SchedFunction } from '../../../lib/ipc/tasks';
import {
  appliesToFamily,
  buildPluginGroups,
  classifyPluginTier,
  favouriteNames,
  groupByPlugin,
  isFavouritePlugin,
  preprocessFunctions,
  toggleFavouritePlugin,
  useQuickAnalysisSelection,
} from './useQuickAnalysis';

function fn(overrides: Partial<SchedFunction> = {}): SchedFunction {
  return {
    function: 'org.test/demo',
    pluginId: 'org.test',
    pluginName: 'Demo',
    pluginVersion: '1',
    analyzer: 'demo',
    displayName: '演示功能',
    typeName: 'baseinfo',
    scope: 'system',
    platforms: ['linux'],
    trigger: 'manually',
    internal: false,
    singleInstance: false,
    provides: [],
    requires: [],
    extensions: [],
    mimeTypes: [],
    inputs: [],
    options: [],
    ...overrides,
  };
}

describe('preprocessFunctions', () => {
  const functions = [
    fn({ function: 'internal/one', internal: true }),
    fn({ function: 'linux/only' }),
    fn({ function: 'any/system', platforms: [] }),
    fn({ function: 'linux/file', scope: 'files' }),
    fn({ function: 'whole/evidence', scope: 'evidence', platforms: [] }),
    fn({ function: 'win/only', platforms: ['Windows'] }),
  ];

  it('lists the selected system’s analyzers and every evidence analyzer, never internal or file ones', () => {
    expect(preprocessFunctions(functions, { family: 'linux' }).map((item) => item.function))
      .toEqual(['linux/only', 'any/system', 'whole/evidence']);
    expect(preprocessFunctions(functions, { family: 'windows' }).map((item) => item.function))
      .toEqual(['any/system', 'whole/evidence', 'win/only']);
  });

  it('keeps only evidence analyzers when no system was detected', () => {
    expect(preprocessFunctions(functions, null).map((item) => item.function)).toEqual(['whole/evidence']);
  });

  it('matches system families case-insensitively and treats no platforms as every family', () => {
    expect(appliesToFamily(fn({ platforms: ['Linux'] }), 'linux')).toBe(true);
    expect(appliesToFamily(fn({ platforms: [] }), 'macos')).toBe(true);
    expect(appliesToFamily(fn({ platforms: ['windows'] }), 'linux')).toBe(false);
  });
});

describe('plugin groups', () => {
  const plugin = (id: string, name: string, typeName: string, analyzers: Array<[string, string]>, extra: Partial<SchedFunction> = {}) =>
    analyzers.map(([analyzer, displayName]) =>
      fn({ function: `${id}/${analyzer}`, pluginId: id, pluginName: name, analyzer, displayName, typeName, ...extra }));
  const functions = [
    ...plugin('com.wxjzc.navicat', 'Navicat 连接分析', 'baseinfo', [['navicat', 'Navicat 连接']], { scope: 'evidence', platforms: [] }),
    ...plugin('org.lovelycarve.harmonyos', 'HarmonyOS NEXT 鸿蒙取证分析', 'apps', [['hm', '鸿蒙概览']], { scope: 'evidence', platforms: [] }),
    ...plugin('org.hash', '全盘哈希', 'longtime', [['hash', '哈希比对']], { scope: 'evidence', platforms: [] }),
    ...plugin('org.lovelycarve.linuxinfo', 'Linux 取证分析', 'baseinfo', [['linux', 'Linux 基础']], { platforms: ['linux'] }),
    ...plugin('org.lovelycarve.iosbackup', 'iOS备份与Sysdiagnose取证', 'jicloud', [['ios', 'iOS 备份']], { scope: 'evidence', platforms: [] }),
    ...plugin('org.lovelycarve.winevtx', 'Windows 系统与事件日志', 'baseinfo', [['win', 'Windows 日志']], { platforms: ['windows'] }),
  ];

  it('merges functions of one plugin into one entry in list order', () => {
    const merged = groupByPlugin([
      fn({ function: 'org.user/wps', pluginId: 'org.user', pluginName: 'Windows用户痕迹', analyzer: 'wps', displayName: '最近访问的WPS文件' }),
      fn({ function: 'org.user/desktop', pluginId: 'org.user', pluginName: 'Windows用户痕迹', analyzer: 'desktop', displayName: '用户桌面' }),
    ]);
    expect(merged).toHaveLength(1);
    expect(merged[0].functions.map(f => f.displayName)).toEqual(['最近访问的WPS文件', '用户桌面']);
  });

  it('groups favourites first, peer OS plugins second (bubbling detected platform), software third, longtime last', () => {
    const groups = buildPluginGroups(groupByPlugin(functions), {
      favourites: ['com.wxjzc.navicat'],
      search: '',
      detectedPlatform: 'harmony',
    });
    expect(groups.map((group) => [group.key, group.label])).toEqual([
      ['favour', '收藏'],
      ['os', '系统取证'],
      ['software', '软件与专项分析'],
      ['longtime', '耗时任务'],
    ]);
    expect(groups[0].plugins.map((item) => item.id)).toEqual(['com.wxjzc.navicat']);
    expect(groups[1].plugins.map((item) => item.id)).toEqual([
      'org.lovelycarve.harmonyos',
      'org.lovelycarve.winevtx',
      'org.lovelycarve.linuxinfo',
      'org.lovelycarve.iosbackup',
    ]);
    expect(groups[2].plugins.map((item) => item.id)).toEqual(['com.wxjzc.navicat']);
  });

  it('classifies OS vs software tiers accurately', () => {
    const plugins = groupByPlugin(functions);
    expect(classifyPluginTier(plugins.find(p => p.id === 'org.lovelycarve.harmonyos')!).tier).toBe('os');
    expect(classifyPluginTier(plugins.find(p => p.id === 'org.lovelycarve.iosbackup')!).tier).toBe('os');
    expect(classifyPluginTier(plugins.find(p => p.id === 'com.wxjzc.navicat')!).tier).toBe('software');
    expect(classifyPluginTier(plugins.find(p => p.id === 'org.hash')!).tier).toBe('longtime');
  });

  it('finds a plugin by its name, id or any of its functions', () => {
    const plugins = groupByPlugin(functions);
    const ids = (search: string) =>
      buildPluginGroups(plugins, { favourites: [], search }).flatMap((group) => group.plugins.map((item) => item.id));
    expect(ids('navicat')).toEqual(['com.wxjzc.navicat']);
    expect(ids('鸿蒙概览')).toEqual(['org.lovelycarve.harmonyos']);
    expect(ids('org.hash/hash')).toEqual(['org.hash']);
    expect(ids('不存在')).toEqual([]);
  });
});

describe('favourites', () => {
  beforeEach(() => {
    favouriteNames.value = [];
  });

  it('toggles a plugin and still honours entries saved per function', () => {
    expect(isFavouritePlugin('org.user')).toBe(false);
    toggleFavouritePlugin('org.user');
    expect(isFavouritePlugin('org.user')).toBe(true);
    expect(isFavouritePlugin('org.use')).toBe(false);
    toggleFavouritePlugin('org.user');
    expect(isFavouritePlugin('org.user')).toBe(false);

    favouriteNames.value = ['org.user/wps', 'org.mail'];
    expect(isFavouritePlugin('org.user')).toBe(true);
    toggleFavouritePlugin('org.user');
    expect(favouriteNames.value).toEqual(['org.mail']);
  });
});

describe('useQuickAnalysisSelection', () => {
  it('tracks whole plugins by id', () => {
    const selection = useQuickAnalysisSelection();
    selection.toggle('org.user');
    expect(selection.isSelected('org.user')).toBe(true);
    expect(selection.selectionList.value).toEqual(['org.user']);
    selection.toggle('org.user');
    expect(selection.isSelected('org.user')).toBe(false);
    selection.toggle('org.user');
    selection.clear();
    expect(selection.selectionList.value).toEqual([]);
  });

  it('selects and clears a category in the order plugins were ticked', () => {
    const selection = useQuickAnalysisSelection();
    selection.toggle('org.hash');
    selection.selectMany(['org.user', 'org.hash']);
    expect(selection.selectionList.value).toEqual(['org.hash', 'org.user']);
    selection.deselectMany(['org.user', 'org.hash']);
    expect(selection.selectionList.value).toEqual([]);
  });
});
