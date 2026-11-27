export type ByteEndian = 'le' | 'be';

export interface DecodedTimestampCandidate {
  id: string;
  format: string;
  endian: ByteEndian | 'n/a';
  rawValue: string;
  utc: string;
  tzFormatted: string;
  tzLabel: string;
  iso: string;
  year: number;
  plausible: boolean;
  priority: number;
}

export interface DecodeTimestampInput {
  rawHex?: string;
  valueText?: string;
  dataType?: string;
  endian?: ByteEndian;
  byteOffset?: number;
  tzOffsetMinutes?: number;
  includeAllRanges?: boolean;
}

const SEC_MS = 1000n;
const UNIX_TO_FILETIME_SEC = 11644473600n;
const COCOA_TO_UNIX_SEC = 978307200n;
const HFS_TO_UNIX_SEC = 2082844800n;
const OLE_EPOCH_MS = Date.UTC(1899, 11, 30, 0, 0, 0, 0);

const PLAUSIBLE_MIN_YEAR = 1995;
const PLAUSIBLE_MAX_YEAR = 2045;
const VALID_MIN_YEAR = 1970;
const VALID_MAX_YEAR = 2100;

/**
 * Extract raw bytes from either a formatted hex-dump string
 * (`00000000  36 f2 3e 46 05 24 dd 01  |6.>F.$..|`) or a plain hex sequence
 * (`36f23e460524dd01`, `0x...`).
 */
export function parseHexDumpBytes(rawHex: string): Uint8Array {
  const trimmed = rawHex.trim();
  if (!trimmed) return new Uint8Array(0);

  const lines = trimmed.split(/\r?\n/);
  const hasDumpLines = lines.some((line) => /^[0-9a-fA-F]{4,8}\s{2}/.test(line));
  if (hasDumpLines) {
    const out: number[] = [];
    for (const line of lines) {
      const m = /^[0-9a-fA-F]{4,8}\s{2}(.{1,48})/.exec(line);
      if (!m) continue;
      // Split before the `|ascii|` preview column if present.
      const beforePipe = m[1].split('|')[0];
      const matches = beforePipe.match(/\b[0-9a-fA-F]{2}\b/g);
      if (matches) {
        for (const byteHex of matches) {
          out.push(Number.parseInt(byteHex, 16));
        }
      }
    }
    return Uint8Array.from(out);
  }

  const normalized = trimmed.replace(/^0x/i, '').replace(/[^0-9a-fA-F]/g, '');
  if (normalized.length < 2) return new Uint8Array(0);
  const evenLen = normalized.length - (normalized.length % 2);
  const out = new Uint8Array(evenLen / 2);
  for (let i = 0; i < evenLen; i += 2) {
    out[i / 2] = Number.parseInt(normalized.slice(i, i + 2), 16);
  }
  return out;
}

export function formatTzLabel(tzOffsetMinutes: number): string {
  if (tzOffsetMinutes === 0) return 'UTC';
  const sign = tzOffsetMinutes >= 0 ? '+' : '-';
  const abs = Math.abs(tzOffsetMinutes);
  const hh = String(Math.floor(abs / 60)).padStart(2, '0');
  const mm = String(abs % 60).padStart(2, '0');
  return mm === '00' ? `UTC${sign}${hh}` : `UTC${sign}${hh}:${mm}`;
}

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function pad4(n: number): string {
  return String(n).padStart(4, '0');
}

function formatEpochMsWithSubNs(
  epochMs: number,
  subMsTicksStr: string,
  tzOffsetMinutes: number,
): { utc: string; tzFormatted: string; iso: string; year: number } | null {
  if (!Number.isFinite(epochMs)) return null;
  const dUtc = new Date(epochMs);
  if (Number.isNaN(dUtc.getTime())) return null;
  const year = dUtc.getUTCFullYear();
  if (year < 1601 || year > 2500) return null;

  const formatParts = (d: Date, suffix: string): string => {
    const msPart = String(d.getUTCMilliseconds()).padStart(3, '0') + subMsTicksStr;
    return `${pad4(d.getUTCFullYear())}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())} ${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())}:${pad2(d.getUTCSeconds())}.${msPart} ${suffix}`;
  };

  const utc = formatParts(dUtc, 'UTC');
  const shifted = new Date(epochMs + tzOffsetMinutes * 60_000);
  const tzLabel = formatTzLabel(tzOffsetMinutes);
  const tzFormatted = tzOffsetMinutes === 0 ? utc : formatParts(shifted, tzLabel);
  const iso = dUtc.toISOString();

  return { utc, tzFormatted, iso, year };
}

