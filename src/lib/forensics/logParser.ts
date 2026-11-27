import { isLikelyTerminalSession, parseTerminalSession } from './terminalSessionParser';

export type LogLevelKind = 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'OTHER';

export interface ParsedLogEntry {
  index: number;
  lineStart: number;
  lineEnd: number;
  timestamp: string;
  level: LogLevelKind;
  source: string;
  message: string;
  rawText: string;
}

export interface ParsedLogResult {
  formatLabel: string;
  entries: ParsedLogEntry[];
  earliestTime: string | null;
  latestTime: string | null;
  levelCounts: Record<LogLevelKind, number>;
  topSources: Array<{ source: string; count: number }>;
  timestampedLineCount: number;
  totalLineCount: number;
}

interface LineMatch {
  timestamp: string;
  levelHint?: string;
  sourceHint?: string;
  rest: string;
  format: string;
}

// 1. Bracketed Date/Time: [2026.07.28 02:13:45], [2026-07-28 02:13:45.123], [2026/07/28 02:13:45]
const RE_BRACKET_DATETIME =
  /^\[(\d{4}[.\-/]\d{1,2}[.\-/]\d{1,2}[T\s]\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,6})?(?:\s*(?:Z|[+-]\d{2}:?\d{2}))?)\]\s*(.*)$/;

// 2. Bracketed Time-Only (Telegram Desktop intra-day / game / app logs): [02:13:45] or [02:13:45.123]
const RE_BRACKET_TIME = /^\[(\d{2}:\d{2}:\d{2}(?:[.,]\d{1,6})?)\]\s*(.*)$/;

// 3. Unbracketed ISO / Standard Date-Time: 2026-07-28 02:13:45.123, 2026.07.28 02:13:45, 2026/07/28 02:13:45
//    Also handles Windows CBS/DISM: "2026-07-28 02:13:45, Info                  CBS    Message..."
const RE_ISO_DATETIME =
  /^(\d{4}[.\-/]\d{1,2}[.\-/]\d{1,2}[T\s]\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})?)[,\s]+(.*)$/;

// 4. Nginx / Apache Access Log: 1.2.3.4 - user [28/Jul/2026:02:13:45 +0800] "GET ..."
const RE_CLF_ACCESS =
  /^(\S+)\s+\S+\s+\S+\s+\[([0-3]\d\/[A-Za-z]{3}\/\d{4}:\d{2}:\d{2}:\d{2}\s+[+-]\d{4})\]\s+(.*)$/;

// 5. Syslog (RFC 3164): Jul 28 02:13:45 host process[123]: message
const RE_SYSLOG =
  /^([A-Z][a-z]{2}\s+[ \d]\d\s+\d{2}:\d{2}:\d{2})\s+(\S+)\s+([A-Za-z0-9_.\-/]+(?:\[\d+\])?):\s*(.*)$/;

// 6. Android Logcat: 07-28 02:13:45.123  1000  1020 I ActivityManager: message
const RE_LOGCAT =
  /^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+\d+\s+\d+\s+([VDIWEFA])\s+([^:]+):\s*(.*)$/;

function normalizeLevel(raw?: string, messageText?: string): LogLevelKind {
  if (raw) {
    const u = raw.trim().toUpperCase();
    if (
      u === 'E' ||
      u === 'F' ||
      u === 'A' ||
      u.startsWith('ERR') ||
      u.startsWith('FAT') ||
      u.startsWith('CRIT') ||
      u.startsWith('PANIC') ||
      u === 'SEVERE'
    ) {
      return 'ERROR';
    }
    if (u === 'W' || u.startsWith('WARN')) {
      return 'WARN';
    }
    if (u === 'I' || u.startsWith('INF') || u === 'NOTICE' || u === 'LOG') {
      return 'INFO';
    }
    if (u === 'D' || u === 'V' || u.startsWith('DEB') || u.startsWith('TRC') || u.startsWith('TRACE') || u.startsWith('VERB')) {
      return 'DEBUG';
    }
  }

  if (messageText) {
    const head = messageText.slice(0, 80);
    if (/\b(?:error|exception|fatal|critical|failed|failure|panic|crash)\b/i.test(head)) {
      return 'ERROR';
    }
    if (/\b(?:warn|warning|deprecated|timeout|retry)\b/i.test(head)) {
      return 'WARN';
    }
    if (/\b(?:debug|trace|verbose)\b/i.test(head)) {
      return 'DEBUG';
    }
  }
  return 'INFO';
}

