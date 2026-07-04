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

export interface VersionInfo {
  ver: number;
  hash: string;
  /** Canonical (Chinese) label written by the core; localized for display */
  label: string;
  sizeBytes: number;
  createdAt: number;
}

/** AI supply ids: local agent CLIs + BYOK providers */
export type AiSupply =
  "claude" | "codex" | "anthropic" | "openai" | "openrouter";
export type ByokProvider = "anthropic" | "openai" | "openrouter";
export const BYOK_PROVIDERS: ByokProvider[] = [
  "anthropic",
  "openai",
  "openrouter",
];

export interface AgentInfo {
  kind: "claude" | "codex";
  path: string;
  version: string | null;
}

/** Non-secret AI preferences persisted in the app config (keys live in the keychain) */
export interface AiConfig {
  supply?: AiSupply;
  models?: Partial<Record<ByokProvider, string>>;
}

export type AiEvent =
  { type: "delta"; text: string } | { type: "action"; label: string };

/** "" = provider default */
export type AiEffort = "" | "low" | "medium" | "high";

export interface AiSession {
  id: string;
  /** Auto-filled from the first user message when empty */
  title: string;
  supply: AiSupply;
  model: string;
  effort: AiEffort;
  agentSessionId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface AiMessage {
  id: string;
  sessionId: string;
  role: "user" | "assistant";
  content: string;
  /** Tool-activity labels shown above the assistant text */
  actions: string[];
  createdAt: number;
}

export interface AiRun {
  id: string;
  assetId: string;
  /** Outcome-derived: "revise" (produced a version) | "reply" (textual answer) */
  kind: string;
  supply: AiSupply;
  model: string;
  instruction: string;
  status: "ok" | "error" | "cancelled";
  ver: number | null;
  report: string | null;
  error: string | null;
  sessionId: string | null;
  messageId: string | null;
  createdAt: number;
}

export const INBOX = "_inbox";

/** A Markdown asset — opened in the editor rather than the preview iframe. */
export const isMd = (name: string) => /\.(md|markdown)$/i.test(name);

/** Display name with the managed extension stripped (HTML or Markdown). */
export const stemName = (name: string) =>
  name.replace(/\.(html?|md|markdown)$/i, "");