function formatFromUnixNano(
  unixNs: bigint,
  tzOffsetMinutes: number,
): { utc: string; tzFormatted: string; iso: string; year: number } | null {
  const nsPerMs = 1_000_000n;
  // Floor division so negative timestamps (pre-1970) stay consistent.
  let ms = unixNs / nsPerMs;
  let remNs = unixNs % nsPerMs;
  if (remNs < 0n) {
    ms -= 1n;
    remNs += nsPerMs;
  }
  const msNum = Number(ms);
  if (!Number.isSafeInteger(msNum)) return null;
  const subDigits = remNs > 0n ? String(remNs).padStart(6, '0').slice(0, 4).replace(/0+$/, '') : '';
  return formatEpochMsWithSubNs(msNum, subDigits, tzOffsetMinutes);
}

function readU32(bytes: Uint8Array, offset: number, endian: ByteEndian): number {
  const b0 = bytes[offset];
  const b1 = bytes[offset + 1];
  const b2 = bytes[offset + 2];
  const b3 = bytes[offset + 3];
  if (endian === 'le') {
    return (b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)) >>> 0;
  }
  return ((b0 << 24) | (b1 << 16) | (b2 << 8) | b3) >>> 0;
}

function readU64(bytes: Uint8Array, offset: number, endian: ByteEndian): bigint {
  let out = 0n;
  if (endian === 'le') {
    for (let i = 7; i >= 0; i--) {
      out = (out << 8n) | BigInt(bytes[offset + i]);
    }
  } else {
    for (let i = 0; i < 8; i++) {
      out = (out << 8n) | BigInt(bytes[offset + i]);
    }
  }
  return out;
}

function readU16(bytes: Uint8Array, offset: number, endian: ByteEndian): number {
  return endian === 'le'
    ? bytes[offset] | (bytes[offset + 1] << 8)
    : (bytes[offset] << 8) | bytes[offset + 1];
}

function readF64(bytes: Uint8Array, offset: number, endian: ByteEndian): number {
  const buf = new ArrayBuffer(8);
  const view = new DataView(buf);
  for (let i = 0; i < 8; i++) {
    view.setUint8(i, bytes[offset + i]);
  }
  return view.getFloat64(0, endian === 'le');
}

function isPlausibleYear(year: number): boolean {
  return year >= PLAUSIBLE_MIN_YEAR && year <= PLAUSIBLE_MAX_YEAR;
}

function isReasonableRange(year: number, includeAllRanges: boolean): boolean {
  if (includeAllRanges) return year >= 1900 && year <= 2200;
  return year >= VALID_MIN_YEAR && year <= VALID_MAX_YEAR;
}

