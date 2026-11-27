/** VT100/xterm session emulator and ANSI/SGR color span extractor for forensic logs. */

export interface AnsiStyle {
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  inverse?: boolean;
  strikethrough?: boolean;
}

export interface TerminalSpan {
  text: string;
  style: AnsiStyle;
}

export interface TerminalLine {
  lineNumber: number;
  plain: string;
  spans: TerminalSpan[];
  section?: 'screen' | 'scrollback' | 'alt-screen';
}

export interface TerminalCommand {
  index: number;
  prompt: string;
  cwd?: string;
  command: string;
  lineNumber: number;
}

export interface TerminalMetadata {
  startedAt?: string;
  doneAt?: string;
  term?: string;
  tty?: string;
  columns?: number;
  rows?: number;
  exitCode?: string;
  titles: string[];
}

export interface TerminalSessionResult {
  isTerminalLog: boolean;
  hasAnsiColors: boolean;
  metadata: TerminalMetadata;
  lines: TerminalLine[];
  commands: TerminalCommand[];
  plainText: string;
}

interface Cell {
  ch: string;
  width: 0 | 1 | 2;
  style: AnsiStyle;
}

const ANSI_16: readonly string[] = [
  '#1f2328', // 0 black
  '#e5534b', // 1 red
  '#57ab5a', // 2 green
  '#c69026', // 3 yellow
  '#539bf5', // 4 blue
  '#b083f0', // 5 magenta
  '#39c5cf', // 6 cyan
  '#cdd9e5', // 7 white
  '#636e7b', // 8 bright black
  '#ff938a', // 9 bright red
  '#6bc46d', // 10 bright green
  '#daaa3f', // 11 bright yellow
  '#6cb6ff', // 12 bright blue
  '#dcbdfb', // 13 bright magenta
  '#56d4dd', // 14 bright cyan
  '#f0f6fc', // 15 bright white
];

const CUBE_LEVELS = [0, 95, 135, 175, 215, 255] as const;

function xterm256Color(index: number): string | undefined {
  if (!Number.isInteger(index) || index < 0 || index > 255) return undefined;
  if (index < 16) return ANSI_16[index];
  if (index >= 232) {
    const v = 8 + (index - 232) * 10;
    const hex = v.toString(16).padStart(2, '0');
    return `#${hex}${hex}${hex}`;
  }
  const n = index - 16;
  const r = CUBE_LEVELS[Math.floor(n / 36)];
  const g = CUBE_LEVELS[Math.floor((n % 36) / 6)];
  const b = CUBE_LEVELS[n % 6];
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
}

function rgbColor(r: number, g: number, b: number): string | undefined {
  if ([r, g, b].some(v => !Number.isInteger(v) || v < 0 || v > 255)) return undefined;
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
}

function cloneStyle(s: AnsiStyle): AnsiStyle {
  const out: AnsiStyle = {};
  if (s.fg) out.fg = s.fg;
  if (s.bg) out.bg = s.bg;
  if (s.bold) out.bold = true;
  if (s.dim) out.dim = true;
  if (s.italic) out.italic = true;
  if (s.underline) out.underline = true;
  if (s.inverse) out.inverse = true;
  if (s.strikethrough) out.strikethrough = true;
  return out;
}

function sameStyle(a: AnsiStyle, b: AnsiStyle): boolean {
  return (
    a.fg === b.fg &&
    a.bg === b.bg &&
    Boolean(a.bold) === Boolean(b.bold) &&
    Boolean(a.dim) === Boolean(b.dim) &&
    Boolean(a.italic) === Boolean(b.italic) &&
    Boolean(a.underline) === Boolean(b.underline) &&
    Boolean(a.inverse) === Boolean(b.inverse) &&
    Boolean(a.strikethrough) === Boolean(b.strikethrough)
  );
}

function isEmptyStyle(s: AnsiStyle): boolean {
  return !s.fg && !s.bg && !s.bold && !s.dim && !s.italic && !s.underline && !s.inverse && !s.strikethrough;
}

function charDisplayWidth(ch: string): 1 | 2 {
  const cp = ch.codePointAt(0);
  if (cp == null) return 1;
  if (
    (cp >= 0x1100 && cp <= 0x115f) ||
    cp === 0x2329 ||
    cp === 0x232a ||
    (cp >= 0x2e80 && cp <= 0x303e) ||
    (cp >= 0x3040 && cp <= 0x33ff) ||
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0xa960 && cp <= 0xa97c) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe10 && cp <= 0xfe19) ||
    (cp >= 0xfe30 && cp <= 0xfe6f) ||
    (cp >= 0xff01 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1faff) ||
    (cp >= 0x20000 && cp <= 0x3ffff)
  ) {
    return 2;
  }
  return 1;
}

