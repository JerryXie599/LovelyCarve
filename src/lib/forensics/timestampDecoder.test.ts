import { describe, expect, it } from 'vitest';
import {
  extractTimestampCandidates,
  parseHexDumpBytes,
} from './timestampDecoder';

describe('timestampDecoder', () => {
  it('parses formatted hex dump lines accurately', () => {
    const bytes = parseHexDumpBytes(
      '00000000  36 f2 3e 46 05 24 dd 01  |6.>F.$..|',
    );
    expect(Array.from(bytes)).toEqual([
      0x36, 0xf2, 0x3e, 0x46, 0x05, 0x24, 0xdd, 0x01,
    ]);
  });

  it('decodes Windows FILETIME QWORD from registry LastConnected screenshot sample', () => {
    const results = extractTimestampCandidates({
      rawHex: '00000000  36 f2 3e 46 05 24 dd 01  |6.>F.$..|',
      valueText: '0x01dd2405463ef236 (134303168962949686)',
      dataType: 'REG_QWORD',
      tzOffsetMinutes: 480,
    });
    const best = results.find((r) => r.id === 'filetime:le');
    expect(best).toBeDefined();
    expect(best?.plausible).toBe(true);
    expect(best?.iso).toBe('2026-08-04T11:34:56.294Z');
    expect(best?.utc).toContain('2026-08-04 11:34:56.');
    expect(best?.tzFormatted).toContain('2026-08-04 19:34:56.');
    expect(best?.tzLabel).toBe('UTC+08');
  });

  it('decodes 16-byte Windows SYSTEMTIME LE structure', () => {
    // 2026-08-05 14:22:33.123
    const leBytes =
      'ea07 0800 0300 0500 0e00 1600 2100 7b00';
    const results = extractTimestampCandidates({
      rawHex: leBytes,
      dataType: 'REG_BINARY',
      tzOffsetMinutes: 480,
    });
    const sys = results[0];
    expect(sys.id).toBe('systemtime:le');
    expect(sys.plausible).toBe(true);
    expect(sys.utc).toBe('2026-08-05 14:22:33.123 UTC');
    expect(sys.tzFormatted).toBe('2026-08-05 22:22:33.123 UTC+08');
  });

  it('decodes 32-bit Unix epoch REG_DWORD and string timestamps', () => {
    // 1785823328 = 0x6a718060 -> LE bytes 60 80 71 6a -> 2026-08-04T06:02:08.000Z
    const dwordResults = extractTimestampCandidates({
      rawHex: '00000000  60 80 71 6a              |`.qj|',
      dataType: 'REG_DWORD',
      tzOffsetMinutes: 0,
    });
    const unix = dwordResults.find((r) => r.id === 'unix-32:le');
    expect(unix?.plausible).toBe(true);
    expect(unix?.utc).toBe('2026-08-04 06:02:08.000 UTC');

    // 1785843296 -> 2026-08-04T11:34:56.000Z -> 19:34:56 UTC+08
    const strResults = extractTimestampCandidates({
      valueText: '1785843296',
      dataType: 'REG_SZ',
      tzOffsetMinutes: 480,
    });
    expect(strResults[0].tzFormatted).toBe('2026-08-04 19:34:56.000 UTC+08');
  });
});
