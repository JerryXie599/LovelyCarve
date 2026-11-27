import { describe, expect, it } from 'vitest';
import { formatSystemDirTooltip, getSystemDirHint } from './systemDirHints';

describe('systemDirHints', () => {
  it('matches Linux top-level and high-value subdirectories with or without partition prefix', () => {
    const etcDirect = getSystemDirHint('/etc');
    expect(etcDirect?.os).toBe('linux');
    expect(etcDirect?.shortLabel).toContain('配置');
    expect(etcDirect?.keyArtifacts).toContain('passwd');

    const etcPart = getSystemDirHint('/分区1/etc');
    expect(etcPart?.os).toBe('linux');
    expect(etcPart?.shortLabel).toBe(etcDirect?.shortLabel);

    const nm = getSystemDirHint('/分区3/etc/NetworkManager/system-connections');
    expect(nm?.os).toBe('linux');
    expect(nm?.detail).toContain('nmconnection');

    const rootDir = getSystemDirHint('part:3/root');
    expect(rootDir?.os).toBe('linux');
    expect(rootDir?.keyArtifacts).toContain('.bash_history');

    const varLog = getSystemDirHint('/分区1/var/log');
    expect(varLog?.os).toBe('linux');

    const ssh = getSystemDirHint('/分区1/home/kali/.ssh');
    expect(ssh?.os).toBe('linux');
    expect(ssh?.keyArtifacts).toContain('authorized_keys');

    const docker = getSystemDirHint('/分区1/var/lib/docker');
    expect(docker?.os).toBe('linux');

    const rootSsh = getSystemDirHint('/分区1/root/.ssh');
    expect(rootSsh?.os).toBe('linux');
    expect(rootSsh?.keyArtifacts).toContain('authorized_keys');

    const bluetooth = getSystemDirHint('/分区1/etc/bluetooth');
    expect(bluetooth?.os).toBe('linux');

    const nmLease = getSystemDirHint('/分区1/var/lib/NetworkManager');
    expect(nmLease?.os).toBe('linux');
  });

  it('matches Windows standard directories case-insensitively across volumes and users', () => {
    const config = getSystemDirHint('/分区2/WINDOWS/System32/config');
    expect(config?.os).toBe('windows');
    expect(config?.keyArtifacts).toContain('SAM');

    const recent = getSystemDirHint('C:\\Users\\Alice\\AppData\\Roaming\\Microsoft\\Windows\\Recent');
    expect(recent?.os).toBe('windows');
    expect(recent?.shortLabel).toContain('Recent');

    const recycle = getSystemDirHint('/分区2/$Recycle.Bin');
    expect(recycle?.os).toBe('windows');

    const wlan = getSystemDirHint('/分区2/ProgramData/Microsoft/Wlansvc/Profiles/Interfaces');
    expect(wlan?.os).toBe('windows');

    const wow64 = getSystemDirHint('/分区2/Windows/SysWOW64');
    expect(wow64?.os).toBe('windows');
  });

  it('prevents false positives on nested user folders and bare partition roots', () => {
    expect(getSystemDirHint('/分区1')).toBeNull();
    expect(getSystemDirHint('/分区1/home/kali/etc')).toBeNull();
    expect(getSystemDirHint('/分区1/tmp/Windows')).toBeNull();
    expect(getSystemDirHint('/分区1/Users/Public/fake/System32')).toBeNull();
    expect(getSystemDirHint('/Users/Alice/etc')).toBeNull();
  });

  it('formats crisp tooltips with key artifacts when available', () => {
    const hint = getSystemDirHint('/分区1/etc')!;
    const tip = formatSystemDirTooltip('etc', hint);
    expect(tip).toContain('etc —');
    expect(tip).toContain('常见重点文件:');
    expect(tip).toContain('shadow');
  });
});
