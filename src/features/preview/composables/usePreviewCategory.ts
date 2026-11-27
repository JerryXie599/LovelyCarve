import type { PreviewCategory } from '../model/preview';
import { computed } from 'vue';
import {
  AR_EXT,
  AUDIO_EXT,
  CODE_EXT,
  CSV_EXT,
  EVTX_EXT,
  EXECUTABLE_EXT,
  GGUF_EXT,
  IMAGE_EXT,
  JOURNAL_EXT,
  KDBX_EXT,
  LEVELDB_EXT,
  MD_EXT,
  PE_EXT,
  REALM_EXT,
  RICH_EXT,
  SQLITE_EXT,
  STRUCTURED_EXT,
  VIDEO_EXT,
  VIM_SWAP_EXT,
  ZIP_EXT,
} from '../../../utils/fileCategory';
import type { FsTypeIdent } from '../../../lib/ipc/fs/derived';
import type { Delimiter } from '../model/csv';

interface UsePreviewCategoryOptions {
  nodeKind: () => string;
  nodeSize: () => number;
  ext: () => string;
  bytes: () => Uint8Array;
  effectiveIdent: () => FsTypeIdent | null;
  mediaCap: number;
}

const SQLITE_MAGIC = [
  0x53, 0x51, 0x4c, 0x69, 0x74, 0x65, 0x20, 0x66,
  0x6f, 0x72, 0x6d, 0x61, 0x74, 0x20, 0x33, 0x00,
];

const REALM_MAGIC_OFFSET = 16;
const REALM_MAGIC = [0x54, 0x2d, 0x44, 0x42]; // "T-DB"

const RENDERABLE_IMAGE_MIME = new Set([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/webp',
  'image/bmp',
  'image/svg+xml',
  'image/avif',
  'image/vnd.microsoft.icon',
  'image/x-icon',
]);

const RENDERABLE_AUDIO_MIME = new Set([
  'audio/mpeg',
  'audio/wav',
  'audio/x-wav',
  'audio/ogg',
  'audio/flac',
  'audio/mp4',
  'audio/aac',
]);

const RENDERABLE_VIDEO_MIME = new Set([
  'video/mp4',
  'video/webm',
  'video/quicktime',
  'video/x-matroska',
]);

const IDENT_RENDER_MIN = 0.9;

function looksTextual(bytes: Uint8Array): boolean {
  if (bytes.length === 0) return false;
  const length = Math.min(bytes.length, 4096);
  let printable = 0;
  for (let index = 0; index < length; index += 1) {
    const byte = bytes[index];
    if (byte === 0) return false;
    if (byte === 9 || byte === 10 || byte === 13 || (byte >= 0x20 && byte <= 0x7e) || byte >= 0x80) {
      printable += 1;
    }
  }
  return printable / length > 0.85;
}

function looksSqlite(bytes: Uint8Array): boolean {
  return bytes.length >= SQLITE_MAGIC.length && SQLITE_MAGIC.every((value, index) => bytes[index] === value);
}

function looksRealm(bytes: Uint8Array): boolean {
  return bytes.length >= REALM_MAGIC_OFFSET + REALM_MAGIC.length &&
    REALM_MAGIC.every((value, index) => bytes[REALM_MAGIC_OFFSET + index] === value);
}

const EVTX_MAGIC = [0x45, 0x6c, 0x66, 0x46, 0x69, 0x6c, 0x65, 0x00]; // "ElfFile\0"
const JOURNAL_MAGIC = [0x4c, 0x50, 0x4b, 0x53, 0x48, 0x48, 0x52, 0x48]; // "LPKSHHRH"
const REGISTRY_MAGIC = [0x72, 0x65, 0x67, 0x66]; // "regf"
const VIM_SWAP_MAGIC = [0x62, 0x30, 0x56, 0x49, 0x4d]; // "b0VIM"

function looksVimSwap(bytes: Uint8Array): boolean {
  return bytes.length >= VIM_SWAP_MAGIC.length && VIM_SWAP_MAGIC.every((value, index) => bytes[index] === value);
}

