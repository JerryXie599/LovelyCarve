<template>
  <div class="quick-jump-nav">
    <div class="qj-group qj-bookmarks">
      <div class="qj-header qj-bookmarks-header">
        <span class="qj-title qj-title-row">
          <Star :size="11" class="qj-star-icon" />
          <span>{{ tr("收藏路径") }}</span>
          <span v-if="bookmarks.length" class="qj-badge mono">{{ bookmarks.length }}</span>
        </span>
      </div>
      <div v-if="bookmarks.length" class="qj-bookmark-list">
        <div v-for="b in bookmarks" :key="b.id" class="qj-bookmark-row">
          <button
            type="button"
            class="ui-btn ghost qj-btn is-detected qj-bookmark-btn"
            :title="formatBookmarkSubpath(b)"
            @click="triggerBookmarkJump(b)"
          >
            <component :is="b.kind === 'file' ? FileText : Folder" :size="14" class="qj-icon" />
            <span class="qj-name">{{ b.label }}</span>
            <span class="qj-bookmark-sub mono">{{ formatBookmarkSubpath(b) }}</span>
          </button>
          <button
            type="button"
            class="ui-icon-btn xs qj-bookmark-remove"
            :title="tr('取消收藏')"
            :aria-label="tr('取消收藏')"
            @click.stop="removeBookmark(props.imageId, b.id)"
          >
            <X :size="12" />
          </button>
        </div>
      </div>
      <p v-else class="qj-bookmark-empty">{{ tr("点击地址栏星标或右键路径可快速收藏") }}</p>
    </div>

    <div class="qj-header">
      <span class="qj-title">{{ tr("常用路径直达") }}</span>
      <button
        type="button"
        class="ui-icon-btn xs"
        :title="tr('刷新常用路径检测')"
        :disabled="probing"
        @click="probeVolumes"
      >
        <RefreshCw :size="12" :class="{ spin: probing }" />
      </button>
    </div>

    <div v-if="detectedUsers.length > 1" class="qj-user-bar">
      <span class="qj-user-label">{{ tr("用户:") }}</span>
      <select v-model="selectedUserKey" class="ui-input xs mono qj-user-select">
        <option v-for="u in detectedUsers" :key="u.key" :value="u.key">
          {{ u.name }} ({{ u.volumeLabel }})
        </option>
      </select>
    </div>

    <div class="qj-groups">
      <div v-for="group in visibleGroups" :key="group.id" class="qj-group">
        <div class="qj-group-label">{{ group.label }}</div>
        <button
          v-for="item in group.items"
          :key="item.id"
          type="button"
          class="ui-btn ghost qj-btn"
          :class="{ 'is-detected': item.detected }"
          :title="item.resolvedTitle"
          @click="triggerJump(item)"
        >
          <component :is="item.icon" :size="14" class="qj-icon" />
          <span class="qj-name">{{ item.label }}</span>
          <span v-if="item.badge" class="qj-badge mono">{{ item.badge }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue';
import {
  Activity, Archive, Camera, Clock, Code2, Cpu, Database, Download, FileCode,
  FileText, Folder, FolderGit2, Globe, HardDrive, KeyRound, Layers, Lock, MapPin,
  MessageSquare, Monitor, Phone, RefreshCw, ShieldAlert, Smartphone, Star,
  Terminal, Trash2, Users, Wifi, X,
} from 'lucide-vue-next';
import { tr } from '../../../composables/useLocale';
import { fsChildren } from '../../../lib/ipc/fs/query';
import type { FsNode } from '../../../lib/ipc/fs/types';
import type { ImageId } from '../../../lib/ipc/ids';
import type { PathBookmark } from '../../../lib/ipc/bookmarks';
import type { FileLocationTarget } from '../../../lib/models/fileLocation';
import { useCaseContext } from '../../cases/api';
import { usePathBookmarks } from '../composables/usePathBookmarks';
import { formatBookmarkSubpath, normalizeLogicalRootId } from '../model/bookmarks';

const props = defineProps<{
  imageId: ImageId;
  platform?: string | null;
  isIosBackup?: boolean;
  isHarmonyBackup?: boolean;
}>();

const emit = defineEmits<{
  jump: [target: FileLocationTarget];
}>();

const { activeCase } = useCaseContext();
const { bookmarks, refreshCaseBookmarks, removeBookmark } = usePathBookmarks(() => props.imageId);

watch(
  () => [props.imageId, activeCase.value?.id ?? null] as const,
  ([id]) => {
    if (id) void refreshCaseBookmarks(id);
  },
  { immediate: true },
);

function triggerBookmarkJump(bookmark: PathBookmark) {
  if (bookmark.nodeId) {
    emit('jump', {
      imageId: props.imageId,
      nodeId: bookmark.nodeId,
    });
    return;
  }
  emit('jump', {
    imageId: props.imageId,
    rootId: bookmark.rootId || 'root',
    path: bookmark.path,
  });
}

interface VolumeProbe {
  rootId: string;
  label: string;
  topDirs: Map<string, string>; // lowerName -> actualName
}

interface DetectedUser {
  key: string;
  rootId: string;
  volumeLabel: string;
  usersDir: string;
  name: string;
}

interface QuickJumpItem {
  id: string;
  label: string;
  icon: Component;
  badge?: string;
  detected: boolean;
  rootId: string;
  path: string;
  resolvedTitle: string;
}

const probing = ref(false);
const volumes = ref<VolumeProbe[]>([]);
const detectedUsers = ref<DetectedUser[]>([]);
const selectedUserKey = ref('');
let probeSeq = 0;

const SKIP_WINDOWS_USERS = new Set([
  'public', 'default', 'default user', 'all users', 'desktop.ini',
]);

async function collectUsersUnderDir(
  imageId: ImageId,
  dirNode: FsNode,
  canonicalRootId: string,
  volumeLabel: string,
  isWindows: boolean,
  seq: number,
  usersOut: DetectedUser[],
) {
  const children = await fsChildren({ imageId, parentId: dirNode.id }).catch(() => [] as FsNode[]);
  if (seq !== probeSeq) return;
  for (const u of children) {
    if (u.kind !== 'dir') continue;
    if (isWindows && SKIP_WINDOWS_USERS.has(u.name.toLowerCase())) continue;
    usersOut.push({
      key: `${canonicalRootId}:${dirNode.name}/${u.name}`,
      rootId: canonicalRootId,
      volumeLabel,
      usersDir: dirNode.name,
      name: u.name,
    });
  }
}

async function probeOneRoot(
  imageId: ImageId,
  parentId: string,
  canonicalRootId: string,
  label: string,
  seq: number,
  vols: VolumeProbe[],
  users: DetectedUser[],
  depth = 0,
) {
  const children = await fsChildren({ imageId, parentId }).catch(() => [] as FsNode[]);
  if (seq !== probeSeq) return;
  const topDirs = new Map<string, string>();
  let usersNode: FsNode | null = null;
  let homeNode: FsNode | null = null;
  const subVols: FsNode[] = [];

  for (const c of children) {
    if (c.kind === 'dir' || c.kind === 'file') {
      const lower = c.name.toLowerCase();
      if (!topDirs.has(lower)) topDirs.set(lower, c.name);
      if (c.kind === 'dir' && lower === 'users') usersNode = c;
      if (c.kind === 'dir' && lower === 'home') homeNode = c;
      if (c.kind === 'dir' && (c.id.startsWith('lvm:') || c.id.startsWith('apfs:part:'))) {
        subVols.push(c);
      }
    }
  }

  vols.push({ rootId: canonicalRootId, label, topDirs });

  if (usersNode) {
    await collectUsersUnderDir(imageId, usersNode, canonicalRootId, label, true, seq, users);
    if (seq !== probeSeq) return;
  }
  if (homeNode) {
    await collectUsersUnderDir(imageId, homeNode, canonicalRootId, label, false, seq, users);
    if (seq !== probeSeq) return;
  }

  if (depth === 0 && subVols.length > 0) {
    for (const sub of subVols.slice(0, 6)) {
      await probeOneRoot(imageId, sub.id, normalizeLogicalRootId(sub.id), sub.name || sub.id, seq, vols, users, 1);
      if (seq !== probeSeq) return;
    }
  }
}

async function probeVolumes() {
  const id = props.imageId;
  if (!id) return;
  const seq = ++probeSeq;
  probing.value = true;
  try {
    const roots = await fsChildren({ imageId: id, parentId: 'root' }).catch(() => [] as FsNode[]);
    if (seq !== probeSeq) return;
    const dirRoots = roots.filter(r => r.kind === 'dir');
    const vols: VolumeProbe[] = [];
    const users: DetectedUser[] = [];

    const targets = dirRoots.length > 0 ? dirRoots.slice(0, 10) : [{ id: 'root', name: '/', kind: 'dir' } as FsNode];
    for (const root of targets) {
      if (root.id === 'root' && dirRoots.length > 0) continue;
      const canonicalRootId = normalizeLogicalRootId(root.id);
      await probeOneRoot(id, root.id, canonicalRootId, root.name || canonicalRootId, seq, vols, users);
      if (seq !== probeSeq) return;
    }

    volumes.value = vols;
    detectedUsers.value = users;
    if (users.length > 0 && !users.some(u => u.key === selectedUserKey.value)) {
      selectedUserKey.value = users[0].key;
    }
  } finally {
    if (seq === probeSeq) probing.value = false;
  }
}

watch(() => props.imageId, () => {
  volumes.value = [];
  detectedUsers.value = [];
  selectedUserKey.value = '';
  void probeVolumes();
}, { immediate: true });

const activeUser = computed<DetectedUser | null>(() => {
  return detectedUsers.value.find(u => u.key === selectedUserKey.value) ?? detectedUsers.value[0] ?? null;
});

function resolveVolumeForTopDir(topFolderLower: string, relTail: string): { detected: boolean; rootId: string; path: string } {
  for (const v of volumes.value) {
    const actualTop = v.topDirs.get(topFolderLower);
    if (actualTop) {
      const path = relTail ? `${actualTop}/${relTail}` : actualTop;
      return { detected: true, rootId: v.rootId, path };
    }
  }
  const fallbackRoot = volumes.value[0]?.rootId ?? 'root';
  const path = relTail ? `${topFolderLower}/${relTail}` : topFolderLower;
  return { detected: false, rootId: fallbackRoot, path };
}

function makeTopItem(
  id: string,
  label: string,
  icon: Component,
  topLower: string,
  relTail: string,
  badge?: string,
  displayTop?: string,
): QuickJumpItem {
  const res = resolveVolumeForTopDir(topLower, relTail);
  const path = !res.detected && displayTop
    ? (relTail ? `${displayTop}/${relTail}` : displayTop)
    : res.path;
  return {
    id,
    label,
    icon,
    badge,
    detected: res.detected,
    rootId: res.rootId,
    path,
    resolvedTitle: `${res.rootId} → /${path}`,
  };
}

function makeIosFsItem(
  id: string,
  label: string,
  icon: Component,
  relAfterPrivate: string,
  badge?: string,
): QuickJumpItem {
  for (const v of volumes.value) {
    const priv = v.topDirs.get('private');
    if (priv) {
      const path = relAfterPrivate ? `${priv}/${relAfterPrivate}` : priv;
      return {
        id,
        label,
        icon,
        badge,
        detected: true,
        rootId: v.rootId,
        path,
        resolvedTitle: `${v.rootId} → /${path}`,
      };
    }
    const firstSeg = relAfterPrivate.split('/')[0]?.toLowerCase() ?? '';
    const direct = firstSeg ? v.topDirs.get(firstSeg) : undefined;
    if (direct) {
      const rest = relAfterPrivate.slice(firstSeg.length).replace(/^\/+/, '');
      const path = rest ? `${direct}/${rest}` : direct;
      return {
        id,
        label,
        icon,
        badge,
        detected: true,
        rootId: v.rootId,
        path,
        resolvedTitle: `${v.rootId} → /${path}`,
      };
    }
  }
  const fallbackRoot = volumes.value[0]?.rootId ?? 'root';
  const path = relAfterPrivate ? `private/${relAfterPrivate}` : 'private';
  return {
    id,
    label,
    icon,
    badge,
    detected: false,
    rootId: fallbackRoot,
    path,
    resolvedTitle: `${fallbackRoot} → /${path}`,
  };
}

function makeUserItem(
  id: string,
  label: string,
  icon: Component,
  subPath: string,
  badge?: string,
): QuickJumpItem {
  const u = activeUser.value;
  if (u) {
    const full = subPath ? `${u.usersDir}/${u.name}/${subPath}` : `${u.usersDir}/${u.name}`;
    return {
      id,
      label,
      icon,
      badge,
      detected: true,
      rootId: u.rootId,
      path: full,
      resolvedTitle: `${u.volumeLabel} → /${full}`,
    };
  }
  return makeTopItem(id, label, icon, 'users', subPath, badge);
}

const isIosBackupDetected = computed(() =>
  Boolean(props.isIosBackup) ||
  volumes.value.some(v =>
    v.topDirs.has('homedomain') ||
    v.topDirs.has('rootdomain') ||
    v.topDirs.has('systempreferencesdomain') ||
    v.topDirs.has('camerarolldomain') ||
    v.topDirs.has('manifest.db') ||
    v.topDirs.has('manifest.plist'),
  ),
);

const isHarmonyNextDetected = computed(() =>
  Boolean(props.isHarmonyBackup) ||
  volumes.value.some(v =>
    v.topDirs.has('modules') ||
    v.topDirs.has('__backup') ||
    v.topDirs.has('.info.json'),
  ),
);

const hasWindows = computed(() =>
  volumes.value.some(v => v.topDirs.has('windows') || v.topDirs.has('users') || v.topDirs.has('$recycle.bin') || v.topDirs.has('program files')),
);
const hasLinuxOrMac = computed(() =>
  volumes.value.some(v => v.topDirs.has('etc') || v.topDirs.has('var') || v.topDirs.has('home') || v.topDirs.has('usr')),
);

const visibleGroups = computed(() => {
  const winGroup = {
    id: 'windows',
    label: tr('Windows 取证直达'),
    items: [
      makeTopItem('reg-config', tr('注册表 (config)'), Database, 'windows', 'System32/config', 'HIVE'),
      makeTopItem('evtx-logs', tr('事件日志 (EVTX)'), ShieldAlert, 'windows', 'System32/winevt/Logs', 'EVTX'),
      makeUserItem('user-desktop', tr('用户桌面 (Desktop)'), Monitor, 'Desktop'),
      makeUserItem('user-downloads', tr('下载目录 (Downloads)'), Download, 'Downloads'),
      makeUserItem('user-documents', tr('文档目录 (Documents)'), FileText, 'Documents'),
      makeUserItem('user-ntuser', tr('用户注册表 (NTUSER)'), KeyRound, 'NTUSER.DAT', 'REG'),
      makeUserItem('user-appdata-local', tr('AppData / Local'), Layers, 'AppData/Local'),
      makeUserItem('user-appdata-roaming', tr('AppData / Roaming'), Folder, 'AppData/Roaming'),
      makeUserItem('user-recent', tr('最近访问 (Recent/JumpList)'), Clock, 'AppData/Roaming/Microsoft/Windows/Recent'),
      makeTopItem('win-prefetch', tr('预读取 (Prefetch)'), Cpu, 'windows', 'Prefetch', 'PF'),
      makeTopItem('win-srum', tr('SRUM 资源统计 (sru)'), Activity, 'windows', 'System32/sru', 'ESE'),
      makeTopItem('win-amcache', tr('执行缓存 (Amcache)'), Archive, 'windows', 'AppCompat/Programs'),
      makeTopItem('win-tasks', tr('计划任务 (Tasks)'), FileCode, 'windows', 'System32/Tasks'),
      makeTopItem('win-recycle', tr('回收站 ($Recycle.Bin)'), Trash2, '$recycle.bin', ''),
      makeTopItem('win-users', tr('全部用户目录 (Users)'), Users, 'users', ''),
    ],
  };

  const linuxGroup = {
    id: 'linux',
    label: tr('Linux / 容器直达'),
    items: [
      makeTopItem('lin-etc', tr('系统配置 (/etc)'), FileCode, 'etc', ''),
      makeTopItem('lin-var-log', tr('系统日志 (/var/log)'), Terminal, 'var', 'log', 'LOG'),
      makeTopItem('lin-journal', tr('Journal 日志 (/var/log/journal)'), Database, 'var', 'log/journal', 'JNL'),
      makeTopItem('lin-root', tr('Root 主目录 (/root)'), KeyRound, 'root', ''),
      makeTopItem('lin-home', tr('用户目录 (/home)'), Users, 'home', ''),
      makeTopItem('lin-docker', tr('Docker 容器 (/var/lib/docker)'), HardDrive, 'var', 'lib/docker', 'DOCKER'),
      makeTopItem('lin-cron', tr('计划任务 (/var/spool/cron)'), Clock, 'var', 'spool/cron'),
      makeTopItem('lin-www', tr('Web 站点 (/var/www)'), Code2, 'var', 'www'),
      makeTopItem('lin-opt', tr('应用目录 (/opt)'), FolderGit2, 'opt', ''),
    ],
  };

  const iosBackupGroup = {
    id: 'ios-backup',
    label: tr('iOS 备份取证直达'),
    items: [
      makeTopItem('iosb-sms', tr('短信 / iMessage (SMS)'), MessageSquare, 'homedomain', 'Library/SMS', 'DB', 'HomeDomain'),
      makeTopItem('iosb-contacts', tr('通讯录 (AddressBook)'), Users, 'homedomain', 'Library/AddressBook', 'DB', 'HomeDomain'),
      makeTopItem('iosb-calls', tr('通话记录 (CallHistory)'), Phone, 'homedomain', 'Library/CallHistoryDB', 'DB', 'HomeDomain'),
      makeTopItem('iosb-notes', tr('备忘录 (Notes)'), FileText, 'homedomain', 'Library/Notes', 'DB', 'HomeDomain'),
      makeTopItem('iosb-safari', tr('Safari 浏览记录'), Globe, 'homedomain', 'Library/Safari', 'DB', 'HomeDomain'),
      makeTopItem('iosb-prefs', tr('系统偏好 (Preferences)'), Layers, 'homedomain', 'Library/Preferences', 'PLIST', 'HomeDomain'),
      makeTopItem('iosb-loc', tr('定位缓存 (locationd)'), MapPin, 'homedomain', 'Library/Caches/locationd', 'DB', 'HomeDomain'),
      makeTopItem('iosb-dcim', tr('相册照片 (DCIM)'), Camera, 'camerarolldomain', 'Media/DCIM', 'MEDIA', 'CameraRollDomain'),
      makeTopItem('iosb-apps', tr('第三方应用 (AppDomain)'), Smartphone, 'appdomain', '', 'APP', 'AppDomain'),
      makeTopItem('iosb-lockdown', tr('设备配对 (Lockdown)'), Lock, 'rootdomain', 'Library/Lockdown', 'PLIST', 'RootDomain'),
      makeTopItem('iosb-syspref', tr('系统配置域'), FileCode, 'systempreferencesdomain', '', 'PLIST', 'SystemPreferencesDomain'),
    ],
  };

  const iosFsGroup = {
    id: 'ios-fs',
    label: tr('iOS 文件系统直达'),
    items: [
      makeIosFsItem('iosf-sms', tr('短信数据库 (SMS)'), MessageSquare, 'var/mobile/Library/SMS', 'DB'),
      makeIosFsItem('iosf-contacts', tr('通讯录 (AddressBook)'), Users, 'var/mobile/Library/AddressBook', 'DB'),
      makeIosFsItem('iosf-calls', tr('通话记录 (CallHistory)'), Phone, 'var/mobile/Library/CallHistoryDB', 'DB'),
      makeIosFsItem('iosf-safari', tr('Safari 浏览记录'), Globe, 'var/mobile/Library/Safari', 'DB'),
      makeIosFsItem('iosf-notes', tr('备忘录 (Notes)'), FileText, 'var/mobile/Library/Notes', 'DB'),
      makeIosFsItem('iosf-apps', tr('应用沙盒 (Data/Application)'), Smartphone, 'var/mobile/Containers/Data/Application', 'APP'),
      makeIosFsItem('iosf-appgroup', tr('共享容器 (AppGroup)'), Layers, 'var/mobile/Containers/Shared/AppGroup', 'GRP'),
      makeIosFsItem('iosf-dcim', tr('相机胶卷 (DCIM)'), Camera, 'var/mobile/Media/DCIM', 'MEDIA'),
      makeIosFsItem('iosf-prefs', tr('移动偏好 (Preferences)'), FileCode, 'var/mobile/Library/Preferences', 'PLIST'),
      makeIosFsItem('iosf-keychains', tr('钥匙串 (Keychains)'), KeyRound, 'var/Keychains', 'KEY'),
      makeIosFsItem('iosf-diag', tr('统一诊断日志 (diagnostics)'), Activity, 'var/db/diagnostics', 'LOG'),
      makeIosFsItem('iosf-wireless', tr('基带/网络库 (wireless)'), Wifi, 'var/wireless/Library/Databases', 'DB'),
    ],
  };

  const macosGroup = {
    id: 'macos',
    label: tr('macOS 取证直达'),
    items: [
      makeTopItem('mac-apps', tr('应用程序 (/Applications)'), Smartphone, 'applications', ''),
      makeTopItem('mac-users', tr('用户目录 (/Users)'), Users, 'users', ''),
      makeTopItem('mac-logs', tr('系统日志 (/Library/Logs)'), Terminal, 'library', 'Logs', 'LOG'),
      makeIosFsItem('mac-etc', tr('系统配置 (/private/etc)'), FileCode, 'etc'),
      makeIosFsItem('mac-var-log', tr('诊断日志 (/private/var/log)'), Database, 'var/log', 'LOG'),
      makeTopItem('mac-sys', tr('系统库 (/System/Library)'), Layers, 'system', 'Library'),
    ],
  };

  const androidGroup = {
    id: 'android',
    label: tr('Android / 鸿蒙取证直达'),
    items: [
      makeTopItem('and-data', tr('应用数据 (/data/data)'), Smartphone, 'data', 'data', 'APP'),
      makeTopItem('and-sys', tr('系统配置 (/data/system)'), FileCode, 'data', 'system'),
      makeTopItem('and-wifi', tr('Wi-Fi 配置 (/data/misc/wifi)'), Wifi, 'data', 'misc/wifi'),
      makeTopItem('and-dcim', tr('相册目录 (/sdcard/DCIM)'), Camera, 'sdcard', 'DCIM', 'MEDIA'),
      makeTopItem('and-dl', tr('下载目录 (/sdcard/Download)'), Download, 'sdcard', 'Download'),
      makeTopItem('and-etc', tr('系统配置 (/system/etc)'), Layers, 'system', 'etc'),
    ],
  };

  const harmonyNextGroup = {
    id: 'harmony-next',
    label: tr('HarmonyOS NEXT 取证直达'),
    items: [
      makeTopItem('hm-backup', tr('备份清单 (.info.json)'), FileText, '__backup', '', 'INFO'),
      makeTopItem('hm-calls', tr('通话与联系人 (el1/rdb)'), Phone, 'modules', 'com.ohos.contactsdataability/data/storage/el1/database/rdb', 'RDB'),
      makeTopItem('hm-contacts', tr('联系人与删除记录 (el2/rdb)'), Users, 'modules', 'com.ohos.contactsdataability/data/storage/el2/database/rdb', 'RDB'),
      makeTopItem('hm-sms', tr('短信与软删短信 (SMS)'), MessageSquare, 'modules', 'com.ohos.telephonydataability/data/storage/el1/database', 'RDB'),
      makeTopItem('hm-notes', tr('备忘录 (Notepad)'), FileText, 'modules', 'com.huawei.hmos.notepad/data/storage/el2/database/rdb', 'RDB'),
      makeTopItem('hm-cal', tr('日历与日程 (Calendar)'), Clock, 'modules', 'com.huawei.hmos.calendardata/data/storage/el2/database/entry/rdb', 'RDB'),
      makeTopItem('hm-fsend', tr('文件快传 (FSend)'), Download, 'modules', 'com.xiaolifaa.fastfiletransfer/data/storage/el2/database/entry/rdb', 'RDB'),
      makeTopItem('hm-photos', tr('图库数据库 (Media)'), Camera, 'modules', 'com.ohos.medialibrary.medialibrarydata/data/storage/el2/database/rdb', 'MEDIA'),
      makeTopItem('hm-douyin', tr('抖音 (Aweme)'), MessageSquare, 'modules', 'com.ss.android.ugc.aweme/data/storage/el2/base/haps/entry/files', 'APP'),
    ],
  };

  const plat = (props.platform || '').trim().toLowerCase();
  if (plat === 'ios' || props.isIosBackup) {
    return isIosBackupDetected.value ? [iosBackupGroup] : [iosFsGroup];
  }
  if (plat === 'huaweihmos' || props.isHarmonyBackup) return [harmonyNextGroup];
  if (plat === 'harmony') return isHarmonyNextDetected.value ? [harmonyNextGroup] : [androidGroup];
  if (plat === 'windows') return [winGroup];
  if (plat === 'linux') return [linuxGroup];
  if (plat === 'macos') return [macosGroup];
  if (plat === 'android') return [androidGroup];

  if (isIosBackupDetected.value) return [iosBackupGroup];
  if (isHarmonyNextDetected.value) return [harmonyNextGroup];
  if (hasWindows.value && !hasLinuxOrMac.value) return [winGroup];
  if (hasLinuxOrMac.value && !hasWindows.value) return [linuxGroup];
  return [winGroup, linuxGroup];
});

function triggerJump(item: QuickJumpItem) {
  emit('jump', {
    imageId: props.imageId,
    rootId: item.rootId,
    path: item.path,
  });
}
</script>

<style scoped>
.quick-jump-nav {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 8px 12px 6px;
  border-top: 1px solid var(--border-color);
  overflow-y: auto;
  min-height: 0;
  flex: 1;
}
.qj-bookmarks {
  padding-bottom: 6px;
  margin-bottom: 2px;
  border-bottom: 1px solid var(--border-color);
}
.qj-bookmarks-header {
  margin-bottom: 2px;
}
.qj-title-row {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.qj-star-icon {
  color: var(--primary-color);
}
.qj-bookmark-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.qj-bookmark-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-width: 0;
}
.qj-bookmark-btn {
  flex: 1;
  min-width: 0;
}
.qj-bookmark-sub {
  font-size: 10px;
  color: var(--text-secondary);
  max-width: 88px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.qj-bookmark-remove {
  opacity: 0;
  flex-shrink: 0;
  color: var(--text-secondary);
}
.qj-bookmark-row:hover .qj-bookmark-remove,
.qj-bookmark-row:focus-within .qj-bookmark-remove {
  opacity: 1;
}
.qj-bookmark-remove:hover {
  color: var(--sev-critical-fg);
}
.qj-bookmark-empty {
  margin: 2px 6px 0;
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.4;
}
.qj-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 6px;
}
.qj-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  letter-spacing: 0.02em;
}
.qj-user-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 6px;
}
.qj-user-label {
  font-size: 11px;
  color: var(--text-secondary);
  flex-shrink: 0;
}
.qj-user-select {
  flex: 1;
  min-width: 0;
  height: 24px;
  font-size: 11px;
  padding: 0 6px;
}
.qj-groups {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.qj-group {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.qj-group-label {
  font-size: 10px;
  font-weight: 600;
  color: var(--text-secondary);
  padding: 2px 8px;
  opacity: 0.8;
}
.qj-btn {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 7px;
  width: 100%;
  height: 27px;
  padding: 0 8px;
  font-size: 12px;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  text-align: left;
}
.qj-btn.is-detected {
  color: var(--text-primary);
}
.qj-btn:hover {
  background: var(--sidebar-hover-bg, var(--hover-bg));
  color: var(--primary-color);
}
.qj-icon {
  flex-shrink: 0;
  opacity: 0.78;
}
.qj-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.qj-badge {
  font-size: 9px;
  padding: 1px 4px;
  border-radius: 3px;
  background: var(--surface-secondary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}
</style>
