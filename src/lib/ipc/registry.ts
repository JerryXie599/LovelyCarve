import { invokeCmd } from './client';
import type { ImageId, NodeId } from './ids';

// ---- Windows 注册表配置单元 (regf) 预览（原类型视图） ----

export interface RegistryKeySummary {
  name: string;
  path: string;
  cellOffset: number;
  subkeyCount: number;
  valueCount: number;
  hasSubkeys: boolean;
  lastWritten: string | null;
  isDeleted: boolean;
}

export interface RegistryDigest {
  fileSize: number;
  hiveName: string;
  embeddedPath: string;
  version: string;
  primarySeq: number;
  secondarySeq: number;
  isDirty: boolean;
  hiveBinsSize: number;
  lastWritten: string | null;
  rootName: string;
  rootSubkeyCount: number;
  rootValueCount: number;
  sidecars: string[];
  deletedScanEnabled: boolean;
  notes: string[];
  rootChildren: RegistryKeySummary[];
}

export interface RegistryValueItem {
  name: string;
  isDefault: boolean;
  dataType: string;
  value: string;
  rawSize: number;
  dataSizeRaw: number;
  rawHex: string;
  hexTruncated: boolean;
  utf16Hint: string | null;
  isDeleted: boolean;
}

export interface RegistryKeyDetail {
  name: string;
  path: string;
  cellOffset: number;
  lastWritten: string | null;
  classNameSize: number;
  subkeyCount: number;
  valueCount: number;
  isDeleted: boolean;
  subkeys: RegistryKeySummary[];
  values: RegistryValueItem[];
  notes: string[];
}

export interface RegistrySearchHit {
  kind: 'key' | 'value';
  keyPath: string;
  keyOffset: number;
  valueName: string | null;
  dataType: string | null;
  preview: string;
  lastWritten: string | null;
  isDeleted: boolean;
}

export interface RegistrySearchResult {
  query: string;
  hits: RegistrySearchHit[];
  truncated: boolean;
}

export function fsRegistryPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
}): Promise<RegistryDigest> {
  return invokeCmd<RegistryDigest>('fs_registry_preview', args);
}

export function fsRegistryKey(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  keyPath: string;
  cellOffset?: number | null;
}): Promise<RegistryKeyDetail> {
  return invokeCmd<RegistryKeyDetail>('fs_registry_key', args);
}

export function fsRegistrySearch(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  query: string;
  limit?: number;
}): Promise<RegistrySearchResult> {
  return invokeCmd<RegistrySearchResult>('fs_registry_search', args);
}