function looksEvtx(bytes: Uint8Array): boolean {
  return bytes.length >= EVTX_MAGIC.length && EVTX_MAGIC.every((value, index) => bytes[index] === value);
}

function looksSystemdJournal(bytes: Uint8Array): boolean {
  return bytes.length >= JOURNAL_MAGIC.length && JOURNAL_MAGIC.every((value, index) => bytes[index] === value);
}

function looksPgHeapPage(bytes: Uint8Array): boolean {
  if (bytes.length < 8192) return false;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const pdLower = view.getUint16(12, true);
  const pdUpper = view.getUint16(14, true);
  const pdSpecial = view.getUint16(16, true);
  const pageSizeVer = view.getUint16(18, true);
  return pageSizeVer === (8192 | 4)
    && pdLower >= 24 && pdLower <= 8192
    && pdUpper >= pdLower
    && pdSpecial >= pdUpper && pdSpecial <= 8192;
}

/** Windows 注册表主配置单元头：`regf` + `FileType::Primary (0)` @ 0x1c（排除 .LOG/.LOG1/.LOG2）。 */
function looksRegistryHive(bytes: Uint8Array): boolean {
  return bytes.length >= 32
    && REGISTRY_MAGIC.every((value, index) => bytes[index] === value)
    && bytes[28] === 0 && bytes[29] === 0 && bytes[30] === 0 && bytes[31] === 0;
}

/** NTFS `$MFT` / `$MFTMirr` 记录 #0 Header（魔数 `FILE` + `record_number == 0` + `base_record == 0` + 合法 2 的幂 record size）。 */
function looksMftFile(bytes: Uint8Array): boolean {
  if (bytes.length < 48) return false;
  if (bytes[0] !== 0x46 || bytes[1] !== 0x49 || bytes[2] !== 0x4c || bytes[3] !== 0x45) return false;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const usaCount = view.getUint16(6, true);
  const firstAttr = view.getUint16(20, true);
  const flags = view.getUint16(22, true);
  const usedSize = view.getUint32(24, true);
  const allocSize = view.getUint32(28, true);
  const baseLo = view.getUint32(32, true);
  const baseHi = view.getUint32(36, true);
  const recordNum = view.getUint32(44, true);
  return recordNum === 0
    && (flags & 0x01) !== 0
    && baseLo === 0 && baseHi === 0
    && usaCount > 1
    && allocSize >= 512 && allocSize <= 65536 && (allocSize & (allocSize - 1)) === 0
    && usedSize >= 48 && usedSize <= allocSize
    && firstAttr >= 48 && firstAttr < usedSize;
}

/** KeePass 2.x / KDBX (second signature 0xB54BFB67, or 0xB54BFB66 for KeePass 2 pre-releases). */
function looksKdbx(bytes: Uint8Array): boolean {
  return bytes.length >= 8 &&
    bytes[0] === 0x03 && bytes[1] === 0xd9 && bytes[2] === 0xa2 && bytes[3] === 0x9a &&
    (bytes[4] === 0x67 || bytes[4] === 0x66) && bytes[5] === 0xfb && bytes[6] === 0x4b && bytes[7] === 0xb5;
}

