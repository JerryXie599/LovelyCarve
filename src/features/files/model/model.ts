import { sourceText, type Translator } from '../../../i18n/translate';
import type { FsNode } from '../../../lib/ipc/fs/types';
import type { NodeId } from '../../../lib/ipc/ids';

/** Backend node with presentation-only path and stable row identity. */
export interface DiskFsNode extends FsNode {
  path?: string;
  rowKey?: string;
}

export interface ColumnDef {
  key: string;
  label: string;
  filterMode: 'text' | 'size' | 'time' | 'bool' | 'none';
  /** false = 数据来源尚未接通：设置列面板灰显禁选，防止整列「—」被解读成否定性结论。 */
  ready?: boolean;
}

export const COLUMNS: ColumnDef[] = [
  { key: 'index', label: '序号', filterMode: 'none' },
  { key: 'name', label: '名称', filterMode: 'text' },
  { key: 'ext', label: '扩展名', filterMode: 'text' },
  { key: 'kind', label: '类型', filterMode: 'text' },
  { key: 'size', label: '大小', filterMode: 'size' },
  { key: 'created', label: '创建时间', filterMode: 'time' },
  { key: 'accessed', label: '访问时间', filterMode: 'time' },
  { key: 'modified', label: '修改时间', filterMode: 'time' },
  { key: 'link', label: '链接指向', filterMode: 'text' },
  { key: 'inode', label: '元数据地址', filterMode: 'text' },
  { key: 'path', label: '全路径', filterMode: 'text' },
  { key: 'md5', label: 'MD5', filterMode: 'text' },
  { key: 'sha1', label: 'SHA1', filterMode: 'text' },
  { key: 'sha256', label: 'SHA256', filterMode: 'text' },
  { key: 'sm3', label: 'SM3', filterMode: 'text' },
  { key: 'tag', label: '标签', filterMode: 'text' },
  { key: 'ftype', label: '识别类型', filterMode: 'text' },
  { key: 'hashtype', label: '哈希类型', filterMode: 'none', ready: false },
  { key: 'encryption', label: '加密类型', filterMode: 'none', ready: false },
  { key: 'deleted', label: '已删除', filterMode: 'bool' },
];

export const DEFAULT_VISIBLE = new Set(['name', 'kind', 'size', 'modified']);

export const BOOL_FILTER_OPTIONS = [
  { value: '', label: '全部' },
  { value: '是', label: '是' },
  { value: '否', label: '否' },
] as const;

export function rowKeyOf(node: DiskFsNode): string {
  return node.rowKey ?? node.id;
}

export function isVirtualUnalloc(node: DiskFsNode): boolean {
  return node.id.startsWith('diskgap:') || node.id.startsWith('unalloc:');
}

/** A container root triggers the one-time Docker overlay composition. */
export function isHostDockerContainerRootId(id: string): boolean {
  return /^host-docker:[^:]+:$/.test(id);
}

/** 「应用」聚合视图（iOS 文件夹检材的容器聚合）的虚拟节点命名空间：
 * 计数已烘焙进名称（`xxx (N)`），不走子项计数 / 空目录探针。 */
export function isAppNodeId(id: string): boolean {
  return id === 'apps' || id.startsWith('app:') || id.startsWith('appdir:');
}

/** 解码 `folder:<hex>` / `appdir:<hex>` 节点 id 里的检材根相对路径。
 * 非该前缀、非法 hex 或非 UTF-8 时返回 null。文件夹检材专属。 */
