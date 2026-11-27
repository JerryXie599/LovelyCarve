import { describe, expect, it } from 'vitest';
import { resolveAppLogo, resolveBrandLogo, resolvePluginLogo, resolveRecordLogo } from './appIcons';

const svgOrNamed = (url: string | null, file: string) => {
  expect(url).toBeTruthy();
  expect(url!.startsWith('data:image/svg+xml') || url!.includes(file)).toBe(true);
};

describe('appIcons', () => {
  it('resolves downloaded official brand logos for major software apps', () => {
    expect(resolveBrandLogo('抖音(分身版)')).toContain('douyin.jpg');
    svgOrNamed(resolveBrandLogo('iCloud 取证分析'), 'icloud.svg');
    expect(resolveBrandLogo('WhatsApp 取证分析')).toContain('whatsapp.jpg');
    expect(resolveBrandLogo('WPS云文档')).toContain('wps.jpg');
    expect(resolveBrandLogo('百度地图')).toContain('baidumap.jpg');
    expect(resolveBrandLogo('夸克浏览器')).toContain('quark.jpg');
    svgOrNamed(resolveBrandLogo('AirDrop 本机标识'), 'airdrop.svg');
    svgOrNamed(resolveBrandLogo('MobaXterm 凭据分析'), 'mobaxterm.svg');
    svgOrNamed(resolveBrandLogo('Navicat 连接分析'), 'navicat.svg');
    svgOrNamed(resolveBrandLogo('Git/GitHub项目取证'), 'github.svg');
  });

  it('distinguishes similar ecosystem brand prefixes reliably', () => {
    expect(resolveBrandLogo('百度地图')).toContain('baidumap.jpg');
    expect(resolveBrandLogo('百度网盘')).toContain('baidunetdisk.png');
    expect(resolveBrandLogo('高德地图')).toContain('gaodemap.jpg');
  });

  it('keeps resolveRecordLogo null for generic OS or system tables so leaf trees stay uncluttered', () => {
    expect(resolveRecordLogo({ artifactType: 'wininfo.service', analyzer: 'win_baseinfo', pluginId: 'org.lovelycarve.winevtx' }, '系统服务')).toBeNull();
    expect(resolveRecordLogo({ artifactType: 'winevtx.security', analyzer: 'winevtx', pluginId: 'org.lovelycarve.winevtx' }, '核心安全事件')).toBeNull();
    expect(resolveRecordLogo({ artifactType: 'harmony.call', analyzer: 'harmony_comm', pluginId: 'org.lovelycarve.harmonyos' }, '通话记录')).toBeNull();
    expect(resolveRecordLogo({ artifactType: 'ios.photos', analyzer: 'ios_photos', pluginId: 'org.lovelycarve.iosbackup' }, '相册资产')).toBeNull();
    expect(resolveRecordLogo({ artifactType: 'harmony.note', analyzer: 'harmony_pim', pluginId: 'org.lovelycarve.harmonyos' }, '备忘录')).toBeNull();

    expect(resolveRecordLogo({ artifactType: 'harmony.im_message', pluginId: 'org.lovelycarve.harmonyos' }, '抖音消息')).toContain('douyin.jpg');
    expect(resolveRecordLogo({ artifactType: 'ios.whatsapp', pluginId: 'org.lovelycarve.iosbackup' }, 'WhatsApp 消息')).toContain('whatsapp.jpg');
  });

  it('still resolves OS and tool logos for Quick Analysis plugins via resolvePluginLogo / resolveAppLogo', () => {
    svgOrNamed(resolvePluginLogo('org.lovelycarve.harmonyos', 'HarmonyOS NEXT 鸿蒙取证'), 'harmonyos.svg');
    svgOrNamed(resolvePluginLogo('org.lovelycarve.iosbackup', 'iOS备份与Sysdiagnose取证'), 'ios.svg');
    expect(resolvePluginLogo('org.lovelycarve.linuxinfo', 'Linux 取证分析')).toContain('linux.svg');
    svgOrNamed(resolvePluginLogo('org.lovelycarve.winevtx', 'Windows 基础信息'), 'windows.svg');
    svgOrNamed(resolvePluginLogo('org.lovelycarve.usbimage', 'USB镜像分析'), 'usb.svg');
    svgOrNamed(resolveAppLogo('android'), 'android.svg');
  });
});