function categoryFromExtAndBytes(nodeKind: string, ext: string, bytes: Uint8Array): PreviewCategory {
  if (nodeKind !== 'file') return 'unsupported';
  const normalizedExt = ext.toLowerCase();
  if (MD_EXT.has(normalizedExt)) return 'markdown';
  if (IMAGE_EXT.has(normalizedExt)) return 'image';
  if (AUDIO_EXT.has(normalizedExt)) return 'audio';
  if (VIDEO_EXT.has(normalizedExt)) return 'video';
  if (PE_EXT.has(normalizedExt) || EXECUTABLE_EXT.has(normalizedExt) || AR_EXT.has(normalizedExt)) return 'binary';
  if (GGUF_EXT.has(normalizedExt)) return 'gguf';
  if (KDBX_EXT.has(normalizedExt)) return 'kdbx';
  if (REALM_EXT.has(normalizedExt)) return 'realm';
  if (EVTX_EXT.has(normalizedExt)) return 'evtx';
  if (JOURNAL_EXT.has(normalizedExt)) return 'journal';
  if (LEVELDB_EXT.has(normalizedExt)) return 'leveldb';
  if (SQLITE_EXT.has(normalizedExt)) return 'sqlite';
  if (RICH_EXT.has(normalizedExt)) return 'rich';
  if (ZIP_EXT.has(normalizedExt)) return 'zip';
  if (VIM_SWAP_EXT.has(normalizedExt)) return 'vimswap';
  if (STRUCTURED_EXT.has(normalizedExt)) return 'structured';
  if (CSV_EXT.has(normalizedExt)) return 'csv';
  if (CODE_EXT.has(normalizedExt)) return 'code';

  if (bytes.length >= 2 && bytes[0] === 0x4d && bytes[1] === 0x5a) return 'binary';
  if (bytes.length >= 4 && bytes[0] === 0x7f && bytes[1] === 0x45 && bytes[2] === 0x4c && bytes[3] === 0x46) return 'binary';
  if (
    bytes.length >= 4 &&
    ((bytes[0] === 0xfe && bytes[1] === 0xed && bytes[2] === 0xfa && (bytes[3] === 0xce || bytes[3] === 0xcf)) ||
      ((bytes[0] === 0xce || bytes[0] === 0xcf) && bytes[1] === 0xfa && bytes[2] === 0xed && bytes[3] === 0xfe))
  ) {
    return 'binary';
  }
  if (
    bytes.length >= 8 &&
    bytes[0] === 0x21 && bytes[1] === 0x3c && bytes[2] === 0x61 && bytes[3] === 0x72 &&
    bytes[4] === 0x63 && bytes[5] === 0x68 && bytes[6] === 0x3e && bytes[7] === 0x0a
  ) {
    return 'binary';
  }
  if (bytes.length >= 4 && bytes[0] === 0x47 && bytes[1] === 0x47 && bytes[2] === 0x55 && bytes[3] === 0x46) return 'gguf';
  if (looksKdbx(bytes)) return 'kdbx';
  if (looksRealm(bytes)) return 'realm';
  if (looksEvtx(bytes)) return 'evtx';
  if (looksSystemdJournal(bytes)) return 'journal';
  if (looksRegistryHive(bytes)) return 'registry';
  if (looksMftFile(bytes)) return 'mft';
  if (looksPgHeapPage(bytes)) return 'pgdata';
  if (looksSqlite(bytes)) return 'sqlite';
  if (bytes.length >= 2 && bytes[0] === 0x50 && bytes[1] === 0x4b) return 'zip';
  if (looksVimSwap(bytes)) return 'vimswap';
  return looksTextual(bytes) ? 'code' : 'unsupported';
}

