import { invokeCmd } from './client';
import type { ImageInfo } from './image';
import type { ImageId, NodeId } from './ids';

export type { ShadowCopyOrigin } from './image';

/** One Volume Shadow Copy of an NTFS volume (creation order, oldest first). */
export interface ShadowCopyInfo {
  index: number;
  store_id: string;
  shadow_copy_id: string | null;
  shadow_copy_set_id: string | null;
  creation_filetime: number;
  created_unix_ms: number | null;
  volume_size: number;
  snapshot_context: number | null;
  attributes: number | null;
  persistent: boolean;
  client_accessible: boolean;
  operating_machine: string | null;
  service_machine: string | null;
  /** 存储区缺少写时复制数据指针、无法打开时的原因。 */
  unavailable_reason: string | null;
  /** 已为该卷影副本打开的会话镜像 ID。 */
  open_image_id: string | null;
}

/** 一个 NTFS 卷上的卷影副本；`shadow_copies` 为空表示有 VSS 区但没有副本。 */
export interface ShadowVolumeInfo {
  partition_id: number;
  volume_size: number;
  shadow_copies: ShadowCopyInfo[];
  /** VSS 元数据读取失败的原因。 */
  error: string | null;
}

export function shadowCopyList(args: { imageId: ImageId }): Promise<ShadowVolumeInfo[]> {
  return invokeCmd<ShadowVolumeInfo[]>('shadow_copy_list', args);
}

export function shadowCopyOpen(args: {
  imageId: ImageId;
  partitionId: number;
  index: number;
  taskId?: string;
}): Promise<ImageInfo> {
  return invokeCmd<ImageInfo>('shadow_copy_open', args);
}

export interface ShadowCopyDiffItem {
  kind: 'deleted' | 'modified' | 'added';
  path: string;
  isDir: boolean;
  shadowRecord: number | null;
  liveRecord: number | null;
  shadowSize: number | null;
  liveSize: number | null;
  shadowModified: string | null;
  liveModified: string | null;
  shadowNodeId: string | null;
  liveNodeId: string | null;
  isForensicArtifact: boolean;
}

export interface ShadowCopyDiffReport {
  partitionId: number;
  snapshotIndex: number;
  storeId: string;
  shadowCopyId: string | null;
  snapshotCreatedUnixMs: number | null;
  shadowRecordCount: number;
  liveRecordCount: number;
  deletedCount: number;
  modifiedCount: number;
  addedCount: number;
  truncated: boolean;
  items: ShadowCopyDiffItem[];
}

export function shadowCopyDiff(args: {
  imageId: ImageId;
  partitionId: number;
  index: number;
  query?: string | null;
  kindFilter?: string | null;
  limit?: number;
  taskId?: string;
}): Promise<ShadowCopyDiffReport> {
  return invokeCmd<ShadowCopyDiffReport>('shadow_copy_diff', args);
}

export interface ShadowCopyFileExportResult {
  bytes_written: number;
}

export function shadowCopyFilePreview(args: {
  imageId: ImageId;
  partitionId: number;
  index: number;
  nodeId: NodeId;
  maxBytes?: number;
}): Promise<number[]> {
  return invokeCmd<number[]>('shadow_copy_file_preview', args);
}

export function shadowCopyFileExport(args: {
  imageId: ImageId;
  partitionId: number;
  index: number;
  nodeId?: NodeId | null;
  filePath?: string | null;
  destination: string;
}): Promise<ShadowCopyFileExportResult> {
  return invokeCmd<ShadowCopyFileExportResult>('shadow_copy_file_export', args);
}
