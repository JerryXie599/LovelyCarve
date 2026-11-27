import { invokeCmd } from './client';
import type { ImageId, NodeId } from './ids';
import type { DecodedBlob } from './sqlite';

export interface FernetDetection {
  column: string;
  tokenPrefix: string;
  timestampUtc8: string;
}

export interface PgColumn {
  attnum: number;
  name: string;
  typeOid: number;
  typeName: string;
  attlen: number;
  attalign: number;
}

export interface PgTableSummary {
  dbOid: number;
  dbName: string;
  relOid: number;
  relfilenode: number;
  toastRelid: number;
  name: string;
  nodeId: NodeId;
  sizeBytes: number;
  liveTuples: number;
  deadTuples: number;
  columns: PgColumn[];
}

export interface PgDigest {
  pgVersion: string | null;
  mode: string;
  rootNodeId: NodeId;
  databases: string[];
  tables: PgTableSummary[];
  notes: string[];
}

export interface PgTupleRow {
  page: number;
  itemIndex: number;
  xmin: number;
  xmax: number;
  deleted: boolean;
  values: string[];
  fernetTokens?: FernetDetection[];
}

export interface PgTableRows {
  tableName: string;
  columns: PgColumn[];
  rows: PgTupleRow[];
  totalLive: number;
  totalDead: number;
  offset: number;
  returned: number;
}

export function fsPgdataPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
}): Promise<PgDigest> {
  return invokeCmd<PgDigest>('fs_pgdata_preview', args);
}

export function fsPgdataRows(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  tableNodeId?: NodeId | null;
  includeDead?: boolean;
  offset?: number;
  limit?: number;
}): Promise<PgTableRows> {
  return invokeCmd<PgTableRows>('fs_pgdata_rows', args);
}

export interface LevelDbEntry {
  sourceFile: string;
  sequence: number;
  deleted: boolean;
  key: string;
  valuePreview: string;
  valueSize: number;
  decodedBlob?: DecodedBlob | null;
  fernet?: FernetDetection | null;
  metamaskVault: boolean;
}

export interface LevelDbDigest {
  filesParsed: string[];
  totalEntries: number;
  liveEntries: number;
  deletedEntries: number;
  metamaskVaults: number;
  fernetTokens: number;
  entries: LevelDbEntry[];
}

export function fsLeveldbPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
  parentId?: NodeId | null;
  query?: string | null;
  includeDeleted?: boolean;
  limit?: number;
}): Promise<LevelDbDigest> {
  return invokeCmd<LevelDbDigest>('fs_leveldb_preview', args);
}

export interface JournalRecord {
  seqnum: number;
  timestampUs: number;
  timestampUtc8: string;
  priority: number;
  unit: string;
  identifier: string;
  pid: string;
  uid: string;
  hostname: string;
  message: string;
}

export interface JournalDigest {
  state: string;
  machineId: string;
  bootId: string;
  compactMode: boolean;
  declaredEntries: number;
  parsedEntries: number;
  headTimeUtc8: string | null;
  tailTimeUtc8: string | null;
  records: JournalRecord[];
}

export function fsJournalPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
  query?: string | null;
  priorityMax?: number | null;
  limit?: number;
}): Promise<JournalDigest> {
  return invokeCmd<JournalDigest>('fs_journal_preview', args);
}

export interface VimSwapMeta {
  version: string;
  page_size: number;
  mtime: number;
  mtime_iso: string | null;
  inode: number;
  pid: number;
  user: string;
  host: string;
  fname: string;
}

export interface VimSwapDigest {
  meta: VimSwapMeta;
  lines: string[];
  bytes_read: number;
  truncated: boolean;
  data_block_count: number;
  pointer_block_count: number;
  unknown_block_count: number;
  encrypted: boolean;
  notes: string[];
}

export function fsVimSwapPreview(args: {
  imageId: ImageId;
  nodeId: NodeId;
}): Promise<VimSwapDigest> {
  return invokeCmd<VimSwapDigest>('fs_vim_swap_preview', args);
}