function addIntegerCandidates(
  out: DecodedTimestampCandidate[],
  seenIds: Set<string>,
  val64: bigint | null,
  val32: number | null,
  f64Val: number | null,
  endian: ByteEndian | 'n/a',
  tzOffsetMinutes: number,
  includeAllRanges: boolean,
) {
  const tzLabel = formatTzLabel(tzOffsetMinutes);
  const push = (
    id: string,
    format: string,
    rawValue: string,
    formatted: { utc: string; tzFormatted: string; iso: string; year: number } | null,
    priority: number,
  ) => {
    if (!formatted) return;
    if (!isReasonableRange(formatted.year, includeAllRanges)) return;
    const fullId = `${id}:${endian}`;
    if (seenIds.has(fullId)) return;
    seenIds.add(fullId);
    out.push({
      id: fullId,
      format,
      endian,
      rawValue,
      utc: formatted.utc,
      tzFormatted: formatted.tzFormatted,
      tzLabel,
      iso: formatted.iso,
      year: formatted.year,
      plausible: isPlausibleYear(formatted.year),
      priority,
    });
  };

  if (val64 !== null && val64 > 0n) {
    const rawStr = `0x${val64.toString(16).padStart(16, '0')} (${val64.toString()})`;

    // 1. Windows FILETIME (100ns ticks since 1601-01-01)
    const filetimeUnixNs = (val64 - UNIX_TO_FILETIME_SEC * 10_000_000n) * 100n;
    push(
      'filetime',
      'Windows FILETIME (64-bit)',
      rawStr,
      formatFromUnixNano(filetimeUnixNs, tzOffsetMinutes),
      100,
    );

    // 2. Chrome / WebKit (microseconds since 1601-01-01)
    const webkitUnixNs = (val64 - UNIX_TO_FILETIME_SEC * 1_000_000n) * 1_000n;
    push(
      'webkit',
      'Chrome / WebKit (μs since 1601)',
      rawStr,
      formatFromUnixNano(webkitUnixNs, tzOffsetMinutes),
      80,
    );

    // 3. Unix Seconds (64-bit)
    if (val64 < 10_000_000_000n) {
      push(
        'unix-s-64',
        'Unix Epoch (Seconds, 64-bit)',
        val64.toString(),
        formatFromUnixNano(val64 * 1_000_000_000n, tzOffsetMinutes),
        88,
      );
    }

    // 4. Unix Milliseconds
    if (val64 >= 10_000_000_000n && val64 < 10_000_000_000_000n) {
      push(
        'unix-ms',
        'Unix Epoch (Milliseconds)',
        val64.toString(),
        formatFromUnixNano(val64 * 1_000_000n, tzOffsetMinutes),
        86,
      );
    }

    // 5. Unix Microseconds
    if (val64 >= 10_000_000_000_000n && val64 < 10_000_000_000_000_000n) {
      push(
        'unix-us',
        'Unix Epoch (Microseconds)',
        val64.toString(),
        formatFromUnixNano(val64 * 1_000n, tzOffsetMinutes),
        84,
      );
    }

    // 6. Unix Nanoseconds
    if (val64 >= 10_000_000_000_000_000n) {
      push(
        'unix-ns',
        'Unix Epoch (Nanoseconds)',
        val64.toString(),
        formatFromUnixNano(val64, tzOffsetMinutes),
        75,
      );
    }

    // 7. Apple Cocoa / Mac Absolute Time (seconds or nanoseconds since 2001-01-01)
    if (val64 < 3_000_000_000n) {
      push(
        'cocoa-s',
        'Apple Cocoa / CoreData (Seconds since 2001)',
        val64.toString(),
        formatFromUnixNano((val64 + COCOA_TO_UNIX_SEC) * 1_000_000_000n, tzOffsetMinutes),
        72,
      );
    }
    if (val64 >= 10_000_000_000_000_000n && val64 < 4_000_000_000_000_000_000n) {
      push(
        'cocoa-ns',
        'Apple Cocoa / CoreData (Nanoseconds since 2001)',
        val64.toString(),
        formatFromUnixNano(val64 + COCOA_TO_UNIX_SEC * 1_000_000_000n, tzOffsetMinutes),
        70,
      );
    }
  }

  if (f64Val !== null && Number.isFinite(f64Val)) {
    // IEEE 754 Double OLE Automation Date (days since 1899-12-30)
    if (f64Val >= 30_000 && f64Val <= 70_000) {
      const epochMs = Math.round(OLE_EPOCH_MS + f64Val * 86_400_000);
      push(
        'ole-f64',
        'OLE Automation Date (Float64 days)',
        String(f64Val),
        formatEpochMsWithSubNs(epochMs, '', tzOffsetMinutes),
        68,
      );
    }
    // Apple Cocoa Float64 seconds since 2001-01-01
    if (f64Val >= 1 && f64Val <= 1_800_000_000) {
      const epochMs = Math.round((f64Val + Number(COCOA_TO_UNIX_SEC)) * Number(SEC_MS));
      push(
        'cocoa-f64',
        'Apple Cocoa / CoreData (Float64 s)',
        String(f64Val),
        formatEpochMsWithSubNs(epochMs, '', tzOffsetMinutes),
        66,
      );
    }
    // Unix Epoch Float64 seconds
    if (f64Val >= 500_000_000 && f64Val <= 3_500_000_000) {
      const epochMs = Math.round(f64Val * Number(SEC_MS));
      push(
        'unix-f64',
        'Unix Epoch (Float64 s)',
        String(f64Val),
        formatEpochMsWithSubNs(epochMs, '', tzOffsetMinutes),
        64,
      );
    }
  }

  if (val32 !== null && val32 > 0) {
    const u32Hex = `0x${val32.toString(16).padStart(8, '0')} (${val32})`;
    // Unix 32-bit seconds
    push(
      'unix-32',
      'Unix Epoch (Seconds, 32-bit)',
      u32Hex,
      formatFromUnixNano(BigInt(val32) * 1_000_000_000n, tzOffsetMinutes),
      92,
    );

    // Apple Cocoa 32-bit seconds
    push(
      'cocoa-32',
      'Apple Cocoa (Seconds since 2001, 32-bit)',
      u32Hex,
      formatFromUnixNano((BigInt(val32) + COCOA_TO_UNIX_SEC) * 1_000_000_000n, tzOffsetMinutes),
      65,
    );

    // Mac HFS+ 32-bit seconds (since 1904-01-01)
    if (BigInt(val32) > HFS_TO_UNIX_SEC) {
      push(
        'hfs-32',
        'Mac HFS+ (Seconds since 1904)',
        u32Hex,
        formatFromUnixNano((BigInt(val32) - HFS_TO_UNIX_SEC) * 1_000_000_000n, tzOffsetMinutes),
        58,
      );
    }

    // FAT / MS-DOS 32-bit DateTime (low 16 = DOS Date, high 16 = DOS Time OR swapped)
    for (const [label, dateWord, timeWord] of [
      ['FAT / MS-DOS DateTime (Time|Date)', val32 >>> 16, val32 & 0xffff],
      ['FAT / MS-DOS DateTime (Date|Time)', val32 & 0xffff, val32 >>> 16],
    ] as const) {
      const day = dateWord & 0x1f;
      const month = (dateWord >> 5) & 0x0f;
      const year = 1980 + ((dateWord >> 9) & 0x7f);
      const sec = (timeWord & 0x1f) * 2;
      const min = (timeWord >> 5) & 0x3f;
      const hour = (timeWord >> 11) & 0x1f;
      if (
        year >= PLAUSIBLE_MIN_YEAR &&
        year <= PLAUSIBLE_MAX_YEAR &&
        month >= 1 &&
        month <= 12 &&
        day >= 1 &&
        day <= 31 &&
        hour <= 23 &&
        min <= 59 &&
        sec <= 59
      ) {
        const ms = Date.UTC(year, month - 1, day, hour, min, sec, 0);
        push(
          label.includes('Time|Date') ? 'dos-td' : 'dos-dt',
          label,
          u32Hex,
          formatEpochMsWithSubNs(ms, '', tzOffsetMinutes),
          55,
        );
      }
    }
  }
}

