export interface SystemDirHint {
  shortLabel: string;
  detail: string;
  os: 'linux' | 'windows';
  keyArtifacts?: string[];
}

interface DirHintDef {
  os: 'linux' | 'windows';
  shortLabel: string;
  detail: string;
  keyArtifacts?: string[];
}

const identityTr = (s: string) => s;

const KNOWN_OS_ROOT_DIRS = new Set([
  // Linux / POSIX FHS
  'etc', 'root', 'home', 'var', 'tmp', 'opt', 'srv', 'boot',
  'bin', 'sbin', 'usr', 'lib', 'lib64', 'dev', 'proc', 'sys',
  'run', 'mnt', 'media', 'lost+found',
  // Windows
  'windows', 'users', 'program files', 'program files (x86)',
  'programdata', '$recycle.bin', 'system volume information',
  'perflogs', 'recovery',
]);

function lookupExact(joined: string, tr: (s: string) => string): DirHintDef | null {
  switch (joined) {
    // ── Linux FHS 根目录 ──
    case 'etc':
      return {
        os: 'linux',
        shortLabel: tr('系统核心配置'),
        detail: tr('存放账户口令(passwd/shadow)、SSH/sudo策略、网络与Wi-Fi密钥、定时任务、服务与软件源等全局配置'),
        keyArtifacts: ['passwd', 'shadow', 'group', 'fstab', 'hosts', 'NetworkManager/system-connections'],
      };
    case 'root':
      return {
        os: 'linux',
        shortLabel: tr('root 用户主目录'),
        detail: tr('超级管理员独立家目录，常见命令历史(.bash_history/.zsh_history)、SSH密钥、维护脚本或攻击者残留工具'),
        keyArtifacts: ['.bash_history', '.zsh_history', '.ssh', '.viminfo'],
      };
    case 'root/.ssh':
      return {
        os: 'linux',
        shortLabel: tr('root SSH 密钥与信任'),
        detail: tr('含 root 账户公钥(authorized_keys)、私钥、连接过的主机(known_hosts)与 SSH 客户端配置'),
        keyArtifacts: ['authorized_keys', 'known_hosts', 'id_rsa', 'id_ed25519'],
      };
    case 'root/.config':
      return {
        os: 'linux',
        shortLabel: tr('root 应用配置'),
        detail: tr('超级管理员运行命令行工具、终端或远程软件留下的配置与状态文件'),
      };
    case 'home':
      return {
        os: 'linux',
        shortLabel: tr('普通用户主目录'),
        detail: tr('按用户名存放各交互用户个人文档、下载、浏览器配置、Shell命令历史、SSH密钥与桌面/回收站痕迹'),
        keyArtifacts: ['.bash_history', '.zsh_history', '.ssh', '.mozilla', '.config', '.local/share/Trash'],
      };
    case 'var':
      return {
        os: 'linux',
        shortLabel: tr('动态数据与日志'),
        detail: tr('存放系统/认证/Web日志、计划任务队列、软件包数据库、数据库数据目录及容器运行卷'),
        keyArtifacts: ['log', 'spool/cron', 'lib', 'www'],
      };
    case 'tmp':
      return {
        os: 'linux',
        shortLabel: tr('系统临时目录'),
        detail: tr('全局可写临时目录，常见脚本缓存、编译产物、会话锁文件及入侵临时载荷'),
      };
    case 'opt':
      return {
        os: 'linux',
        shortLabel: tr('第三方/附加应用'),
        detail: tr('非发行版包管理器安装的独立软件与服务目录(如运维面板、自研服务、渗透工具)'),
      };
    case 'srv':
      return {
        os: 'linux',
        shortLabel: tr('服务数据目录'),
        detail: tr('存放 FTP、Samba、Web、Git 或 NAS 共享等对外提供服务的数据'),
      };
    case 'boot':
      return {
        os: 'linux',
        shortLabel: tr('系统引导与内核'),
        detail: tr('存放内核镜像(vmlinuz)、initramfs 与 GRUB 启动引导配置，可核查启动参数与内核版本'),
        keyArtifacts: ['grub', 'grub2', 'config-*'],
      };
    case 'usr':
      return {
        os: 'linux',
        shortLabel: tr('系统程序与资源'),
        detail: tr('存放绝大多数用户态可执行程序、共享库、Systemd 系统内置服务单元与文档'),
        keyArtifacts: ['bin', 'sbin', 'lib', 'local'],
      };
    case 'usr/local':
      return {
        os: 'linux',
        shortLabel: tr('本地手动安装软件'),
        detail: tr('管理员手动编译安装的可执行文件(/usr/local/bin)与脚本，常见自定义工具与后门'),
        keyArtifacts: ['bin', 'sbin', 'etc'],
      };
    case 'usr/lib/systemd/system':
      return {
        os: 'linux',
        shortLabel: tr('内置 Systemd 单元'),
        detail: tr('软件包默认安装的 .service/.timer 服务定义文件'),
      };
    case 'lib/systemd/system':
      return {
        os: 'linux',
        shortLabel: tr('内置 Systemd 单元'),
        detail: tr('发行版基础软件包安装的 .service/.timer 单元文件'),
      };
    case 'bin':
      return {
        os: 'linux',
        shortLabel: tr('基础命令二进制'),
        detail: tr('基础系统命令(ls/sh/bash 等)所在目录，现代发行版多软链接至 /usr/bin'),
      };
    case 'sbin':
      return {
        os: 'linux',
        shortLabel: tr('系统管理二进制'),
        detail: tr('系统级管理与修复命令所在目录，现代发行版多软链接至 /usr/sbin'),
      };
    case 'lib':
      return {
        os: 'linux',
        shortLabel: tr('核心共享库与模块'),
        detail: tr('基础动态链接库、udev 规则与内核模块'),
      };
    case 'lib64':
      return {
        os: 'linux',
        shortLabel: tr('64位动态共享库'),
        detail: tr('x86_64/arm64 架构基础共享库与动态链接器'),
      };
    case 'dev':
      return {
        os: 'linux',
        shortLabel: tr('设备节点挂载点'),
        detail: tr('运行时由 devtmpfs 挂载的设备文件目录；离线磁盘镜像中通常仅存少量静态节点'),
      };
    case 'proc':
      return {
        os: 'linux',
        shortLabel: tr('内核进程虚拟目录'),
        detail: tr('运行时 procfs 虚拟文件系统挂载点；离线镜像中一般为空目录'),
      };
    case 'sys':
      return {
        os: 'linux',
        shortLabel: tr('内核设备树虚拟目录'),
        detail: tr('运行时 sysfs 虚拟文件系统挂载点；离线镜像中一般为空目录'),
      };
    case 'run':
      return {
        os: 'linux',
        shortLabel: tr('运行时临时状态'),
        detail: tr('tmpfs 运行时 PID、Socket 与临时网络配置挂载点；离线镜像中一般为空或仅含占位'),
      };
    case 'mnt':
      return {
        os: 'linux',
        shortLabel: tr('临时挂载点'),
        detail: tr('管理员手动临时挂载外接磁盘、ISO 或网络共享的目录'),
      };
    case 'media':
      return {
        os: 'linux',
        shortLabel: tr('可移动介质挂载点'),
        detail: tr('桌面环境自动挂载 U 盘、移动硬盘或光驱的默认目录'),
      };
    case 'lost+found':
      return {
        os: 'linux',
        shortLabel: tr('文件系统修复碎片区'),
        detail: tr('fsck 修复 ext 文件系统时存放孤立 inode 碎片文件的目录'),
      };

    // ── Linux 高价值 /etc 与 /var 子目录 ──
    case 'etc/networkmanager':
      return {
        os: 'linux',
        shortLabel: tr('NetworkManager 配置'),
        detail: tr('管理有线、Wi-Fi 与 VPN 连接的全局配置目录'),
        keyArtifacts: ['system-connections'],
      };
    case 'etc/networkmanager/system-connections':
      return {
        os: 'linux',
        shortLabel: tr('Wi-Fi/网络凭据配置'),
        detail: tr('存放 *.nmconnection 等连接配置，常含明文 Wi-Fi SSID 与 PSK 密码([wifi-security] psk=)'),
        keyArtifacts: ['*.nmconnection'],
      };
    case 'etc/netplan':
      return {
        os: 'linux',
        shortLabel: tr('Netplan 网络配置'),
        detail: tr('Ubuntu 等现代发行版 YAML 网络与 Wi-Fi 配置文件'),
        keyArtifacts: ['*.yaml'],
      };
    case 'etc/wpa_supplicant':
      return {
        os: 'linux',
        shortLabel: tr('WPA 无线配置'),
        detail: tr('wpa_supplicant.conf 无线配置文件，常含明文 Wi-Fi SSID 与 PSK'),
        keyArtifacts: ['wpa_supplicant.conf'],
      };
    case 'etc/bluetooth':
      return {
        os: 'linux',
        shortLabel: tr('蓝牙服务配置'),
        detail: tr('Linux 蓝牙守护进程(BlueZ)主配置与配对策略目录'),
        keyArtifacts: ['main.conf'],
      };
    case 'etc/ca-certificates':
      return {
        os: 'linux',
        shortLabel: tr('CA 根证书配置'),
        detail: tr('系统受信任的 CA 根证书列表及自定义信任证书'),
      };
    case 'etc/profile.d':
      return {
        os: 'linux',
        shortLabel: tr('全局 Shell 启动脚本'),
        detail: tr('所有用户登录交互式 Shell 时自动加载的环境变量与启动脚本片段，常见持久化点'),
      };
    case 'etc/sysconfig':
      return {
        os: 'linux',
        shortLabel: tr('RHEL 系统服务配置'),
        detail: tr('CentOS/RHEL/Fedora 系网络、防火墙及系统服务启动参数配置目录'),
      };
    case 'etc/systemd':
      return {
        os: 'linux',
        shortLabel: tr('Systemd 全局配置'),
        detail: tr('Systemd 服务单元、networkd 网络与 journald 日志策略配置'),
        keyArtifacts: ['system', 'journald.conf', 'network'],
      };
    case 'etc/systemd/system':
      return {
        os: 'linux',
        shortLabel: tr('自定义/自启服务'),
        detail: tr('管理员自定义或启用的开机自启服务(.service)与定时器(.timer)，持久化排查重点'),
        keyArtifacts: ['multi-user.target.wants'],
      };
    case 'etc/systemd/network':
      return {
        os: 'linux',
        shortLabel: tr('systemd-networkd 配置'),
        detail: tr('*.network 静态 IP、DHCP 与 DNS 配置文件'),
      };
    case 'etc/ssh':
      return {
        os: 'linux',
        shortLabel: tr('SSH 服务与主机密钥'),
        detail: tr('sshd_config 安全策略(PermitRootLogin/Port/PasswordAuthentication)及服务器主机公私钥'),
        keyArtifacts: ['sshd_config', 'ssh_config', 'sshd_config.d'],
      };
    case 'etc/sudoers.d':
      return {
        os: 'linux',
        shortLabel: tr('Sudo 提权规则片段'),
        detail: tr('扩展的 NOPASSWD 免密或特定用户提权策略配置'),
      };
    case 'etc/pam.d':
      return {
        os: 'linux',
        shortLabel: tr('PAM 认证模块配置'),
        detail: tr('系统登录、sudo 与 sshd 的认证流程配置，可用于排查恶意 PAM 后门模块'),
      };
    case 'etc/audit':
      return {
        os: 'linux',
        shortLabel: tr('Auditd 审计规则'),
        detail: tr('audit.rules 审计规则，决定内核监控哪些敏感文件或系统调用'),
        keyArtifacts: ['rules.d', 'auditd.conf'],
      };
    case 'etc/cron.d':
      return {
        os: 'linux',
        shortLabel: tr('系统计划任务片段'),
        detail: tr('独立命名的定时执行任务配置，常用于排查持久化任务'),
      };
    case 'etc/cron.daily':
      return {
        os: 'linux',
        shortLabel: tr('每日定时脚本'),
        detail: tr('每天自动执行的系统维护或异常持久化脚本'),
      };
    case 'etc/cron.hourly':
      return {
        os: 'linux',
        shortLabel: tr('每小时定时脚本'),
        detail: tr('每小时自动执行的计划脚本'),
      };
    case 'etc/cron.weekly':
      return {
        os: 'linux',
        shortLabel: tr('每周定时脚本'),
        detail: tr('每周自动执行的计划脚本'),
      };
    case 'etc/cron.monthly':
      return {
        os: 'linux',
        shortLabel: tr('每月定时脚本'),
        detail: tr('每月自动执行的计划脚本'),
      };
    case 'etc/init.d':
      return {
        os: 'linux',
        shortLabel: tr('SysV Init 启动脚本'),
        detail: tr('传统开机自启服务脚本，常用于排查老旧发行版或兼容自启项'),
      };
    case 'etc/nginx':
      return {
        os: 'linux',
        shortLabel: tr('Nginx Web 配置'),
        detail: tr('nginx.conf 与 sites-enabled 虚拟主机、反代目标及 SSL 证书路径配置'),
        keyArtifacts: ['nginx.conf', 'sites-enabled', 'conf.d'],
      };
    case 'etc/apache2':
      return {
        os: 'linux',
        shortLabel: tr('Apache2 服务配置'),
        detail: tr('Debian/Ubuntu 系 Apache 虚拟主机、模块与日志路径配置'),
        keyArtifacts: ['apache2.conf', 'sites-enabled'],
      };
    case 'etc/httpd':
      return {
        os: 'linux',
        shortLabel: tr('Apache/HTTPD 配置'),
        detail: tr('RHEL/CentOS 系 httpd 主配置与虚拟主机目录'),
        keyArtifacts: ['conf/httpd.conf', 'conf.d'],
      };
    case 'etc/samba':
      return {
        os: 'linux',
        shortLabel: tr('Samba 共享配置'),
        detail: tr('smb.conf 共享目录、权限与匿名访问策略配置'),
        keyArtifacts: ['smb.conf'],
      };
    case 'etc/docker':
      return {
        os: 'linux',
        shortLabel: tr('Docker 守护配置'),
        detail: tr('daemon.json 镜像源、日志驱动、监听端口与存储驱动配置'),
        keyArtifacts: ['daemon.json'],
      };
    case 'etc/sysconfig/network-scripts':
      return {
        os: 'linux',
        shortLabel: tr('RHEL 系网卡配置'),
        detail: tr('CentOS/RHEL/Rocky ifcfg-* 网卡 IP、网关与 DNS 配置'),
        keyArtifacts: ['ifcfg-*'],
      };
    case 'etc/network':
      return {
        os: 'linux',
        shortLabel: tr('Debian ifupdown 配置'),
        detail: tr('interfaces 传统有线/无线网卡配置目录'),
        keyArtifacts: ['interfaces', 'interfaces.d'],
      };
    case 'etc/apt':
      return {
        os: 'linux',
        shortLabel: tr('APT 软件源配置'),
        detail: tr('sources.list 软件源地址与 apt 代理配置'),
        keyArtifacts: ['sources.list', 'sources.list.d'],
      };
    case 'etc/yum.repos.d':
      return {
        os: 'linux',
        shortLabel: tr('YUM/DNF 软件源'),
        detail: tr('RHEL/CentOS/Fedora .repo 软件仓库配置'),
      };
    case 'etc/ssl':
      return {
        os: 'linux',
        shortLabel: tr('系统 SSL/TLS 证书'),
        detail: tr('根证书、自签名证书及服务端私钥存储位置'),
        keyArtifacts: ['certs', 'private'],
      };
    case 'var/log':
      return {
        os: 'linux',
        shortLabel: tr('系统与安全日志'),
        detail: tr('取证核心目录：含登录记录(wtmp/btmp/lastlog)、认证日志(auth.log/secure)、内核/审计及Web日志'),
        keyArtifacts: ['auth.log', 'secure', 'syslog', 'messages', 'wtmp', 'btmp', 'journal', 'audit'],
      };
    case 'var/log/journal':
      return {
        os: 'linux',
        shortLabel: tr('Systemd 二进制日志'),
        detail: tr('systemd-journald 持久化日志目录(.journal)，记录服务启停、SSH登录、USB插拔与内核事件'),
        keyArtifacts: ['system.journal', 'user-*.journal'],
      };
    case 'var/log/audit':
      return {
        os: 'linux',
        shortLabel: tr('Linux 审计日志'),
        detail: tr('auditd 记录的系统调用、命令执行(EXECVE)、提权与敏感文件访问审计日志'),
        keyArtifacts: ['audit.log'],
      };
    case 'var/log/nginx':
      return {
        os: 'linux',
        shortLabel: tr('Nginx Web 日志'),
        detail: tr('Nginx 访问日志(access.log)与错误日志(error.log)，用于溯源Web攻击与异常IP'),
        keyArtifacts: ['access.log', 'error.log'],
      };
    case 'var/log/apache2':
      return {
        os: 'linux',
        shortLabel: tr('Apache2 Web 日志'),
        detail: tr('Debian/Ubuntu 系 Apache2 访问与错误日志'),
        keyArtifacts: ['access.log', 'error.log'],
      };
    case 'var/log/httpd':
      return {
        os: 'linux',
        shortLabel: tr('Apache/HTTPD 日志'),
        detail: tr('RHEL/CentOS 系 httpd 访问与错误日志'),
        keyArtifacts: ['access_log', 'error_log'],
      };
    case 'var/spool/cron':
      return {
        os: 'linux',
        shortLabel: tr('用户计划任务'),
        detail: tr('各用户通过 crontab -e 创建的定时任务文件，常用于排查持久化后门'),
        keyArtifacts: ['crontabs'],
      };
    case 'var/spool/cron/crontabs':
      return {
        os: 'linux',
        shortLabel: tr('用户 Crontab 列表'),
        detail: tr('Debian/Ubuntu/Kali 下按用户名存储的定时任务定义'),
      };
    case 'var/lib':
      return {
        os: 'linux',
        shortLabel: tr('服务与包状态数据'),
        detail: tr('存放数据库、Docker容器、网络DHCP租约、iwd Wi-Fi及软件包安装状态'),
        keyArtifacts: ['docker', 'postgresql', 'mysql', 'dpkg', 'rpm'],
      };
    case 'var/lib/networkmanager':
      return {
        os: 'linux',
        shortLabel: tr('NetworkManager 状态/租约'),
        detail: tr('存放 DHCP 历史租约(*.lease)及网络状态时间戳数据库'),
        keyArtifacts: ['*.lease', 'timestamps'],
      };
    case 'var/lib/docker':
      return {
        os: 'linux',
        shortLabel: tr('Docker 容器数据'),
        detail: tr('Docker 镜像层(overlay2)、容器配置(config.v2.json)、容器标准输出日志与持久化卷'),
        keyArtifacts: ['containers', 'volumes', 'image'],
      };
    case 'var/lib/iwd':
      return {
        os: 'linux',
        shortLabel: tr('iwd 无线配置'),
        detail: tr('Intel iwd 无线守护进程保存的 Wi-Fi 配置文件(*.psk)，内含明文预共享密钥'),
        keyArtifacts: ['*.psk'],
      };
    case 'var/lib/dpkg':
      return {
        os: 'linux',
        shortLabel: tr('APT/DPKG 软件包库'),
        detail: tr('Debian/Ubuntu/Kali 已安装软件包清单(status)与包文件列表'),
        keyArtifacts: ['status', 'info'],
      };
    case 'var/lib/rpm':
      return {
        os: 'linux',
        shortLabel: tr('RPM 软件包库'),
        detail: tr('RHEL/CentOS/Fedora 已安装软件包的 RPM 数据库'),
        keyArtifacts: ['rpmdb.sqlite', 'Packages'],
      };
    case 'var/lib/postgresql':
      return {
        os: 'linux',
        shortLabel: tr('PostgreSQL 数据目录'),
        detail: tr('PostgreSQL 数据库集群与表空间存储目录'),
      };
    case 'var/lib/mysql':
      return {
        os: 'linux',
        shortLabel: tr('MySQL/MariaDB 数据'),
        detail: tr('MySQL/MariaDB 数据库库文件与二进制日志(binlog)'),
      };
    case 'var/www':
      return {
        os: 'linux',
        shortLabel: tr('Web 站点根目录'),
        detail: tr('Apache/Nginx 默认网站源码目录，常见 WebShell、业务源码与配置文件'),
        keyArtifacts: ['html'],
      };
    case 'var/tmp':
      return {
        os: 'linux',
        shortLabel: tr('持久临时目录'),
        detail: tr('重启后仍保留的临时文件目录，常被攻击者用于暂存恶意载荷或脚本'),
      };

    // ── Windows 标准系统目录 ──
    case 'windows':
      return {
        os: 'windows',
        shortLabel: tr('Windows 系统核心'),
        detail: tr('包含注册表 Hive、系统事件日志(EVTX)、预读取执行痕迹、计划任务与系统组件'),
        keyArtifacts: ['System32', 'Prefetch', 'Tasks', 'inf'],
      };
    case 'users':
      return {
        os: 'windows',
        shortLabel: tr('所有用户配置目录'),
        detail: tr('按账户保存桌面、下载、文档、NTUSER.DAT 用户注册表与 AppData 应用数据'),
        keyArtifacts: ['NTUSER.DAT', 'Desktop', 'Downloads', 'AppData'],
      };
    case 'users/public':
      return {
        os: 'windows',
        shortLabel: tr('公共用户共享目录'),
        detail: tr('所有本地账户共享访问的公共桌面与文档目录，常见横向移动或公共投递文件'),
      };
    case 'users/default':
      return {
        os: 'windows',
        shortLabel: tr('默认用户配置模板'),
        detail: tr('新建 Windows 用户时复制的默认配置与默认注册表模板'),
      };
    case 'program files':
      return {
        os: 'windows',
        shortLabel: tr('64位软件安装目录'),
        detail: tr('默认安装的 64 位应用程序、浏览器、远程工具与安全软件主程序'),
      };
    case 'program files (x86)':
      return {
        os: 'windows',
        shortLabel: tr('32位软件安装目录'),
        detail: tr('32 位兼容应用程序与老旧客户端安装目录'),
      };
    case 'programdata':
      return {
        os: 'windows',
        shortLabel: tr('全局共享应用数据'),
        detail: tr('隐藏的系统级程序数据目录，含 Defender 隔离/日志、Wi-Fi 配置文件、软件全局配置'),
        keyArtifacts: ['Microsoft/Wlansvc', 'Microsoft/Windows Defender'],
      };
    case '$recycle.bin':
      return {
        os: 'windows',
        shortLabel: tr('NTFS 回收站'),
        detail: tr('按用户 SID 分目录存放已删除文件的 $I*(原路径/删除时间) 与 $R*(文件内容)'),
        keyArtifacts: ['$I*', '$R*'],
      };
    case 'system volume information':
      return {
        os: 'windows',
        shortLabel: tr('卷影副本与还原点'),
        detail: tr('存放 VSS 卷影快照、恢复点与 NTFS 索引数据，常留存历史文件版本'),
      };
    case 'perflogs':
      return {
        os: 'windows',
        shortLabel: tr('系统性能日志'),
        detail: tr('Windows 默认系统性能监控日志目录'),
      };
    case 'recovery':
      return {
        os: 'windows',
        shortLabel: tr('Windows 恢复环境'),
        detail: tr('WinRE 恢复镜像与系统引导故障修复相关文件'),
      };
    case 'windows/system32':
      return {
        os: 'windows',
        shortLabel: tr('系统核心与取证要地'),
        detail: tr('存放系统命令、驱动、注册表 Hive(config)、事件日志(winevt)与网络/任务配置'),
        keyArtifacts: ['config', 'winevt', 'Tasks', 'drivers', 'sru'],
      };
    case 'windows/syswow64':
      return {
        os: 'windows',
        shortLabel: tr('32位系统兼容目录'),
        detail: tr('64 位 Windows 上运行 32 位系统组件与兼容程序的系统目录'),
      };
    case 'windows/system32/config':
      return {
        os: 'windows',
        shortLabel: tr('系统注册表 Hive'),
        detail: tr('核心取证目录：含 SAM(本地账户)、SYSTEM(硬件/盘符/服务/USB)、SOFTWARE(软件/自启)、SECURITY'),
        keyArtifacts: ['SAM', 'SYSTEM', 'SOFTWARE', 'SECURITY', 'DEFAULT'],
      };
    case 'windows/system32/winevt':
      return {
        os: 'windows',
        shortLabel: tr('事件日志根目录'),
        detail: tr('Windows 事件查看器日志存储容器'),
        keyArtifacts: ['Logs'],
      };
    case 'windows/system32/winevt/logs':
      return {
        os: 'windows',
        shortLabel: tr('Windows 事件日志(EVTX)'),
        detail: tr('记录登录登出(Security)、系统启停(System)、RDP远程桌面与 PowerShell 执行等关键日志'),
        keyArtifacts: ['Security.evtx', 'System.evtx', 'Application.evtx'],
      };
    case 'windows/system32/tasks':
      return {
        os: 'windows',
        shortLabel: tr('系统计划任务'),
        detail: tr('系统与用户创建的持久化计划任务 XML 配置文件'),
      };
    case 'windows/system32/sru':
      return {
        os: 'windows',
        shortLabel: tr('SRUM 资源使用统计'),
        detail: tr('SRUDB.dat (ESE数据库)，记录近30-60天程序运行、网络流量与账户活动'),
        keyArtifacts: ['SRUDB.dat'],
      };
    case 'windows/system32/drivers/etc':
      return {
        os: 'windows',
        shortLabel: tr('Windows 网络映射'),
        detail: tr('hosts 域名解析映射、services 与 networks 静态网络配置'),
        keyArtifacts: ['hosts'],
      };
    case 'windows/logs':
      return {
        os: 'windows',
        shortLabel: tr('Windows 维护日志'),
        detail: tr('系统更新(CBS)、安装部署(DISM)与系统维护诊断日志目录'),
        keyArtifacts: ['CBS/CBS.log', 'DISM/dism.log'],
      };
    case 'windows/prefetch':
      return {
        os: 'windows',
        shortLabel: tr('Prefetch 预读取痕迹'),
        detail: tr('*.pf 文件记录可执行程序首次/最近运行时间(最多8次)、运行次数与加载路径'),
        keyArtifacts: ['*.pf'],
      };
    case 'windows/appcompat/programs':
      return {
        os: 'windows',
        shortLabel: tr('Amcache 执行缓存'),
        detail: tr('Amcache.hve 注册表存储已运行/安装程序路径、SHA1 哈希与编译时间'),
        keyArtifacts: ['Amcache.hve'],
      };
    case 'windows/temp':
      return {
        os: 'windows',
        shortLabel: tr('系统级临时目录'),
        detail: tr('SYSTEM/高权限服务写入的临时文件，常见安装包残留与恶意载荷'),
      };
    case 'windows/inf':
      return {
        os: 'windows',
        shortLabel: tr('设备驱动安装日志'),
        detail: tr('setupapi.dev.log 记录 U 盘、移动存储与外设首次插入时间及序列号'),
        keyArtifacts: ['setupapi.dev.log'],
      };
    case 'windows/minidump':
      return {
        os: 'windows',
        shortLabel: tr('蓝屏内存转储'),
        detail: tr('系统崩溃产生的内核小转储文件(.dmp)'),
      };
    case 'programdata/microsoft/wlansvc':
      return {
        os: 'windows',
        shortLabel: tr('Windows Wi-Fi 服务'),
        detail: tr('WlanSvc 服务存放无线网络配置文件的根目录'),
        keyArtifacts: ['Profiles/Interfaces'],
      };
    case 'programdata/microsoft/wlansvc/profiles/interfaces':
      return {
        os: 'windows',
        shortLabel: tr('Wi-Fi 配置文件(XML)'),
        detail: tr('按网卡 GUID 存放已连接 Wi-Fi 的 SSID、加密方式与密钥 XML'),
        keyArtifacts: ['*.xml'],
      };
    case 'programdata/microsoft/windows defender':
      return {
        os: 'windows',
        shortLabel: tr('Windows Defender 痕迹'),
        detail: tr('杀毒扫描记录、隔离区与威胁检测历史'),
        keyArtifacts: ['Scans', 'Quarantine'],
      };
    default:
      return null;
  }
}

