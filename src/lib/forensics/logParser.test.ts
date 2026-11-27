import { describe, expect, it } from 'vitest';
import { isLikelyStructuredLog, parseStructuredLog } from './logParser';

describe('logParser', () => {
  it('parses Telegram Desktop log.txt format with bracketed timestamps and continuations', () => {
    const sample = [
      '[2026.07.28 02:13:45] Launched version: 5002003, install beta: [FALSE], alpha: 0, debug mode: [FALSE]',
      '[2026.07.28 02:13:45] Executable dir: C:/Users/Tronic_Office/AppData/Roaming/Telegram Desktop/, name: Telegram.exe',
      '[2026.07.28 02:13:46] API Warning: not loaded minimal channel applied.',
      '[2026.07.28 02:13:47] MTP Error: could not connect to datacenter 2',
      '  stack trace line 1',
      '  stack trace line 2',
    ].join('\n');

    expect(isLikelyStructuredLog(sample, 'log.txt')).toBe(true);
    const parsed = parseStructuredLog(sample);
    expect(parsed.entries.length).toBe(4);
    expect(parsed.entries[0].timestamp).toBe('2026-07-28 02:13:45');
    expect(parsed.entries[0].source).toBe('Launched version');
    expect(parsed.entries[2].level).toBe('WARN');
    expect(parsed.entries[2].source).toBe('API');
    expect(parsed.entries[3].level).toBe('ERROR');
    expect(parsed.entries[3].source).toBe('MTP');
    expect(parsed.entries[3].lineStart).toBe(4);
    expect(parsed.entries[3].lineEnd).toBe(6);
    expect(parsed.entries[3].message).toContain('stack trace line 2');
  });

  it('parses ISO and Windows CBS log formats', () => {
    const sample = [
      '2026-07-28 02:15:01, Info                  CBS    Loaded Servicing Stack v10.0',
      '2026-07-28 02:15:02, Warning               DISM   Package state mismatch',
      '2026-07-28 02:15:03, Error                 CBS    Failed to process package',
    ].join('\n');

    expect(isLikelyStructuredLog(sample, 'CBS.log')).toBe(true);
    const parsed = parseStructuredLog(sample);
    expect(parsed.entries.length).toBe(3);
    expect(parsed.entries[0].source).toBe('CBS');
    expect(parsed.entries[1].level).toBe('WARN');
    expect(parsed.entries[2].level).toBe('ERROR');
  });
});