function categoryFromIdent(ident: FsTypeIdent): PreviewCategory {
  const mime = ident.mime_type;
  if (ident.label === 'markdown' || mime === 'text/markdown') return 'markdown';
  if (mime === 'application/pdf') return 'rich';
  if (ident.label === 'docx' || ident.label === 'docm' || mime === 'application/vnd.openxmlformats-officedocument.wordprocessingml.document') return 'rich';
  if (
    ident.label === 'xlsx' || ident.label === 'xlsm' || ident.label === 'xls' || ident.label === 'xlsb' || ident.label === 'ods' ||
    mime === 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' ||
    mime === 'application/vnd.ms-excel.sheet.macroEnabled.12' ||
    mime === 'application/vnd.ms-excel' ||
    mime === 'application/vnd.ms-excel.sheet.binary.macroEnabled.12' ||
    mime === 'application/vnd.oasis.opendocument.spreadsheet'
  ) {
    return 'rich';
  }
  if (ident.label === 'pptx' || mime === 'application/vnd.openxmlformats-officedocument.presentationml.presentation') return 'rich';
  if (
    ident.label === 'doc' || ident.label === 'dot' || ident.label === 'rtf' || ident.label === 'odt' || ident.label === 'odp' ||
    mime === 'application/msword'
  ) {
    return 'rich';
  }
  if (ident.label === 'ppt' || ident.label === 'pps' || ident.label === 'pot' || mime === 'application/vnd.ms-powerpoint') return 'rich';
  if (
    ident.label === '7z' || ident.label === 'rar' || ident.label === 'tar' || ident.label === 'gzip' ||
    ident.label === 'bzip2' || ident.label === 'xz' || ident.label === 'cab' || ident.label === 'iso' ||
    mime === 'application/x-7z-compressed' || mime === 'application/vnd.rar' ||
    mime === 'application/x-tar' || mime === 'application/gzip' ||
    mime === 'application/x-bzip2' || mime === 'application/x-xz' ||
    mime === 'application/vnd.ms-cab-compressed' || mime === 'application/x-iso9660-image'
  ) {
    return 'rich';
  }
  if (ident.label === 'eml' || ident.label === 'msg' || ident.label === 'mbox' || mime === 'message/rfc822') return 'rich';
  if (ident.label === 'epub' || mime === 'application/epub+zip') return 'rich';
  if (ident.label === 'ofd' || mime === 'application/ofd') return 'rich';
  if (ident.label === 'sqlite' || ident.label === 'sqlite3') return 'sqlite';
  if (ident.label === 'realm') return 'realm';
  if (ident.label === 'evtx') return 'evtx';
  if (ident.label === 'winregistry' || mime === 'application/x-ms-registry') return 'registry';
  if (ident.label === 'ntfsmft' || mime === 'application/x-ntfs-mft') return 'mft';
  if (ident.label === 'pebin' || mime === 'application/x-dosexec') return 'binary';
  if (ident.label === 'elf' || mime === 'application/x-executable-elf') return 'binary';
  if (ident.label === 'macho' || mime === 'application/x-mach-o') return 'binary';
  if (ident.label === 'gguf') return 'gguf';
  if (
    ident.label === 'zip' || ident.label === 'jar' || ident.label === 'apk' ||
    mime === 'application/zip' ||
    mime === 'application/java-archive' ||
    mime === 'application/vnd.android.package-archive'
  ) {
    return 'zip';
  }
  if (RENDERABLE_IMAGE_MIME.has(mime)) return 'image';
  if (RENDERABLE_AUDIO_MIME.has(mime)) return 'audio';
  if (RENDERABLE_VIDEO_MIME.has(mime)) return 'video';
  if (ident.label === 'json' || ident.label === 'json5' || mime === 'application/json') return 'structured';
  if (ident.label === 'yaml' || mime === 'application/yaml' || mime === 'text/yaml' || mime === 'application/x-yaml') return 'structured';
  if (ident.label === 'xml' || mime === 'text/xml' || mime === 'application/xml') return 'structured';
  if (ident.label === 'csv' || mime === 'text/csv') return 'csv';
  if (ident.label === 'tsv' || mime === 'text/tab-separated-values') return 'csv';
  if (ident.is_text) return 'code';
  return 'unsupported';
}

function mimeForExt(ext: string): string {
  switch (ext.toLowerCase()) {
    case 'png': return 'image/png';
    case 'jpg':
    case 'jpeg': return 'image/jpeg';
    case 'gif': return 'image/gif';
    case 'webp': return 'image/webp';
    case 'bmp': return 'image/bmp';
    case 'ico': return 'image/x-icon';
    case 'avif': return 'image/avif';
    case 'svg': return 'image/svg+xml';
    case 'pdf': return 'application/pdf';
    case 'mp3': return 'audio/mpeg';
    case 'wav': return 'audio/wav';
    case 'ogg': return 'audio/ogg';
    case 'flac': return 'audio/flac';
    case 'm4a': return 'audio/mp4';
    case 'aac': return 'audio/aac';
    case 'mp4': return 'video/mp4';
    case 'webm': return 'video/webm';
    case 'mov': return 'video/quicktime';
    case 'mkv': return 'video/x-matroska';
    default: return 'application/octet-stream';
  }
}