function matchWildcard(segments: readonly string[], tr: (s: string) => string): DirHintDef | null {
  if (segments.length < 2) return null;
  const root = segments[0];
  const user = segments[1];
  if (!user || user === '.' || user === '..') return null;

  if (root === 'home') {
    const rest = segments.slice(2).join('/');
    switch (rest) {
      case '':
        return {
          os: 'linux',
          shortLabel: tr('用户个人主目录'),
          detail: tr('包含该用户的命令历史、SSH密钥、浏览器历史与桌面/文档/回收站文件'),
          keyArtifacts: ['.bash_history', '.zsh_history', '.ssh', 'Desktop', 'Downloads'],
        };
      case '.ssh':
        return {
          os: 'linux',
          shortLabel: tr('SSH 密钥与信任主机'),
          detail: tr('含登录公钥(authorized_keys)、私钥(id_rsa/id_ed25519)、known_hosts 连接记录与 config'),
          keyArtifacts: ['authorized_keys', 'known_hosts', 'id_rsa', 'id_ed25519', 'config'],
        };
      case '.config':
        return {
          os: 'linux',
          shortLabel: tr('用户应用配置目录'),
          detail: tr('浏览器、远程工具(FileZilla/Remmina)、桌面环境及终端软件配置文件'),
        };
      case '.local/share/trash':
        return {
          os: 'linux',
          shortLabel: tr('Linux 用户回收站'),
          detail: tr('桌面删除的文件本体(files/)与原始路径/删除时间元数据(.trashinfo)'),
          keyArtifacts: ['info', 'files'],
        };
      default:
        return null;
    }
  }

  if (root === 'users') {
    const rest = segments.slice(2).join('/');
    switch (rest) {
      case '':
        return {
          os: 'windows',
          shortLabel: tr('Windows 用户个人目录'),
          detail: tr('包含桌面、下载、文档、AppData 及该用户的 NTUSER.DAT 注册表'),
          keyArtifacts: ['NTUSER.DAT', 'Desktop', 'Downloads', 'Documents', 'AppData'],
        };
      case 'desktop':
        return {
          os: 'windows',
          shortLabel: tr('用户桌面'),
          detail: tr('用户桌面文件、快捷方式(.lnk)与常用工具直观入口'),
        };
      case 'downloads':
        return {
          os: 'windows',
          shortLabel: tr('用户下载目录'),
          detail: tr('浏览器及聊天工具默认下载的文件、安装包与文档'),
        };
      case 'documents':
        return {
          os: 'windows',
          shortLabel: tr('用户文档目录'),
          detail: tr('个人办公文档、邮件存档与部分软件本地工程'),
        };
      case 'appdata':
        return {
          os: 'windows',
          shortLabel: tr('用户应用数据根目录'),
          detail: tr('存放软件配置、浏览器缓存/历史/密码、聊天记录与用户级程序'),
          keyArtifacts: ['Local', 'Roaming', 'LocalLow'],
        };
      case 'appdata/local':
        return {
          os: 'windows',
          shortLabel: tr('本地应用数据 (Local)'),
          detail: tr('不随域漫游的本机数据：含 Chrome/Edge 用户数据、聊天软件缓存、UsrClass.dat 与 Temp'),
          keyArtifacts: ['Google/Chrome', 'Microsoft/Edge', 'Microsoft/Windows/UsrClass.dat', 'Temp'],
        };
      case 'appdata/roaming':
        return {
          os: 'windows',
          shortLabel: tr('漫游应用数据 (Roaming)'),
          detail: tr('应用配置、Recent 最近访问快捷方式、MobaXterm/SecureCRT/Xshell 配置与浏览器数据'),
          keyArtifacts: ['Microsoft/Windows/Recent'],
        };
      case 'appdata/locallow':
        return {
          os: 'windows',
          shortLabel: tr('低完整性应用数据'),
          detail: tr('浏览器沙盒与部分插件产生的低权限缓存数据'),
        };
      case 'appdata/local/temp':
        return {
          os: 'windows',
          shortLabel: tr('用户临时目录'),
          detail: tr('用户态解压残留、Office 临时副本与下载/攻击脚本暂存位置'),
        };
      case 'appdata/roaming/microsoft/windows/recent':
        return {
          os: 'windows',
          shortLabel: tr('最近访问痕迹 (Recent)'),
          detail: tr('包含 LNK 快捷方式与 AutomaticDestinations JumpList，还原用户打开过的文件与时间'),
          keyArtifacts: ['*.lnk', 'AutomaticDestinations'],
        };
      case 'appdata/roaming/microsoft/windows/start menu/programs/startup':
        return {
          os: 'windows',
          shortLabel: tr('用户开机自启目录'),
          detail: tr('用户登录后自动运行的快捷方式或脚本，持久化排查点'),
        };
      default:
        return null;
    }
  }

  return null;
}