function extractLevelAndSource(rest: string): {
  level: LogLevelKind;
  source: string;
  message: string;
} {
  let working = rest.trimStart();
  let explicitLevel: string | undefined;
  let source = '';

  // Pattern A: [LEVEL] [Source] message or [LEVEL] message
  const bracketLevel = working.match(
    /^\[(ERROR|ERR|WARN|WARNING|INFO|DEBUG|DBG|TRACE|FATAL|CRITICAL|VERBOSE)\]\s*(.*)$/i,
  );
  if (bracketLevel) {
    explicitLevel = bracketLevel[1];
    working = bracketLevel[2].trimStart();
  } else {
    // Pattern B: LEVEL [Source] or LEVEL: or LEVEL   Source (like Windows CBS: "Info                  CBS    ...")
    const bareLevel = working.match(
      /^(ERROR|ERR|WARN|WARNING|INFO|DEBUG|DBG|TRACE|FATAL|CRITICAL|SEVERE)(?:[:\s]+)(.*)$/i,
    );
    if (bareLevel) {
      explicitLevel = bareLevel[1];
      working = bareLevel[2].trimStart();
    }
  }

  // Extract optional [Source/Module] or "Module: " prefix (e.g., Telegram "API Warning: ...", "MTP Error: ...", "Launched version: ...")
  const bracketSource = working.match(/^\[([A-Za-z0-9_.\-/: ]{1,36})\]\s*(.*)$/);
  if (bracketSource && !/^(?:TRUE|FALSE|\d+)$/i.test(bracketSource[1])) {
    source = bracketSource[1].trim();
    working = bracketSource[2].trimStart();
  } else {
    // Windows CBS style: "CBS    Loaded..." or "CSI    00000001..."
    const cbsSource = working.match(/^([A-Z][A-Za-z0-9_.\-]{1,20})\s{2,}(.+)$/);
    if (cbsSource) {
      source = cbsSource[1].trim();
      working = cbsSource[2].trimStart();
    } else {
      // Telegram / General "Prefix Warning: ..." or "Component: ..." at start of message
      const colonPrefix = working.match(/^([A-Za-z][A-Za-z0-9_.\- ]{1,28}):\s+(.+)$/);
      if (colonPrefix) {
        const candidate = colonPrefix[1].trim();
        const lowerCandidate = candidate.toLowerCase();
        if (lowerCandidate.endsWith(' error') || lowerCandidate.endsWith(' err')) {
          explicitLevel = explicitLevel ?? 'ERROR';
          source = candidate.replace(/\s+(?:error|err)$/i, '').trim() || candidate;
          working = colonPrefix[2];
        } else if (lowerCandidate.endsWith(' warning') || lowerCandidate.endsWith(' warn')) {
          explicitLevel = explicitLevel ?? 'WARN';
          source = candidate.replace(/\s+(?:warning|warn)$/i, '').trim() || candidate;
          working = colonPrefix[2];
        } else if (candidate.split(/\s+/).length <= 3) {
          source = candidate;
          working = colonPrefix[2];
        }
      }
    }
  }

  const level = normalizeLevel(explicitLevel, working);
  return {
    level,
    source: source || '—',
    message: working || rest,
  };
}

function matchTimestampedLine(line: string): LineMatch | null {
  const trimmed = line.trimStart();
  if (!trimmed) return null;

  let m = trimmed.match(RE_BRACKET_DATETIME);
  if (m) {
    return {
      timestamp: m[1].replace('.', '-').replace('.', '-'),
      rest: m[2],
      format: 'Telegram / Bracketed Timestamp Log',
    };
  }

  m = trimmed.match(RE_BRACKET_TIME);
  if (m) {
    return {
      timestamp: m[1],
      rest: m[2],
      format: 'Bracketed Time Log',
    };
  }

  m = trimmed.match(RE_ISO_DATETIME);
  if (m) {
    return {
      timestamp: m[1],
      rest: m[2],
      format: 'ISO / Standard Timestamp Log',
    };
  }

  m = trimmed.match(RE_LOGCAT);
  if (m) {
    return {
      timestamp: m[1],
      levelHint: m[2],
      sourceHint: m[3].trim(),
      rest: m[4],
      format: 'Android Logcat',
    };
  }

  m = trimmed.match(RE_SYSLOG);
  if (m) {
    return {
      timestamp: m[1],
      sourceHint: `${m[2]} ${m[3]}`,
      rest: m[4],
      format: 'Syslog',
    };
  }

  m = trimmed.match(RE_CLF_ACCESS);
  if (m) {
    return {
      timestamp: m[2],
      sourceHint: m[1],
      rest: m[3],
      format: 'Web Access Log',
    };
  }

  return null;
}

/**
 * Checks whether a file should be treated as a structured log file.
 */
