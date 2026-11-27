import { invokeCmd } from './client';
import type { ImageId, NodeId } from './ids';

export interface MftDigest {
  fileSize: number;
  recordSize: number;
  totalRecords: number;
  volumeName: string | null;
  ntfsVersion: string | null;
  volumeFlags: number | null;
  indexBuilt: boolean;
  indexedRecords: number;
  notes: string[];
}

export interface MftRecordSummary {
  recordNumber: number;
  sequence: number;
  inUse: boolean;
  isDir: boolean;
  name: string;
  fullPath: string;
  parentRecord: number;
  logicalSize: number;
  isResident: boolean;
  adsCount: number;
  siCreated: string | null;
  siModified: string | null;
  siMftModified: string | null;
  siAccessed: string | null;
  fnCreated: string | null;
  fnModified: string | null;
  timestompSuspect: boolean;
  timestompReasons: string[];
}

export type MftFilterKind =
  | 'all'
  | 'allocated'
  | 'deleted'
  | 'timestomped'
  | 'ads'
  | 'directories'
  | 'resident';

export interface MftPage {
  totalRecords: number;
  filteredTotal: number;
  offset: number;
  limit: number;
  indexed: boolean;
  items: MftRecordSummary[];
}

export interface MftStandardInfoDetail {
  created: string | null;
  modified: string | null;
  mftModified: string | null;
  accessed: string | null;
  rawFiletimes: [number, number, number, number];
  fileAttributes: number;
  fileAttributeFlags: string[];
  ownerId: number | null;
  securityId: number | null;
  usn: number | null;
  quotaCharged: number | null;
}

export interface MftFileNameDetail {
  name: string;
  namespace: string;
  parentRecord: number;
  parentSequence: number;
  created: string | null;
  modified: string | null;
  mftModified: string | null;
  accessed: string | null;
  rawFiletimes: [number, number, number, number];
  logicalSize: number;
  allocatedSize: number;
  fileFlags: number;
}

export interface MftDataRunInfo {
  vcnOffset: number;
  clusters: number;
  lcn: number | null;
}

export interface MftAttributeDetail {
  typeCode: number;
  typeName: string;
  instance: number;
  name: string;
  resident: boolean;
  flags: number;
  logicalSize: number;
  allocatedSize: number;
  residentPreviewText: string | null;
  residentPreviewHex: string | null;
  dataRuns: MftDataRunInfo[];
}

export interface MftRecordDetail {
  recordNumber: number;
  sequence: number;
  inUse: boolean;
  isDir: boolean;
  baseRecord: number;
  baseSequence: number;
  hardLinkCount: number;
  lsn: number;
  usedSize: number;
  allocatedSize: number;
  fullPath: string;
  standardInfo: MftStandardInfoDetail | null;
  filenames: MftFileNameDetail[];
  attributes: MftAttributeDetail[];
  timestompWarnings: string[];
  notes: string[];
}

export interface NtfsStreamExtentView {
  logicalOffset: number;
  storageLength: number;
  volumeOffset: number | null;
}

export interface NtfsAttributeForensicView {
  recordNumber: number;
  instance: number;
  attributeType: number;
  typeName: string;
  name: string;
  flags: number;
  resident: boolean;
  logicalSize: number;
  initializedSize: number;
  allocatedSize: number;
  lowestVcn: number;
  compressionUnitExponent: number;
  extents: NtfsStreamExtentView[];
}

export interface NtfsRecordForensicView {
  recordNumber: number;
  sequence: number;
  allocated: boolean;
  directory: boolean;
  baseRecord: number;
  baseSequence: number;
  hardLinkCount: number;
  logfileSequenceNumber: number;
  standardInfo: MftStandardInfoDetail | null;
  filenames: MftFileNameDetail[];
  attributes: NtfsAttributeForensicView[];
  timestompWarnings: string[];
  reparseTag: number | null;
  linkTarget: string | null;
  reparsePrintName: string | null;
}

export function fsMftPreview(args: { imageId: ImageId; nodeId: NodeId }): Promise<MftDigest> {
  return invokeCmd<MftDigest>('fs_mft_preview', args);
}

export function fsMftRecords(args: {
  imageId: ImageId;
  nodeId: NodeId;
  query?: string | null;
  filter?: MftFilterKind | string | null;
  offset: number;
  limit: number;
}): Promise<MftPage> {
  return invokeCmd<MftPage>('fs_mft_records', args);
}

export function fsMftRecordDetail(args: {
  imageId: ImageId;
  nodeId: NodeId;
  recordNumber: number;
}): Promise<MftRecordDetail> {
  return invokeCmd<MftRecordDetail>('fs_mft_record_detail', args);
}

export function fsNtfsRecordMetadata(args: {
  imageId: ImageId;
  nodeId: NodeId;
}): Promise<NtfsRecordForensicView | null> {
  return invokeCmd<NtfsRecordForensicView | null>('fs_ntfs_record_metadata', args);
}
