export interface LinuxAuthHintSection {
  id: 'passwd' | 'shadow' | 'users_result';
  title: string;
  formatLine: string;
  badges: Array<{ token: string; summary: string }>;
  notes: string[];
}

const identityTr = (s: string) => s;

function hashPrefixBadges(tr: (s: string) => string): Array<{ token: string; summary: string; translatableToken?: boolean }> {
  return [
    { token: '$y$', summary: tr('yescrypt (现代 Debian/Kali/Ubuntu 默认高强度 KDF)') },
    { token: '$gy$', summary: tr('gost-yescrypt (GOST R 34.11 + yescrypt)') },
    { token: '$2a$/$2b$/$2y$', summary: tr('bcrypt (Blowfish 变体，常见于 BSD/部分 Linux)') },
    { token: '$6$', summary: tr('SHA-512 crypt (常见 CentOS/RHEL 7-8 / 旧 Ubuntu)') },
    { token: '$5$', summary: tr('SHA-256 crypt') },
    { token: '$1$', summary: tr('MD5 crypt (弱哈希，极易爆破)') },
    { token: '$7$', summary: tr('scrypt') },
    { token: '$argon2id$', summary: tr('Argon2id/Argon2i/Argon2d') },
    { token: '! / !! / *', summary: tr('账户/密码已锁定，禁止密码认证（仍可配置 SSH 公钥登录）') },
    { token: tr('空字段'), summary: tr('空口令高危风险：无需密码即可本地/提权验证'), translatableToken: true },
  ];
}

/** Returns a concise forensic cheat-sheet when previewing `/etc/passwd` or `/etc/shadow`. */
export function getLinuxAuthFileHint(
  fullPathOrName: string,
  tr: (s: string) => string = identityTr,
): LinuxAuthHintSection | null {
  const norm = fullPathOrName.replace(/\\/g, '/').trim().toLowerCase();
  const base = norm.split('/').pop() ?? norm;

  const looksLikeEtc =
    norm.includes('/etc/') || norm.startsWith('etc/') || base === 'passwd' || base === 'shadow';
  if (!looksLikeEtc) return null;

  if (base === 'passwd' || base === 'passwd-' || base === 'passwd.bak') {
    return {
      id: 'passwd',
      title: tr('/etc/passwd 取证格式与权限速查'),
      formatLine: tr('格式（7列冒号分隔）：用户名 : 密码占位(x) : UID : GID : 注释/全名(GECOS) : 家目录 : 登录Shell'),
      badges: [
        { token: 'UID = 0', summary: tr('root 超级管理员权限（需警惕 UID=0 的伪装后门账号）') },
        { token: 'UID 1–999', summary: tr('系统服务与守护进程内置账号') },
        { token: 'UID ≥ 1000', summary: tr('普通可登录交互用户（nobody 通常为 65534）') },
        { token: '/bin/bash / /bin/zsh / /bin/sh', summary: tr('具备完整交互式登录 Shell 能力') },
        { token: '/usr/sbin/nologin / /bin/false', summary: tr('禁止交互式 Shell 登录（多为服务账号或已禁用）') },
      ],
      notes: [
        tr('第2列为 x 表示真实密码哈希存放在 /etc/shadow；若直接出现 $6$/$y$ 等哈希则为历史内联口令格式。'),
        tr('若发现非 root 账号出现 UID=0 或家目录/Shell 被篡改为 /bin/bash，常对应权限维持/后门线索。'),
      ],
    };
  }

  if (base === 'shadow' || base === 'shadow-' || base === 'gshadow') {
    return {
      id: 'shadow',
      title: tr('/etc/shadow 取证格式与哈希算法速查'),
      formatLine: tr('格式（9列冒号分隔）：用户名 : 加密哈希/锁定符 : 上次改密(自1970-01-01天数) : 最短间隔 : 最长有效 : 警告期 : 宽限停用 : 过期天数 : 保留'),
      badges: hashPrefixBadges(tr),
      notes: [
        tr('结构示例：$y$参数$盐值$哈希 或 $6$rounds=5000$盐值$哈希；若以 !$6$... 开头，表示账号密码已锁定但保留了原哈希（去掉 ! 后仍可审计/爆破历史密码）。'),
        tr('第3列天数乘以 86400 秒加 1970-01-01 即上次改密 UTC 日期；第8列若存在值则表示账号绝对过期时间。'),
      ],
    };
  }

  return null;
}

/** Returns a concise forensic cheat-sheet for parsed Linux user/shadow result tables (`linuxf.user`, `linux.user`). */
export function getLinuxAuthSchemaHint(
  recordType: string,
  tr: (s: string) => string = identityTr,
): LinuxAuthHintSection | null {
  if (recordType !== 'linuxf.user' && recordType !== 'linux.user') return null;
  return {
    id: 'users_result',
    title: tr('Linux 账户、Shell 登录能力与 Shadow 哈希符号说明'),
    formatLine: tr('交互权限 = 登录Shell (/bin/bash,/bin/zsh 有效 vs nologin/false 禁止) + Shadow状态 (!/* 锁定或仅密钥登录 vs $哈希 可密码登录)'),
    badges: [
      { token: 'UID = 0', summary: tr('root 超级权限（注意排查异常 UID=0 后门）') },
      { token: '/bin/bash · /bin/zsh · /bin/sh', summary: tr('允许交互式 Shell 登录') },
      { token: '/sbin/nologin · /bin/false', summary: tr('禁止交互式 Shell') },
      ...hashPrefixBadges(tr),
    ],
    notes: [
      tr('即使密码列标记为 ! 锁定，账户仍可能通过 ~/.ssh/authorized_keys 进行 SSH 公钥登录；需结合 Shell 类型与 SSH 策略交叉研判。'),
    ],
  };
}