function tryDecodeSystemtime(
  bytes: Uint8Array,
  offset: number,
  endian: ByteEndian,
  tzOffsetMinutes: number,
  seenIds: Set<string>,
  out: DecodedTimestampCandidate[],
) {
  if (bytes.length - offset < 16) return;
  const year = readU16(bytes, offset, endian);
  const month = readU16(bytes, offset + 2, endian);
  const dow = readU16(bytes, offset + 4, endian);
  const day = readU16(bytes, offset + 6, endian);
  const hour = readU16(bytes, offset + 8, endian);
  const minute = readU16(bytes, offset + 10, endian);
  const second = readU16(bytes, offset + 12, endian);
  const millis = readU16(bytes, offset + 14, endian);

  if (
    year < 1970 ||
    year > 2100 ||
    month < 1 ||
    month > 12 ||
    dow > 6 ||
    day < 1 ||
    day > 31 ||
    hour > 23 ||
    minute > 59 ||
    second > 59 ||
    millis > 999
  ) {
    return;
  }

  const ms = Date.UTC(year, month - 1, day, hour, minute, second, millis);
  const formatted = formatEpochMsWithSubNs(ms, '', tzOffsetMinutes);
  if (!formatted) return;

  const id = `systemtime:${endian}`;
  if (seenIds.has(id)) return;
  seenIds.add(id);

  out.push({
    id,
    format: 'Windows SYSTEMTIME (16B)',
    endian,
    rawValue: `${year}-${pad2(month)}-${pad2(day)} ${pad2(hour)}:${pad2(minute)}:${pad2(second)}.${String(millis).padStart(3, '0')}`,
    utc: formatted.utc,
    tzFormatted: formatted.tzFormatted,
    tzLabel: formatTzLabel(tzOffsetMinutes),
    iso: formatted.iso,
    year,
    plausible: isPlausibleYear(year),
    priority: 110,
  });
}