/** Detect whether text content looks like a recorded VT100/ANSI terminal session (`script` log). */
export function isLikelyTerminalSession(text: string, filename?: string): boolean {
  if (!text) return false;
  const sample = text.length > 65536 ? text.slice(0, 65536) : text;
  if (/^Script started on /m.test(sample)) return true;

  let escCount = 0;
  for (let i = 0; i < sample.length - 1; i += 1) {
    if (sample.charCodeAt(i) === 0x1b) {
      const next = sample[i + 1];
      if (next === '[' || next === ']' || next === '(' || next === '=' || next === '>') {
        escCount += 1;
        if (escCount >= 3) return true;
      }
    }
  }
  if (escCount >= 1 && filename && /(?:typescript|session\.log|terminal|\.term\.log)$/i.test(filename)) {
    return true;
  }
  return false;
}

interface ScreenState {
  rows: Cell[][];
  row: number;
  col: number;
}

function emptyCell(style: AnsiStyle = {}): Cell {
  return { ch: ' ', width: 1, style: cloneStyle(style) };
}

function createScreen(): ScreenState {
  return { rows: [[]], row: 0, col: 0 };
}

function ensureRow(screen: ScreenState, r: number): Cell[] {
  while (screen.rows.length <= r) screen.rows.push([]);
  return screen.rows[r]!;
}

function ensureCol(rowCells: Cell[], col: number) {
  while (rowCells.length <= col) {
    rowCells.push(emptyCell());
  }
}

function applySgrParams(params: number[], style: AnsiStyle): boolean {
  let sawColor = false;
  const list = params.length ? params : [0];
  for (let i = 0; i < list.length; i += 1) {
    const code = list[i]!;
    if (code === 0) {
      delete style.fg;
      delete style.bg;
      delete style.bold;
      delete style.dim;
      delete style.italic;
      delete style.underline;
      delete style.inverse;
      delete style.strikethrough;
    } else if (code === 1) {
      style.bold = true;
      sawColor = true;
    } else if (code === 2) {
      style.dim = true;
      sawColor = true;
    } else if (code === 3) {
      style.italic = true;
    } else if (code === 4) {
      style.underline = true;
    } else if (code === 7) {
      style.inverse = true;
      sawColor = true;
    } else if (code === 9) {
      style.strikethrough = true;
    } else if (code === 22) {
      delete style.bold;
      delete style.dim;
    } else if (code === 23) {
      delete style.italic;
    } else if (code === 24) {
      delete style.underline;
    } else if (code === 27) {
      delete style.inverse;
    } else if (code === 29) {
      delete style.strikethrough;
    } else if (code >= 30 && code <= 37) {
      style.fg = ANSI_16[code - 30];
      sawColor = true;
    } else if (code === 39) {
      delete style.fg;
    } else if (code >= 40 && code <= 47) {
      style.bg = ANSI_16[code - 40];
      sawColor = true;
    } else if (code === 49) {
      delete style.bg;
    } else if (code >= 90 && code <= 97) {
      style.fg = ANSI_16[8 + (code - 90)];
      sawColor = true;
    } else if (code >= 100 && code <= 107) {
      style.bg = ANSI_16[8 + (code - 100)];
      sawColor = true;
    } else if (code === 38 || code === 48) {
      const isBg = code === 48;
      const mode = list[i + 1];
      if (mode === 5 && i + 2 < list.length) {
        const col = xterm256Color(list[i + 2]!);
        if (col) {
          if (isBg) style.bg = col;
          else style.fg = col;
          sawColor = true;
        }
        i += 2;
      } else if (mode === 2 && i + 4 < list.length) {
        const col = rgbColor(list[i + 2]!, list[i + 3]!, list[i + 4]!);
        if (col) {
          if (isBg) style.bg = col;
          else style.fg = col;
          sawColor = true;
        }
        i += 4;
      }
    }
  }
  return sawColor;
}

function rowCellsToOutput(
  cells: Cell[],
  lineNumber: number,
  section: 'screen' | 'scrollback' | 'alt-screen',
): TerminalLine {
  let end = cells.length;
  while (end > 0) {
    const cell = cells[end - 1]!;
    if (cell.width !== 0 && cell.ch === ' ' && isEmptyStyle(cell.style)) {
      end -= 1;
    } else {
      break;
    }
  }

  let plain = '';
  const spans: TerminalSpan[] = [];
  for (let i = 0; i < end; i += 1) {
    const cell = cells[i]!;
    if (cell.width === 0) continue;
    plain += cell.ch;
    const prev = spans[spans.length - 1];
    if (prev && sameStyle(prev.style, cell.style)) {
      prev.text += cell.ch;
    } else {
      spans.push({ text: cell.ch, style: cloneStyle(cell.style) });
    }
  }

  return { lineNumber, plain, spans, section };
}

