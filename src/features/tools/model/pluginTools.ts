import type { AnalysisRecord } from '../../../lib/ipc/analysisResults';
import type { GitDiffFile } from '../../../lib/ipc/gitWorkspace';
import type { ImageId } from '../../../lib/ipc/ids';
import type { PluginPackageInfo } from '../../../lib/ipc/plugins';
import type { FileLocationTarget } from '../../../lib/models/fileLocation';
import { pluginTextKey, type PluginTextLookup } from '../../../lib/models/pluginText';

export interface PluginToolGroup {
  toolId: string;
  pluginId: string;
  label: string;
  icon: string;
  view: string;
  itemRecord: string;
  itemKey: string;
  itemLabel: string;
  itemSublabel: string | null;
  primaryFunction: string;
  functions: string[];
}

export interface PluginToolItem {
  key: string;
  label: string;
  sublabel: string;
  recordId: string;
  runId: string;
  values: Record<string, unknown>;
}

const FALLBACK_GIT_TOOL = {
  id: 'git_workspace',
  label: 'Git 仓库',
  icon: 'GitBranch',
  view: 'git_repo',
  itemRecord: 'git.repo',
  itemKey: 'repo_path',
  itemLabel: 'repo_name',
  itemSublabel: 'repo_path',
};

/** Collect tool workspace registrations declared by installed plugins (`[analyzer.tool]`). */
export function collectPluginToolGroups(
  packages: readonly PluginPackageInfo[],
  translateText: PluginTextLookup = (_p, _k, fallback) => fallback,
): PluginToolGroup[] {
  const byToolId = new Map<string, PluginToolGroup>();

  for (const pkg of packages) {
    for (const analyzer of pkg.analyzers) {
      let tool = analyzer.tool ?? null;
      if (
        !tool &&
        pkg.id === 'org.forensicflow.collector' &&
        (analyzer.id === 'git_forensics' || analyzer.id === 'git_forensics_files')
      ) {
        tool = FALLBACK_GIT_TOOL;
      }
      if (!tool) continue;

      const fnName = `${pkg.id}/${analyzer.id}`;
      const existing = byToolId.get(tool.id);
      if (existing) {
        if (!existing.functions.includes(fnName)) {
          existing.functions.push(fnName);
        }
        if (analyzer.scope === 'evidence') {
          existing.primaryFunction = fnName;
        }
        continue;
      }

      byToolId.set(tool.id, {
        toolId: tool.id,
        pluginId: pkg.id,
        label: translateText(pkg.id, pluginTextKey.tool(tool.id), tool.label),
        icon: tool.icon || 'Wrench',
        view: tool.view || 'generic',
        itemRecord: tool.itemRecord,
        itemKey: tool.itemKey,
        itemLabel: tool.itemLabel,
        itemSublabel: tool.itemSublabel ?? null,
        primaryFunction: fnName,
        functions: [fnName],
      });
    }
  }

  return [...byToolId.values()];
}

function stringifyField(value: unknown): string {
  if (typeof value === 'string') return value.trim();
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return '';
}

/** Extract unique project items from plugin records matching a tool's `itemRecord`. */
export function extractToolItemsFromRecords(
  group: Pick<PluginToolGroup, 'itemKey' | 'itemLabel' | 'itemSublabel'>,
  records: readonly AnalysisRecord[],
): PluginToolItem[] {
  const seen = new Map<string, PluginToolItem>();
  for (const record of records) {
    const key = stringifyField(record.values[group.itemKey]);
    if (!key) continue;
    const label = stringifyField(record.values[group.itemLabel]) || key;
    const sublabel = group.itemSublabel ? stringifyField(record.values[group.itemSublabel]) : '';
    seen.set(key, {
      key,
      label,
      sublabel: sublabel && sublabel !== label ? sublabel : '',
      recordId: record.recordId,
      runId: record.runId,
      values: record.values,
    });
  }
  return [...seen.values()].sort((a, b) => a.key.localeCompare(b.key));
}

/** Convert an evidence repository path like `part:1/home/kali/wifi-deauth` to a file-list location target. */
export function parseRepoEvidenceLocation(
  imageId: ImageId,
  repoPath: string,
): FileLocationTarget | null {
  if (!imageId || !repoPath.trim()) return null;
  const clean = repoPath.trim().replace(/\\/g, '/').replace(/^\/+|\/+$/g, '');
  if (!clean) return null;
  const slashIdx = clean.indexOf('/');
  if (slashIdx === -1) {
    return { imageId, rootId: clean, path: '' };
  }
  const rootId = clean.slice(0, slashIdx);
  const path = clean.slice(slashIdx + 1);
  if (!rootId) return null;
  return { imageId, rootId, path };
}

export function shortSha(sha: string | null | undefined, length = 8): string {
  if (!sha) return '';
  return sha.slice(0, length);
}

export function formatGitTimestamp(iso: string | null | undefined): string {
  if (!iso) return '—';
  const ts = Date.parse(iso);
  if (!Number.isFinite(ts)) return iso;
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function summarizeDiffStats(files: readonly GitDiffFile[]): {
  files: number;
  additions: number;
  deletions: number;
} {
  let additions = 0;
  let deletions = 0;
  for (const f of files) {
    additions += f.additions;
    deletions += f.deletions;
  }
  return { files: files.length, additions, deletions };
}

export function isMarkdownFilename(name: string): boolean {
  const lower = name.toLowerCase();
  return lower.endsWith('.md') || lower.endsWith('.markdown') || lower.endsWith('.mdx');
}

export function inferCodeLang(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'rs': return 'Rust';
    case 'ts': case 'mts': case 'cts': return 'TypeScript';
    case 'js': case 'mjs': case 'cjs': return 'JavaScript';
    case 'vue': return 'Vue';
    case 'py': return 'Python';
    case 'go': return 'Go';
    case 'c': case 'h': return 'C';
    case 'cpp': case 'cc': case 'hpp': return 'C++';
    case 'sh': case 'bash': case 'zsh': return 'Shell';
    case 'json': return 'JSON';
    case 'toml': return 'TOML';
    case 'yaml': case 'yml': return 'YAML';
    case 'md': case 'markdown': return 'Markdown';
    case 'html': case 'htm': return 'HTML';
    case 'css': case 'scss': return 'CSS';
    case 'sql': return 'SQL';
    default: return ext ? ext.toUpperCase() : 'Text';
  }
}
