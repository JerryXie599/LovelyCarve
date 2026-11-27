import type {
  ContestQuestion,
  ContestWorkspace,
  PinTarget,
  QuestionStatus,
} from '../../../lib/ipc/contest';

export const CONTEST_AI_FORMAT_PROMPT = `请帮我将以下电子取证比赛题目整理为 LovelyCarve 标准 Markdown 赛题格式（严格遵守以下规则，方便直接导入）：
1. 可在最上方用 \`# 比赛名称\` 标注总标题；
2. 每个检材用二级标题 \`## 检材名称\`（如 \`## VTM-1\`），检材标题下方可写一段纯文本案情摘要；
3. 每道题目用三级标题 \`### 题号. 题干内容（分值:1）（答案格式:...）\`，括号里的分值和答案格式没有可省略；
4. 若有选项，每个选项单占一行，以 \`A. \`、\`B. \`、\`C. \`、\`D. \` 开头；
5. 不要输出多余寒暄或解析占位，直接输出整理后的 Markdown 代码块。`;

export interface WorkspaceSummary {
  totalQuestions: number;
  answeredQuestions: number;
  reviewQuestions: number;
  stuckQuestions: number;
  totalExhibits: number;
  boundExhibits: number;
  totalPins: number;
  capturedPins: number;
}

export function summarizeWorkspace(ws: ContestWorkspace | null): WorkspaceSummary {
  if (!ws) {
    return {
      totalQuestions: 0,
      answeredQuestions: 0,
      reviewQuestions: 0,
      stuckQuestions: 0,
      totalExhibits: 0,
      boundExhibits: 0,
      totalPins: 0,
      capturedPins: 0,
    };
  }
  return {
    totalQuestions: ws.questions.length,
    answeredQuestions: ws.questions.filter(q => q.status === 'answered').length,
    reviewQuestions: ws.questions.filter(q => q.status === 'review').length,
    stuckQuestions: ws.questions.filter(q => q.status === 'stuck').length,
    totalExhibits: ws.exhibits.length,
    boundExhibits: ws.exhibits.filter(e => Boolean(e.imageId)).length,
    totalPins: ws.pins.length,
    capturedPins: ws.pins.filter(p => Boolean(p.screenshotRel)).length,
  };
}

export function filterQuestions(
  ws: ContestWorkspace | null,
  options: { exhibitId?: string; status?: QuestionStatus | 'all'; keyword?: string },
): ContestQuestion[] {
  if (!ws) return [];
  const kw = (options.keyword ?? '').trim().toLowerCase();
  return ws.questions.filter(q => {
    if (options.exhibitId && options.exhibitId !== 'all' && q.exhibitId !== options.exhibitId) {
      return false;
    }
    if (options.status && options.status !== 'all' && q.status !== options.status) {
      return false;
    }
    if (!kw) return true;
    const haystack = `${q.id} ${q.number} ${q.stem} ${q.answer} ${q.options.join(' ')}`.toLowerCase();
    return haystack.includes(kw);
  });
}

export function formatPinTargetLabel(target: PinTarget): string {
  switch (target.kind) {
    case 'file':
      return target.metaAddr ? `${target.path} (#${target.metaAddr})` : target.path;
    case 'analysis_record':
      return `${target.categoryName || target.categoryId} #${target.recordId}`;
    case 'hex_range':
      return `${target.path} @ 0x${target.offset.toString(16).toUpperCase()}+0x${target.length.toString(16).toUpperCase()}`;
    case 'sqlite_row':
      return target.rowId
        ? `${target.path} [${target.table}#${target.rowId}]`
        : `${target.path} [${target.table}]`;
    case 'custom':
      return target.detail;
  }
}

export function extractAssetRefs(markdown: string): string[] {
  const refs = new Set<string>();
  const regex = /!\[[^\]]*\]\(((?:\.\/)?assets\/[^)\s]+)\)/g;
  let match: RegExpExecArray | null = null;
  while ((match = regex.exec(markdown)) !== null) {
    const raw = match[1].replace(/^\.\//, '');
    refs.add(raw);
  }
  return Array.from(refs);
}

export function replaceMarkdownAssetUrls(
  markdown: string,
  assetMap: Record<string, string>,
): string {
  return markdown.replace(
    /!\[([^\]]*)\]\(((?:\.\/)?assets\/[^)\s]+)\)/g,
    (full, alt: string, rel: string) => {
      const normalized = rel.replace(/^\.\//, '');
      const resolved = assetMap[normalized] || assetMap[`./${normalized}`];
      if (!resolved) return full;
      return `![${alt}](${resolved})`;
    },
  );
}

export type WriteupBlock =
  | { kind: 'anchor'; id: string }
  | { kind: 'heading'; level: 1 | 2 | 3 | 4; text: string }
  | { kind: 'code'; lang: string; code: string }
  | { kind: 'quote'; text: string }
  | { kind: 'image'; alt: string; rel: string }
  | { kind: 'list'; items: string[] }
  | { kind: 'hr' }
  | { kind: 'paragraph'; text: string };

export function parseWriteupBlocks(markdown: string): WriteupBlock[] {
  const lines = markdown.split(/\r?\n/);
  const blocks: WriteupBlock[] = [];
  let inCode = false;
  let codeLang = '';
  let codeLines: string[] = [];
  let listItems: string[] = [];

  const flushList = () => {
    if (listItems.length > 0) {
      blocks.push({ kind: 'list', items: listItems });
      listItems = [];
    }
  };

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();

    if (inCode) {
      if (trimmed.startsWith('```')) {
        blocks.push({ kind: 'code', lang: codeLang, code: codeLines.join('\n') });
        inCode = false;
        codeLang = '';
        codeLines = [];
      } else {
        codeLines.push(rawLine);
      }
      continue;
    }

    if (trimmed.startsWith('```')) {
      flushList();
      inCode = true;
      codeLang = trimmed.slice(3).trim();
      codeLines = [];
      continue;
    }

    if (!trimmed) {
      flushList();
      continue;
    }

    const anchorMatch = trimmed.match(/^<a\s+id=["']([^"']+)["']\s*><\/a>$/i);
    if (anchorMatch) {
      flushList();
      blocks.push({ kind: 'anchor', id: anchorMatch[1] });
      continue;
    }

    if (trimmed === '---' || trimmed === '***') {
      flushList();
      blocks.push({ kind: 'hr' });
      continue;
    }

    const headingMatch = trimmed.match(/^(#{1,4})\s+(.+)$/);
    if (headingMatch) {
      flushList();
      const level = headingMatch[1].length as 1 | 2 | 3 | 4;
      blocks.push({ kind: 'heading', level, text: headingMatch[2].trim() });
      continue;
    }

    const imgMatch = trimmed.match(/^!\[([^\]]*)\]\(([^)]+)\)$/);
    if (imgMatch) {
      flushList();
      blocks.push({
        kind: 'image',
        alt: imgMatch[1],
        rel: imgMatch[2].replace(/^\.\//, ''),
      });
      continue;
    }

    if (trimmed.startsWith('> ')) {
      flushList();
      blocks.push({ kind: 'quote', text: trimmed.slice(2).trim() });
      continue;
    }

    if (/^[-*]\s+/.test(trimmed)) {
      listItems.push(trimmed.replace(/^[-*]\s+/, ''));
      continue;
    }

    flushList();
    blocks.push({ kind: 'paragraph', text: trimmed });
  }

  flushList();
  if (inCode && codeLines.length > 0) {
    blocks.push({ kind: 'code', lang: codeLang, code: codeLines.join('\n') });
  }
  return blocks;
}
