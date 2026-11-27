import { invokeCmd } from './client';
import type { ImageId, NodeId } from './ids';

// ---- EVTX 预览（原类型视图）。类型镜像 core/evtx_preview.rs 的
// Serialize 输出（camelCase）。单槽缓存解析结果，前端按页快速检索。

export interface EventCount {
  eventId: number;
  count: number;
  title: string;
}

export interface LevelCount {
  level: number;
  name: string;
  count: number;
}

export interface EvtxDigest {
  fileSize: number;
  chunkCount: number;
  nextRecordId: number;
  isDirty: boolean;
  isFull: boolean;
  majorVersion: number;
  minorVersion: number;
  totalRecords: number;
  channel: string | null;
  computer: string | null;
  earliestTime: string | null;
  latestTime: string | null;
  eventIdDistribution: EventCount[];
  levelDistribution: LevelCount[];
  notes: string[];
}

export interface EventDataField {
  name: string;
  value: string;
}

export interface EvtxRecordItem {
  recordId: number;
  timestamp: string;
  eventId: number;
  level: number;
  levelName: string;
  provider: string;
  channel: string;
  computer: string;
  title: string;
  summary: string;
  eventData: EventDataField[];
  xml: string;
}

export interface EvtxPage {
  total: number;
  filteredTotal: number;
  offset: number;
  limit: number;
  records: EvtxRecordItem[];
}

export function fsEvtxPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
}): Promise<EvtxDigest> {
  return invokeCmd<EvtxDigest>('fs_evtx_preview', args);
}

export function fsEvtxRecords(args: {
  imageId: ImageId;
  nodeId: NodeId;
  query?: string | null;
  level?: number | null;
  eventId?: number | null;
  offset: number;
  limit: number;
}): Promise<EvtxPage> {
  return invokeCmd<EvtxPage>('fs_evtx_records', args);
}