export function hexIdRelPathOf(id: string, prefix: 'folder:' | 'appdir:'): string | null {
  if (!id.startsWith(prefix)) return null;
  const rest = id.slice(prefix.length);
  if (rest === '') return '';
  if (!/^[0-9a-f]*$/.test(rest) || rest.length % 2 !== 0) return null;
  const bytes = new Uint8Array(rest.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = parseInt(rest.slice(i * 2, i * 2 + 2), 16);
  }
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** 「应用」聚合视图里的行能否按真实路径回跳：虚拟目录节点（appdir:）与
 * 真实文件节点（folder:）都携带确定性路径；`apps` 根与 `app:<bundle>`
 * 层没有单一真实目录，不可跳。 */
export function canJumpToRealPath(id: string): boolean {
  if (id === 'apps' || id.startsWith('app:')) return false;
  if (id.startsWith('appdir:')) return hexIdRelPathOf(id, 'appdir:') != null;
  if (id.startsWith('folder:')) return hexIdRelPathOf(id, 'folder:') != null;
  return false;
}

/** Only the synthetic Docker group and an individual container root are valid
 * read-only mount targets. Descendants stay browsable/exportable but are not
 * presented as separate volumes. */
export function isHostDockerMountRoot(node: DiskFsNode | null): node is DiskFsNode {
  if (!node || node.kind !== 'dir') return false;
  return node.id === 'host-docker:' || isHostDockerContainerRootId(node.id);
}

export function kindLabel(node: DiskFsNode, tr: Translator = sourceText): string {
  if (isVirtualUnalloc(node)) return tr("未分配空间");
  if (node.kind === 'dir') return tr("目录");
  if (node.kind === 'file') return tr("文件");
  if (node.kind === 'symlink') return tr("符号链接");
  if (node.kind === 'unallocated') return tr("未分配");
  return node.kind;
}

export function extOf(node: DiskFsNode): string {
  if (node.kind === 'dir') return '';
  const idx = node.name.lastIndexOf('.');
  if (idx <= 0 || idx === node.name.length - 1) return '';
  return node.name.slice(idx + 1);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(2)} ${units[unit]}`;
}

export function pathFromStack(
  node: DiskFsNode,
  stack: Array<{ name: string }>,
): string {
  if (node.path) return node.path;
  const base = stack.slice(1).map(frame => frame.name).join('/');
  return base ? `/${base}/${node.name}` : `/${node.name}`;
}

// Shell-style wildcard matcher for file-name filters. `*` matches any string
// and `?` matches one code point; without wildcards it falls back to substring
// matching. The two-pointer glob avoids regex backtracking on adversarial names.
export function nameQueryMatcher(query: string): (lowerName: string) => boolean {
  if (!/[*?]/.test(query)) return name => name.includes(query);
  const pattern = Array.from(query);
  return name => {
    const chars = Array.from(name);
    let stringIndex = 0;
    let patternIndex = 0;
    let starPatternIndex = -1;
    let starStringIndex = 0;
    while (stringIndex < chars.length) {
      if (
        patternIndex < pattern.length &&
        (pattern[patternIndex] === '?' || pattern[patternIndex] === chars[stringIndex])
      ) {
        patternIndex += 1;
        stringIndex += 1;
      } else if (patternIndex < pattern.length && pattern[patternIndex] === '*') {
        starPatternIndex = patternIndex;
        patternIndex += 1;
        starStringIndex = stringIndex;
      } else if (starPatternIndex !== -1) {
        patternIndex = starPatternIndex + 1;
        starStringIndex += 1;
        stringIndex = starStringIndex;
      } else {
        return false;
      }
    }
    while (patternIndex < pattern.length && pattern[patternIndex] === '*') patternIndex += 1;
    return patternIndex === pattern.length;
  };
}

export type DiskFileSortContext = {
  digestOf: (nodeId: NodeId) => {
    md5?: string | null;
    sha1?: string | null;
    sha256?: string | null;
    sm3?: string | null;
  } | undefined;
  tagOf: (node: DiskFsNode) => string | undefined;
  ftypeLabelOf: (nodeId: NodeId) => string;
  fullPathOf: (node: DiskFsNode) => string;
};

export function sortValueOf(
  node: DiskFsNode,
  key: string,
  context: DiskFileSortContext,
): string | number {
  switch (key) {
    case 'name':
      return node.name.toLowerCase();
    case 'ext':
      return extOf(node).toLowerCase();
    case 'kind':
      return node.kind;
    case 'size':
      return node.size;
    case 'created':
      return node.timestamps?.created || node.timestamps?.birth || '';
    case 'accessed':
      return node.timestamps?.accessed || '';
    case 'modified':
      return node.timestamps?.modified || '';
    case 'inode':
      return node.inode ?? -1;
    case 'link':
      return node.link_target ?? '';
    case 'md5':
      return context.digestOf(node.id)?.md5 ?? '';
    case 'sha1':
      return context.digestOf(node.id)?.sha1 ?? '';
    case 'sha256':
      return context.digestOf(node.id)?.sha256 ?? '';
    case 'sm3':
      return context.digestOf(node.id)?.sm3 ?? '';
    case 'tag':
      return context.tagOf(node) ?? '';
    case 'ftype':
      return context.ftypeLabelOf(node.id);
    case 'path':
      return context.fullPathOf(node);
    case 'deleted':
      return node.deleted ? 1 : 0;
    default:
      return '';
  }
}
