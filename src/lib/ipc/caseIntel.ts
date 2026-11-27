import { invokeCmd } from './client';

export interface CaseIocItem {
  kind: string;
  value: string;
  count: number;
  crossImageHit: boolean;
  images: string[];
  sources: string[];
}

export interface CaseIocsReport {
  totalIocs: number;
  crossImageCount: number;
  byKind: Record<string, number>;
  items: CaseIocItem[];
}

export interface CaseWordlistReport {
  baseCount: number;
  totalWithVariants: number;
  exportedPath: string | null;
  words: string[];
  sources: string[];
}

export interface CrackRequest {
  targetType: 'ntlm' | 'md5' | 'sha1' | 'sha256' | 'sha512' | 'fernet' | 'metamask' | 'zip' | string;
  target: string;
  wordlistPath?: string | null;
  candidates?: string[];
  enableRules?: boolean | null;
  pinLength?: number | null;
}

export interface CrackResult {
  targetType: string;
  found: boolean;
  password: string | null;
  decryptedPreview: string | null;
  testedCandidates: number;
  elapsedMs: number;
}

export function caseIocsList(args?: {
  kind?: string | null;
  query?: string | null;
}): Promise<CaseIocsReport> {
  return invokeCmd<CaseIocsReport>('case_iocs_list', {
    kind: args?.kind ?? null,
    query: args?.query ?? null,
  });
}

export function caseWordlistGet(args?: {
  destination?: string | null;
}): Promise<CaseWordlistReport> {
  return invokeCmd<CaseWordlistReport>('case_wordlist_get', {
    destination: args?.destination ?? null,
  });
}

export function crackerStart(request: CrackRequest): Promise<CrackResult> {
  return invokeCmd<CrackResult>('cracker_start', { request });
}