export function isLikelyStructuredLog(text: string, filename: string): boolean {
  if (!text) return false;
  if (isLikelyTerminalSession(text, filename)) return false;
  const lower = filename.toLowerCase();
  const isLogFilename =
    lower.endsWith('.log') ||
    lower === 'log.txt' ||
    lower.endsWith('_log.txt') ||
    lower.endsWith('-log.txt') ||
    lower.startsWith('log_') ||
    lower.startsWith('log-') ||
    lower.includes('.log.') ||
    lower === 'syslog' ||
    lower === 'messages' ||
    lower === 'auth.log' ||
    lower === 'secure' ||
    lower === 'dmesg';

  const sampleLines = text.slice(0, 32768).split(/\r?\n/);
  let nonEmpty = 0;
  let matched = 0;
  for (const line of sampleLines) {
    if (!line.trim()) continue;
    nonEmpty += 1;
    if (matchTimestampedLine(line)) {
      matched += 1;
    }
    if (nonEmpty >= 120) break;
  }

  if (nonEmpty === 0) return false;
  if (isLogFilename && matched >= 1) return true;
  return matched >= 3 && matched / nonEmpty >= 0.25;
}

/**
 * Parses a raw log text into structured table rows.
 * Even if the file is a `.log` / `log.txt` with some non-timestamped header/continuation lines,
 * continuation lines are attached to the preceding timestamped entry (or grouped cleanly).
 */
export function parseStructuredLog(text: string): ParsedLogResult {
  const cleanText = isLikelyTerminalSession(text) ? parseTerminalSession(text).plainText : text;
  const lines = cleanText.split(/\r?\n/);
  const entries: ParsedLogEntry[] = [];
  const levelCounts: Record<LogLevelKind, number> = {
    ERROR: 0,
    WARN: 0,
    INFO: 0,
    DEBUG: 0,
    OTHER: 0,
  };
  const sourceMap = new Map<string, number>();

  let detectedFormat = '';
  let timestampedLineCount = 0;
  let current: ParsedLogEntry | null = null;

  const flushCurrent = () => {
    if (!current) return;
    entries.push(current);
    levelCounts[current.level] = (levelCounts[current.level] ?? 0) + 1;
    if (current.source && current.source !== '—') {
      sourceMap.set(current.source, (sourceMap.get(current.source) ?? 0) + 1);
    }
    current = null;
  };

  for (let i = 0; i < lines.length; i += 1) {
    const rawLine = lines[i];
    const lineNo = i + 1;
    if (rawLine.length === 0 && i === lines.length - 1) continue;

    const matched = matchTimestampedLine(rawLine);
    if (matched) {
      flushCurrent();
      timestampedLineCount += 1;
      if (!detectedFormat) detectedFormat = matched.format;

      const parsed = extractLevelAndSource(matched.rest);
      const finalLevel = matched.levelHint
        ? normalizeLevel(matched.levelHint, parsed.message)
        : parsed.level;
      const finalSource = matched.sourceHint || parsed.source;

      current = {
        index: entries.length + 1,
        lineStart: lineNo,
        lineEnd: lineNo,
        timestamp: matched.timestamp,
        level: finalLevel,
        source: finalSource,
        message: parsed.message,
        rawText: rawLine,
      };
    } else if (current) {
      if (rawLine.trim().length > 0) {
        current.lineEnd = lineNo;
        current.message += `\n${rawLine}`;
        current.rawText += `\n${rawLine}`;
      }
    } else if (rawLine.trim().length > 0) {
      // Lines before the first timestamp (e.g. log header metadata)
      const parsed = extractLevelAndSource(rawLine);
      current = {
        index: entries.length + 1,
        lineStart: lineNo,
        lineEnd: lineNo,
        timestamp: '—',
        level: parsed.level === 'INFO' ? 'OTHER' : parsed.level,
        source: parsed.source !== '—' ? parsed.source : 'Header',
        message: parsed.message,
        rawText: rawLine,
      };
      flushCurrent();
    }
  }
  flushCurrent();

  let earliestTime: string | null = null;
  let latestTime: string | null = null;
  for (const e of entries) {
    if (e.timestamp && e.timestamp !== '—') {
      if (!earliestTime) earliestTime = e.timestamp;
      latestTime = e.timestamp;
    }
  }

  const topSources = Array.from(sourceMap.entries())
    .map(([source, count]) => ({ source, count }))
    .sort((a, b) => b.count - a.count)
    .slice(0, 20);

  return {
    formatLabel: detectedFormat || 'Structured Text Log',
    entries,
    earliestTime,
    latestTime,
    levelCounts,
    topSources,
    timestampedLineCount,
    totalLineCount: lines.length,
  };
}