function parseNumericToken(text: string): bigint | null {
  const t = text.trim();
  if (!t) return null;
  if (/^0x[0-9a-fA-F]{1,16}$/.test(t)) {
    return BigInt(t);
  }
  if (/^[0-9]{7,20}$/.test(t)) {
    return BigInt(t);
  }
  // E.g. `0x01dd2405463ef236 (134303168962949686)`
  const hexPrefix = /^(0x[0-9a-fA-F]{1,16})\s*\(/.exec(t);
  if (hexPrefix) {
    return BigInt(hexPrefix[1]);
  }
  return null;
}

export function extractTimestampCandidates(
  input: DecodeTimestampInput,
): DecodedTimestampCandidate[] {
  const bytes = input.rawHex ? parseHexDumpBytes(input.rawHex) : new Uint8Array(0);
  const offset = Math.max(0, input.byteOffset ?? 0);
  const preferredEndian: ByteEndian =
    input.endian ??
    (input.dataType === 'REG_DWORD_BIG_ENDIAN' ? 'be' : 'le');
  const tzOffsetMinutes = input.tzOffsetMinutes ?? 480; // Default UTC+08:00
  const includeAllRanges = input.includeAllRanges ?? false;

  const out: DecodedTimestampCandidate[] = [];
  const seenIds = new Set<string>();

  if (bytes.length - offset >= 4) {
    const endians: ByteEndian[] =
      preferredEndian === 'le' ? ['le', 'be'] : ['be', 'le'];

    for (const endian of endians) {
      tryDecodeSystemtime(bytes, offset, endian, tzOffsetMinutes, seenIds, out);
      const val64 = bytes.length - offset >= 8 ? readU64(bytes, offset, endian) : null;
      const val32 = readU32(bytes, offset, endian);
      const f64 = bytes.length - offset >= 8 ? readF64(bytes, offset, endian) : null;
      addIntegerCandidates(
        out,
        seenIds,
        val64,
        val32,
        f64,
        endian,
        tzOffsetMinutes,
        includeAllRanges,
      );
    }
  } else if (input.valueText) {
    const num = parseNumericToken(input.valueText);
    if (num !== null) {
      const val32 = num <= 0xffffffffn ? Number(num) : null;
      addIntegerCandidates(
        out,
        seenIds,
        num,
        val32,
        null,
        'n/a',
        tzOffsetMinutes,
        includeAllRanges,
      );
    }
  }

  // Also parse string integers inside REG_SZ/REG_EXPAND_SZ even when rawHex is UTF-16LE bytes.
  if (bytes.length > 0 && input.valueText && out.every((c) => !c.plausible)) {
    const num = parseNumericToken(input.valueText);
    if (num !== null) {
      const val32 = num <= 0xffffffffn ? Number(num) : null;
      addIntegerCandidates(
        out,
        seenIds,
        num,
        val32,
        null,
        'n/a',
        tzOffsetMinutes,
        includeAllRanges,
      );
    }
  }

  out.sort((a, b) => {
    if (a.plausible !== b.plausible) return a.plausible ? -1 : 1;
    const aPreferred = a.endian === preferredEndian || a.endian === 'n/a' ? 1 : 0;
    const bPreferred = b.endian === preferredEndian || b.endian === 'n/a' ? 1 : 0;
    if (aPreferred !== bPreferred) return bPreferred - aPreferred;
    if (a.priority !== b.priority) return b.priority - a.priority;
    return a.id.localeCompare(b.id);
  });

  return out;
}
