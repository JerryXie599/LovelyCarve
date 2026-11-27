import { describe, expect, it } from 'vitest';
import { getLinuxAuthFileHint, getLinuxAuthSchemaHint } from './linuxAuthHints';

describe('linuxAuthHints', () => {
  it('returns passwd cheat-sheet with UID and shell rules', () => {
    const hint = getLinuxAuthFileHint('/分区1/etc/passwd');
    expect(hint?.id).toBe('passwd');
    expect(hint?.formatLine).toContain('UID');
    expect(hint?.badges.some(b => b.token.includes('nologin'))).toBe(true);
  });

  it('returns shadow cheat-sheet covering yescrypt, bcrypt, and sha prefixes', () => {
    const hint = getLinuxAuthFileHint('/etc/shadow');
    expect(hint?.id).toBe('shadow');
    const tokens = hint?.badges.map(b => b.token).join(' ') ?? '';
    expect(tokens).toContain('$y$');
    expect(tokens).toContain('$2b$');
    expect(tokens).toContain('$6$');
    expect(tokens).toContain('$5$');
  });

  it('returns schema hint for linuxf.user and linux.user only', () => {
    expect(getLinuxAuthSchemaHint('linuxf.user')?.id).toBe('users_result');
    expect(getLinuxAuthSchemaHint('linux.user')?.id).toBe('users_result');
    expect(getLinuxAuthSchemaHint('wininfo.user')).toBeNull();
  });
});