function flushScreenRows(
  screen: ScreenState,
  section: 'screen' | 'scrollback' | 'alt-screen',
  out: TerminalLine[],
  dropTrailingBlanks: boolean,
) {
  let end = screen.rows.length;
  if (dropTrailingBlanks) {
    while (end > 0) {
      const candidate = rowCellsToOutput(screen.rows[end - 1]!, end, section);
      if (candidate.plain.length === 0) end -= 1;
      else break;
    }
  }
  for (let i = 0; i < end; i += 1) {
    const built = rowCellsToOutput(screen.rows[i]!, out.length + 1, section);
    out.push(built);
  }
}

function extractCommands(lines: TerminalLine[]): TerminalCommand[] {
  const cmds: TerminalCommand[] = [];
  let pendingKaliHeader: string | null = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i]!;
    const text = line.plain;
    const kaliTop = text.match(/^[┌┌│└─\s]*┌──\(([^)]+)\)-\[([^\]]+)\]/);
    if (kaliTop) {
      pendingKaliHeader = kaliTop[2] ?? '';
      continue;
    }

    const kaliCmd = text.match(/^└─([#$%])\s*(.*)$/);
    if (kaliCmd) {
      const cmd = (kaliCmd[2] ?? '').trim();
      if (cmd) {
        cmds.push({
          index: cmds.length + 1,
          prompt: `└─${kaliCmd[1]}`,
          cwd: pendingKaliHeader ?? undefined,
          command: cmd,
          lineNumber: line.lineNumber,
        });
      }
      pendingKaliHeader = null;
      continue;
    }
    pendingKaliHeader = null;

    const stdPrompt = text.match(
      /^(?:\([^)]+\)\s*)?([a-zA-Z0-9._-]+@[a-zA-Z0-9._-]+):([^\s#$]+)[#$]\s+(.+)$/,
    );
    if (stdPrompt) {
      const cmd = stdPrompt[3]!.trim();
      if (cmd) {
        cmds.push({
          index: cmds.length + 1,
          prompt: `${stdPrompt[1]}:${stdPrompt[2]}`,
          cwd: stdPrompt[2],
          command: cmd,
          lineNumber: line.lineNumber,
        });
      }
      continue;
    }

    const rhelPrompt = text.match(/^\[([a-zA-Z0-9._-]+@[a-zA-Z0-9._-]+\s+([^\]]+))\][#$]\s+(.+)$/);
    if (rhelPrompt) {
      const cmd = rhelPrompt[3]!.trim();
      if (cmd) {
        cmds.push({
          index: cmds.length + 1,
          prompt: `[${rhelPrompt[1]}]`,
          cwd: rhelPrompt[2],
          command: cmd,
          lineNumber: line.lineNumber,
        });
      }
    }
  }

  return cmds;
}

/** Parse and replay a recorded VT100/ANSI terminal log into clean lines, styles and commands. */
export function parseTerminalSession(raw: string): TerminalSessionResult {
  const isTerminalLog = isLikelyTerminalSession(raw);
  const metadata: TerminalMetadata = { titles: [] };
  const titleSet = new Set<string>();
  let body = raw;

  // Strip `script(1)` header line(s)
  const headerMatch = body.match(
    /^Script started on ([^\r\n[]+)(?:\[([^\]]+)\])?\r?\n/,
  );
  if (headerMatch) {
    metadata.startedAt = headerMatch[1]!.trim();
    const attrs = headerMatch[2] ?? '';
    const termM = attrs.match(/TERM="([^"]*)"/);
    const ttyM = attrs.match(/TTY="([^"]*)"/);
    const colM = attrs.match(/COLUMNS="(\d+)"/);
    const rowM = attrs.match(/LINES="(\d+)"/);
    if (termM) metadata.term = termM[1];
    if (ttyM) metadata.tty = ttyM[1];
    if (colM) metadata.columns = Number(colM[1]);
    if (rowM) metadata.rows = Number(rowM[1]);
    body = body.slice(headerMatch[0].length);
  }

  // Strip optional trailing `Script done on ...`
  const footerMatch = body.match(/\r?\nScript done on ([^\r\n[]+)(?:\[([^\]]+)\])?\s*$/);
  if (footerMatch) {
    metadata.doneAt = footerMatch[1]!.trim();
    const codeM = (footerMatch[2] ?? '').match(/COMMAND_EXIT_CODE="([^"]*)"/);
    if (codeM) metadata.exitCode = codeM[1];
    body = body.slice(0, body.length - footerMatch[0].length);
  }

  const maxCols = metadata.columns && metadata.columns > 20 ? Math.min(metadata.columns, 512) : 240;
  let primary = createScreen();
  let altScreen: ScreenState | null = null;
  let inAltScreen = false;
  let current = primary;
  let style: AnsiStyle = {};
  let hasAnsiColors = false;
  const outputLines: TerminalLine[] = [];

  const cps = Array.from(body);
  let i = 0;

  while (i < cps.length) {
    const ch = cps[i]!;
    const cp = ch.codePointAt(0)!;

    if (cp === 0x00 || cp === 0x07) {
      i += 1;
      continue;
    }
    if (cp === 0x08) {
      // BS: step back 1 column; skip padding zero-width trailing cell of a wide char
      if (current.col > 0) {
        current.col -= 1;
        const r = ensureRow(current, current.row);
        if (current.col > 0 && r[current.col]?.width === 0) {
          current.col -= 1;
        }
      }
      i += 1;
      continue;
    }
    if (cp === 0x09) {
      const nextStop = Math.min(maxCols, (Math.floor(current.col / 8) + 1) * 8);
      const r = ensureRow(current, current.row);
      ensureCol(r, nextStop - 1);
      current.col = nextStop;
      i += 1;
      continue;
    }
    if (cp === 0x0d) {
      current.col = 0;
      i += 1;
      continue;
    }
    if (cp === 0x0a) {
      current.row += 1;
      current.col = 0;
      ensureRow(current, current.row);
      i += 1;
      continue;
    }

    if (cp === 0x1b) {
      const next = cps[i + 1];
      if (next === '[') {
        // CSI sequence: ESC [ <params> <final byte 0x40..0x7E>
        let j = i + 2;
        while (j < cps.length) {
          const code = cps[j]!.codePointAt(0)!;
          if (code >= 0x40 && code <= 0x7e) break;
          j += 1;
        }
        if (j >= cps.length) break;
        const paramRaw = cps.slice(i + 2, j).join('');
        const finalChar = cps[j]!;
        const isPrivate = paramRaw.startsWith('?') || paramRaw.startsWith('>');
        const cleanParam = isPrivate ? paramRaw.slice(1) : paramRaw;
        const nums = cleanParam
          ? cleanParam.split(';').map(s => {
              const parsed = parseInt(s, 10);
              return Number.isFinite(parsed) ? parsed : 0;
            })
          : [];

        if (!isPrivate) {
          switch (finalChar) {
            case 'm':
              if (applySgrParams(nums, style)) hasAnsiColors = true;
              break;
            case 'A':
              current.row = Math.max(0, current.row - (nums[0] || 1));
              ensureRow(current, current.row);
              break;
            case 'B':
              current.row += nums[0] || 1;
              ensureRow(current, current.row);
              break;
            case 'C':
              current.col = Math.min(maxCols - 1, current.col + (nums[0] || 1));
              break;
            case 'D': {
              const delta = nums[0] || 1;
              current.col = Math.max(0, current.col - delta);
              break;
            }
            case 'G':
              current.col = Math.max(0, Math.min(maxCols - 1, (nums[0] || 1) - 1));
              break;
            case 'd':
              current.row = Math.max(0, (nums[0] || 1) - 1);
              ensureRow(current, current.row);
              break;
            case 'H':
            case 'f': {
              const targetRow = Math.max(0, (nums[0] || 1) - 1);
              const targetCol = Math.max(0, (nums[1] || 1) - 1);
              current.row = targetRow;
              current.col = targetCol;
              ensureRow(current, current.row);
              break;
            }
            case 'K': {
              const mode = nums[0] ?? 0;
              const r = ensureRow(current, current.row);
              if (mode === 0) {
                r.splice(current.col);
              } else if (mode === 1) {
                ensureCol(r, Math.max(0, current.col));
                for (let c = 0; c <= current.col && c < r.length; c += 1) {
                  r[c] = emptyCell(style);
                }
              } else if (mode === 2) {
                r.length = 0;
              }
              break;
            }
            case 'J': {
              const mode = nums[0] ?? 0;
              if (mode === 2 || mode === 3) {
                if (!inAltScreen) {
                  flushScreenRows(current, 'scrollback', outputLines, true);
                }
                current.rows = [[]];
                current.row = 0;
                current.col = 0;
              } else if (mode === 0) {
                const r = ensureRow(current, current.row);
                r.splice(current.col);
                current.rows.splice(current.row + 1);
              } else if (mode === 1) {
                for (let rIdx = 0; rIdx < current.row; rIdx += 1) {
                  current.rows[rIdx] = [];
                }
                const r = ensureRow(current, current.row);
                ensureCol(r, current.col);
                for (let c = 0; c <= current.col; c += 1) r[c] = emptyCell(style);
              }
              break;
            }
            case 'P': {
              const count = nums[0] || 1;
              const r = ensureRow(current, current.row);
              if (current.col < r.length) {
                r.splice(current.col, count);
              }
              break;
            }
            case 'X': {
              const count = nums[0] || 1;
              const r = ensureRow(current, current.row);
              ensureCol(r, current.col + count - 1);
              for (let k = 0; k < count; k += 1) {
                r[current.col + k] = emptyCell(style);
              }
              break;
            }
            case '@': {
              const count = nums[0] || 1;
              const r = ensureRow(current, current.row);
              ensureCol(r, current.col);
              const blanks = Array.from({ length: count }, () => emptyCell(style));
              r.splice(current.col, 0, ...blanks);
              break;
            }
            default:
              break;
          }
        } else if (paramRaw.startsWith('?')) {
          const pMode = nums[0];
          if ((pMode === 1049 || pMode === 1047 || pMode === 47) && finalChar === 'h') {
            if (!inAltScreen) {
              flushScreenRows(primary, 'scrollback', outputLines, true);
              primary = createScreen();
              altScreen = createScreen();
              current = altScreen;
              inAltScreen = true;
            }
          } else if ((pMode === 1049 || pMode === 1047 || pMode === 47) && finalChar === 'l') {
            if (inAltScreen && altScreen) {
              flushScreenRows(altScreen, 'alt-screen', outputLines, true);
              altScreen = null;
              current = primary;
              inAltScreen = false;
            }
          }
        }
        i = j + 1;
        continue;
      }

      if (next === ']') {
        // OSC sequence: terminated by BEL (\x07) or ST (\x1b\\)
        let j = i + 2;
        while (j < cps.length) {
          const code = cps[j]!.codePointAt(0)!;
          if (code === 0x07) break;
          if (code === 0x1b && cps[j + 1] === '\\') break;
          j += 1;
        }
        const oscPayload = cps.slice(i + 2, j).join('');
        const semi = oscPayload.indexOf(';');
        if (semi > 0) {
          const oscCode = oscPayload.slice(0, semi);
          const title = oscPayload.slice(semi + 1).trim();
          if ((oscCode === '0' || oscCode === '2') && title && !titleSet.has(title)) {
            titleSet.add(title);
            metadata.titles.push(title);
          }
        }
        if (j < cps.length && cps[j] === '\x1b' && cps[j + 1] === '\\') i = j + 2;
        else i = j + 1;
        continue;
      }

      if (next === '(' || next === ')' || next === '*' || next === '+' || next === '#' || next === '%') {
        i += 3;
        continue;
      }

      i += 2;
      continue;
    }

    if (cp < 0x20 || cp === 0x7f) {
      i += 1;
      continue;
    }

    const w = charDisplayWidth(ch);
    const r = ensureRow(current, current.row);
    // If cursor is on the trailing half of a wide char, blank the lead cell first
    if (current.col > 0 && r[current.col]?.width === 0) {
      r[current.col - 1] = emptyCell();
    }
    ensureCol(r, current.col + w - 1);
    // If writing a wide char over an adjacent lead wide char, blank its trailing half
    if (w === 2 && r[current.col + 1]?.width === 2 && r[current.col + 2]?.width === 0) {
      r[current.col + 2] = emptyCell();
    }
    r[current.col] = { ch, width: w, style: cloneStyle(style) };
    if (w === 2) {
      r[current.col + 1] = { ch: '', width: 0, style: cloneStyle(style) };
    }
    current.col += w;
    if (current.col >= maxCols && cps[i + 1] !== '\r' && cps[i + 1] !== '\n') {
      current.row += 1;
      current.col = 0;
      ensureRow(current, current.row);
    }
    i += 1;
  }

  if (inAltScreen && altScreen) {
    flushScreenRows(altScreen, 'alt-screen', outputLines, true);
  }
  flushScreenRows(primary, 'screen', outputLines, true);

  const plainText = outputLines.map(l => l.plain).join('\n');
  const commands = extractCommands(outputLines);

  return {
    isTerminalLog,
    hasAnsiColors,
    metadata,
    lines: outputLines,
    commands,
    plainText,
  };
}
