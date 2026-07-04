export interface AssetMeta {
  id: string;
  relPath: string;
  fileName: string;
  folder: string;
  title: string;
  source: string;
  sizeBytes: number;
  currentHash: string;
  verCount: number;
  createdAt: number;
  updatedAt: number;
  tags: string[];
}

export interface TagInfo {
  name: string;
  count: number;
}

export interface TreeNode {
  name: string;
  rel: string;
  count: number;
  children: TreeNode[];
  files: TreeFile[];
}

export interface TreeFile {
  id: string;
  name: string;
  verCount: number;
}

export interface SearchHit {
  asset: AssetMeta;
  snippet: string;
}

export interface ScanProgress {
  found: number;
  indexed: number;
}

export interface ScanSummary {
  total: number;
  added: number;
  updated: number;
  moved: number;
  removed: number;
  tagsSynced: number;
}

export interface ImportResult {
  added: number;
  duplicates: number;
  renamed: number;
  skipped: number;
  dupOf: string[];
  /** Relative paths actually written to disk */
  imported: string[];
}

export type SortKey = "recent" | "name" | "modified";

export const INBOX = "_inbox";