export function normalizeSystemDirSegments(fullPath: string): string[] {
  return fullPath
    .replace(/\\/g, '/')
    .trim()
    .replace(/^\/+|\/+$/g, '')
    .split('/')
    .map(s => s.trim())
    .filter(Boolean);
}

function lookupNormalized(
  lowerSegments: readonly string[],
  tr: (s: string) => string,
): SystemDirHint | null {
  if (lowerSegments.length === 0) return null;
  const joined = lowerSegments.join('/');
  const exact = lookupExact(joined, tr) ?? matchWildcard(lowerSegments, tr);
  if (!exact) return null;
  return {
    os: exact.os,
    shortLabel: exact.shortLabel,
    detail: exact.detail,
    ...(exact.keyArtifacts ? { keyArtifacts: exact.keyArtifacts } : {}),
  };
}

/**
 * Returns forensic guidance for universal Linux / Windows system directories,
 * stripping at most one leading partition/volume segment (e.g. `分区1`, `part:3`, `C:`)
 * when it is not itself a known OS root directory, preventing false positives on nested paths.
 */
export function getSystemDirHint(
  fullPath: string,
  tr: (s: string) => string = identityTr,
): SystemDirHint | null {
  const lower = normalizeSystemDirSegments(fullPath).map(s => s.toLowerCase());
  if (lower.length === 0) return null;

  const direct = lookupNormalized(lower, tr);
  if (direct) return direct;

  if (lower.length >= 2 && !KNOWN_OS_ROOT_DIRS.has(lower[0])) {
    return lookupNormalized(lower.slice(1), tr);
  }
  return null;
}

export function formatSystemDirTooltip(
  name: string,
  hint: SystemDirHint,
  tr: (s: string) => string = identityTr,
): string {
  const lines = [`${name} — ${hint.shortLabel}`, hint.detail];
  if (hint.keyArtifacts?.length) {
    lines.push(`${tr('常见重点文件')}: ${hint.keyArtifacts.join(', ')}`);
  }
  return lines.join('\n');
}
