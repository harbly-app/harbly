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
  /** Starred by the user; mirrors the on-file com.harbly.favorite xattr */
  favorite: boolean;
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
  favorite: boolean;
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

/** The full effort spectrum; each supply accepts a subset (see the panel's
 * EFFORT_CHOICES). "" only appears on legacy sessions created before efforts
 * became mandatory — the backend treats it as "send nothing". */
export type AiEffort =
  "" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

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

// Virtual views are addressed by sentinel strings STARTING WITH "/" — a
// library-relative path can never start with a slash, so no real directory
// (created in-app or in Finder, whatever its name) can ever collide with a
// view. A folder literally named "#work" or "::favorites" stays a plain
// folder. (INBOX above is different: it IS a real directory.)

/** Virtual folder id for the starred view. */
export const FAVORITES = "/::favorites";

/** Prefix addressing a tag view; the tag name follows it verbatim. */
export const TAG_PREFIX = "/#";

/** The view id for a tag. */
export const tagView = (name: string) => TAG_PREFIX + name;

export const isTagView = (folder: string) => folder.startsWith(TAG_PREFIX);

/** The tag name a tag-view id addresses. */
export const tagOfView = (folder: string) => folder.slice(TAG_PREFIX.length);

/** Where creation actions (new folder / note / page) land for the current
 * view: virtual views and the inbox create in the library root; any real
 * folder creates inside itself. */
export const creationDest = (folder: string) =>
  isTagView(folder) || folder === FAVORITES || folder === INBOX ? "" : folder;

/** A Markdown asset — opened in the editor rather than the preview iframe. */
export const isMd = (name: string) => /\.(md|markdown)$/i.test(name);

/** A page document (.hdoc) — opened in the block editor. */
export const isHdoc = (name: string) => /\.hdoc$/i.test(name);

/** Display name with the managed extension stripped. */
export const stemName = (name: string) =>
  name.replace(/\.(html?|md|markdown|hdoc)$/i, "");
