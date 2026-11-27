const appAssets = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>('../assets/{bcp-apps,brand-icons}/*.{png,jpg,svg}', {
      eager: true,
      import: 'default',
      query: '?url',
    }),
  ).map(([path, url]) => [path.slice(path.lastIndexOf('/') + 1), url]),
);

/** Genuine third-party / ecosystem software & service logos shown in result trees and run lists. */
const BRAND_APP_RULES: ReadonlyArray<readonly [file: string, patterns: readonly string[]]> = [
  ['whatsapp.jpg', ['whatsapp']],
  ['icloud.svg', ['icloud']],
  ['airdrop.svg', ['airdrop', '隔空投送']],
  ['telegram.png', ['telegram', '电报']],
  ['discord.png', ['discord']],
  ['twitter.png', ['twitter']],
  ['douyin.jpg', ['抖音', 'douyin', 'aweme', 'tiktok']],
  ['kuaishou.png', ['快手', 'kuaishou', 'gifmaker']],
  ['xiaohongshu.jpg', ['小红书', 'xiaohongshu', 'xhs_hos', 'xhs', 'redbook']],
  ['bilibili.png', ['哔哩哔哩', 'bilibili', 'bili']],
  ['baidumap.jpg', ['百度地图', 'baidumap', 'hmmap']],
  ['gaodemap.jpg', ['高德地图', '高德', 'gaode', 'amap', 'autonavi']],
  ['baidunetdisk.png', ['百度网盘', 'netdisk']],
  ['baidu.jpg', ['百度', 'baidu']],
  ['ctrip.jpg', ['携程', 'ctrip']],
  ['umetrip.jpg', ['航旅纵横', 'umetrip']],
  ['12306.png', ['12306', 'mobileticket']],
  ['alipay.jpg', ['支付宝', 'alipay']],
  ['jingdong.png', ['京东', 'jingdong', 'jd.hm']],
  ['idlefish.jpg', ['闲鱼', 'idlefish']],
  ['taobao.jpg', ['淘宝', 'taobao']],
  ['pinduoduo.jpg', ['拼多多', 'pinduoduo', 'xunmeng']],
  ['dazhongdianping.png', ['大众点评', 'dianping']],
  ['meituan.jpg', ['美团', 'meituan', 'sankuai']],
  ['dingding.jpg', ['钉钉', 'dingtalk', 'rimet']],
  ['zhihu.jpg', ['知乎', 'zhihu']],
  ['maimai.jpg', ['脉脉', 'maimai']],
  ['wps.jpg', ['wps', 'mobileoffice']],
  ['meetime.svg', ['畅连', 'meetime']],
  ['safari.jpg', ['safari']],
  ['quark.jpg', ['夸克', 'quark']],
  ['chrome.jpg', ['chrome']],
  ['edge.png', ['edge', 'emmx']],
  ['firefox.jpg', ['firefox', '火狐']],
  ['brave.png', ['brave']],
  ['360browser.jpg', ['360浏览器', '360安全浏览器']],
  ['mobaxterm.svg', ['mobaxterm']],
  ['navicat.svg', ['navicat']],
  ['github.svg', ['github', 'git_', 'git/', 'collector']],
];

/** Operating system & hardware-level plugin logos (used in Quick Analysis cards, not per-leaf result rows). */
const PLATFORM_RULES: ReadonlyArray<readonly [file: string, patterns: readonly string[]]> = [
  ['usb.svg', ['usb', 'u盘']],
  ['windows.svg', ['windows', 'winevtx', 'winsysteminfo', 'win_']],
  ['linux.svg', ['linux']],
  ['harmonyos.svg', ['harmonyos', 'harmony', 'hmos', '鸿蒙']],
  ['ios.svg', ['iosbackup', 'ios_', 'ios', 'apple', 'iphone', 'ipad', 'sysdiagnose', 'coreduet', 'powerlog']],
  ['android.svg', ['android']],
];

const WORD_BOUNDARY_PATTERNS = new Set(['usb', 'ios', 'edge']);

function assetUrl(file: string): string | null {
  return appAssets[file] ?? null;
}

function matchesPattern(lower: string, pattern: string): boolean {
  if (WORD_BOUNDARY_PATTERNS.has(pattern)) {
    return new RegExp(`(^|[^a-z0-9])${pattern}([^a-z0-9]|$)`).test(lower);
  }
  return lower.includes(pattern);
}

function matchRuleList(
  rules: ReadonlyArray<readonly [file: string, patterns: readonly string[]]>,
  text: string,
): string | null {
  const lower = text.trim().toLowerCase();
  if (!lower) return null;
  for (const [file, patterns] of rules) {
    if (patterns.some(pattern => matchesPattern(lower, pattern))) {
      return assetUrl(file);
    }
  }
  return null;
}

/** Resolves a colored logo strictly for genuine third-party / ecosystem software brands. */
export function resolveBrandLogo(text: string): string | null {
  return matchRuleList(BRAND_APP_RULES, text);
}

/** Resolves a colored brand or platform/OS logo URL from an app, plugin, or facet label. */
export function resolveAppLogo(text: string): string | null {
  return matchRuleList(BRAND_APP_RULES, text) ?? matchRuleList(PLATFORM_RULES, text);
}

/** Resolves a colored logo for a result node only when it represents a genuine brand app. */
export function resolveRecordLogo(
  node: { artifactType?: string; typeName?: string; analyzer?: string; pluginId?: string },
  label: string,
): string | null {
  return resolveBrandLogo(label) ?? resolveBrandLogo(node.artifactType ?? '');
}

/** Resolves the best colored logo for a plugin tile in Quick Analysis. */
export function resolvePluginLogo(pluginId: string, pluginName: string): string | null {
  return resolveAppLogo(pluginName) ?? resolveAppLogo(pluginId);
}