export function usePreviewCategory(options: UsePreviewCategoryOptions) {
  const extCategory = computed<PreviewCategory>(() =>
    categoryFromExtAndBytes(options.nodeKind(), options.ext(), options.bytes()),
  );

  const identForRender = computed<FsTypeIdent | null>(() => {
    const ident = options.effectiveIdent();
    if (!ident) return null;
    return !ident.inferred || ident.score >= IDENT_RENDER_MIN ? ident : null;
  });

  const identCategory = computed<PreviewCategory>(() => {
    const ident = identForRender.value;
    return ident ? categoryFromIdent(ident) : 'unsupported';
  });

  const category = computed<PreviewCategory>(() => {
    if (options.nodeKind() !== 'file') return 'unsupported';
    const rawBytes = options.bytes();
    if (looksRegistryHive(rawBytes)) return 'registry';
    if (looksMftFile(rawBytes)) return 'mft';
    if (looksVimSwap(rawBytes)) return 'vimswap';
    // Magika has no KeePass label and reads the encrypted payload as noise;
    // the KDBX signature decides.
    if (looksKdbx(rawBytes)) return 'kdbx';
    const ident = identCategory.value;
    // 特例：扩展名已指明 rich（Flyfish 渲染）而 magika 只给出泛文本判定时让扩展名
    // 赢——否则 .eml/.rtf/.mbox 这类文本型扩展会被 text/plain 高置信度识别抢到
    // 代码视图。其余场景保持 ident 优先（改后缀的 .docx/.7z 靠高置信度识别进 rich）。
    if (ident !== 'unsupported' && !(ident === 'code' && extCategory.value === 'rich')) return ident;
    return extCategory.value;
  });

  const structuredFormat = computed<'json' | 'yaml' | 'xml' | undefined>(() => {
    const ext = options.ext().toLowerCase();
    if (ext === 'yaml' || ext === 'yml') return 'yaml';
    if (ext === 'xml') return 'xml';
    if (ext === 'json' || ext === 'json5' || ext === 'jsonc') return 'json';
    const ident = options.effectiveIdent();
    if (ident?.label === 'yaml') return 'yaml';
    if (ident?.label === 'xml') return 'xml';
    if (ident?.label === 'json' || ident?.label === 'json5') return 'json';
    return undefined;
  });

  const csvDelimiter = computed<Delimiter | undefined>(() => {
    const ext = options.ext().toLowerCase();
    if (ext === 'tsv') return '\t';
    const ident = options.effectiveIdent();
    if (ident?.label === 'tsv' || ident?.mime_type === 'text/tab-separated-values') return '\t';
    return undefined;
  });

  const extMismatch = computed<boolean>(() => {
    const ident = options.effectiveIdent();
    if (!ident || options.nodeKind() !== 'file') return false;
    const ext = options.ext().toLowerCase();
    if (!ext || ident.extensions.length === 0) return false;
    return !ident.extensions.some(candidate => candidate.toLowerCase() === ext);
  });

  const isMediaCategory = computed(() =>
    category.value === 'image' || category.value === 'audio' || category.value === 'video',
  );
  const isBlobMedia = computed(() => category.value === 'image');
  const isStreamMedia = computed(() => category.value === 'video' || category.value === 'audio');
  const mediaTooLarge = computed(() => isBlobMedia.value && options.nodeSize() > options.mediaCap);

  const effectiveMime = computed<string>(() => {
    const ident = identForRender.value;
    const identDrivenCategory = identCategory.value;
    if (ident && (identDrivenCategory === 'image' || identDrivenCategory === 'audio' || identDrivenCategory === 'video')) {
      return ident.mime_type === 'audio/x-wav' ? 'audio/wav' : ident.mime_type;
    }
    return mimeForExt(options.ext());
  });

  return {
    category,
    structuredFormat,
    csvDelimiter,
    extMismatch,
    isMediaCategory,
    isBlobMedia,
    isStreamMedia,
    mediaTooLarge,
    effectiveMime,
  };
}
