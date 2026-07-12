import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentInfo,
  AiConfig,
  AiEvent,
  AiMessage,
  AiRun,
  AiSession,
  AssetMeta,
  ImportResult,
  ScanSummary,
  SearchHit,
  SortKey,
  TagInfo,
  TreeNode,
  VersionInfo,
} from "./types";

// Commands whose Rust side returns `()` resolve to `null` in the webview,
// hence invoke<null> rather than invoke<void>.
export const api = {
  libraryStatus: () => invoke<{ root: string | null }>("library_status"),
  defaultLibraryPath: () => invoke<string>("default_library_path"),
  pickFolder: () => invoke<string | null>("pick_folder"),
  libraryInit: (path: string) => invoke<string>("library_init", { path }),
  scanLibrary: () => invoke<ScanSummary>("scan_library"),
  rescan: () => invoke<ScanSummary>("rescan"),
  dirTree: () => invoke<TreeNode>("dir_tree"),
  listAssets: (folder: string, sort: SortKey) =>
    invoke<AssetMeta[]>("list_assets", { folder, sort }),
  assetGet: (id: string) => invoke<AssetMeta>("asset_get", { id }),
  inboxCount: () => invoke<number>("inbox_count"),
  // Markdown editing
  assetReadText: (id: string) => invoke<string>("asset_read_text", { id }),
  assetWrite: (id: string, content: string) =>
    invoke<AssetMeta>("asset_write", { id, content }),
  assetCheckpoint: (id: string, baseHash: string) =>
    invoke<boolean>("asset_checkpoint", { id, baseHash }),
  newMarkdown: (folder: string, name?: string) =>
    invoke<AssetMeta>("asset_new_markdown", { folder, name: name ?? null }),
  newHdoc: (folder: string, name?: string) =>
    invoke<AssetMeta>("asset_new_hdoc", { folder, name: name ?? null }),
  /** Bake an hdoc into a standalone HTML file via a save dialog. */
  exportHdocHtml: (id: string) =>
    invoke<string | null>("export_hdoc_html", { id }),
  importPaths: (paths: string[], dest: string) =>
    invoke<ImportResult>("import_paths", { paths, dest }),
  pickAndImport: (dest: string) =>
    invoke<ImportResult>("pick_and_import", { dest }),
  search: (q: string) => invoke<SearchHit[]>("search_assets", { q }),
  rename: (id: string, newName: string) =>
    invoke<AssetMeta>("asset_rename", { id, newName }),
  assetsMove: (ids: string[], dest: string) =>
    invoke<number>("assets_move", { ids, dest }),
  assetsTrash: (ids: string[]) =>
    invoke<{ count: number; undoable: boolean }>("assets_trash", { ids }),
  undoOp: () => invoke<{ label: string; count: number } | null>("undo_op"),
  redoOp: () => invoke<{ label: string; count: number } | null>("redo_op"),
  pasteboardCopy: (ids: string[]) => invoke<number>("pasteboard_copy", { ids }),
  pasteboardPaste: (dest: string, moveItems: boolean) =>
    invoke<{ count: number; moved: number; copied: number }>(
      "pasteboard_paste",
      {
        dest,
        moveItems,
      },
    ),
  forwardEdit: (
    action: "copy" | "paste" | "cut" | "selectAll" | "deleteToLineStart",
  ) => invoke<null>("forward_edit_action", { action }),
  setLanguage: (lang: string) => invoke<null>("set_language", { lang }),
  getLanguage: () => invoke<string>("get_language"),
  revealAsset: (id: string) => invoke<null>("reveal_asset", { id }),
  openInBrowser: (id: string) => invoke<null>("open_in_browser", { id }),
  /** Bake an hdoc to a temp HTML file and open it in the system browser. */
  previewHdoc: (id: string) => invoke<null>("preview_hdoc", { id }),
  /** Read one image off the system clipboard as a PNG data: URL (or null). */
  readClipboardImage: () => invoke<string | null>("read_clipboard_image"),
  openUrl: (url: string) => invoke<null>("open_url", { url }),
  revealFolder: (rel: string) => invoke<null>("reveal_folder", { rel }),
  createFolder: (parent: string, name: string) =>
    invoke<string>("create_folder", { parent, name }),
  folderRename: (rel: string, newName: string) =>
    invoke<string>("folder_rename", { rel, newName }),
  folderDelete: (rel: string) => invoke<boolean>("folder_delete", { rel }),
  folderHasContent: (rel: string) =>
    invoke<boolean>("folder_has_content", { rel }),
  folderDuplicate: (rel: string) => invoke<string>("folder_duplicate", { rel }),
  assetDuplicate: (id: string) => invoke<AssetMeta>("asset_duplicate", { id }),
  setTags: (id: string, tags: string[]) =>
    invoke<null>("set_tags", { id, tags }),
  allTags: () => invoke<TagInfo[]>("all_tags"),
  assetsByTag: (tag: string) => invoke<AssetMeta[]>("assets_by_tag", { tag }),
  allowOnce: (id: string) => invoke<string>("asset_allow_once", { id }),
  exportAsset: (id: string) => invoke<string | null>("export_asset", { id }),
  exportFolder: (rel: string) =>
    invoke<string | null>("export_folder", { rel }),
  thumbsRebuild: () => invoke<null>("thumbs_rebuild"),
  requestThumbs: (ids: string[]) => invoke<null>("request_thumbs", { ids }),
  listVersions: (id: string) => invoke<VersionInfo[]>("list_versions", { id }),
  restoreVersion: (id: string, ver: number) =>
    invoke<null>("restore_version", { id, ver }),
  // AI
  aiDetectAgents: () => invoke<AgentInfo[]>("ai_detect_agents"),
  aiKeyStatus: () => invoke<Record<string, boolean>>("ai_key_status"),
  aiSetKey: (provider: string, key: string) =>
    invoke<null>("ai_set_key", { provider, key }),
  aiGetConfig: () => invoke<AiConfig>("ai_get_config"),
  aiSetConfig: (config: AiConfig) => invoke<null>("ai_set_config", { config }),
  aiRunsList: (id: string, limit?: number) =>
    invoke<AiRun[]>("ai_runs_list", { id, limit: limit ?? null }),
  aiCancel: (job: string) => invoke<null>("ai_cancel", { job }),
  // Sessions
  aiSessionsList: () => invoke<AiSession[]>("ai_sessions_list"),
  aiSessionCreate: (supply: string, model: string, effort: string) =>
    invoke<AiSession>("ai_session_create", { supply, model, effort }),
  aiSessionDelete: (id: string) => invoke<null>("ai_session_delete", { id }),
  /** Undo the most recent session deletion; resolves with the restored id. */
  aiSessionRestore: () => invoke<string | null>("ai_session_restore"),
  aiSessionSetPrefs: (
    id: string,
    supply: string,
    model: string,
    effort: string,
  ) => invoke<null>("ai_session_set_prefs", { id, supply, model, effort }),
  aiSessionMessages: (id: string) =>
    invoke<AiMessage[]>("ai_session_messages", { id }),
  /** One conversation turn: resolves with the assistant message when the turn
   * finishes; progress (text deltas + tool actions) streams via `onEvent`. */
  aiSend: (
    args: {
      job: string;
      sessionId: string;
      text: string;
      currentAssetId?: string | null;
    },
    onEvent: (e: AiEvent) => void,
  ) => {
    const ch = new Channel<AiEvent>();
    ch.onmessage = onEvent;
    return invoke<AiMessage>("ai_send", {
      ...args,
      currentAssetId: args.currentAssetId ?? null,
      onEvent: ch,
    });
  },
};

export const assetUrl = (id: string) =>
  `harbly-asset://localhost/current/${encodeURIComponent(id)}`;

/** URL for a file referenced (relatively) by a Markdown asset — resolved by the
 * protocol handler against the asset's own folder. Each path segment is encoded
 * to match the handler's per-segment percent-decoding. */
export const relAssetUrl = (id: string, rel: string) =>
  `harbly-asset://localhost/rel/${encodeURIComponent(id)}/${rel
    .split("/")
    .map(encodeURIComponent)
    .join("/")}`;

export const thumbUrl = (hash: string) =>
  `harbly-thumb://localhost/${hash}.jpg`;

/** Sandboxed URL of a historical version snapshot (same CSP as `assetUrl`) */
export const versionUrl = (id: string, ver: number) =>
  `harbly-asset://localhost/version/${encodeURIComponent(id)}/${ver}`;

/** Relative time in the UI language (the app's six locales are all valid
 * BCP-47 tags, so they feed Intl directly). */
export function timeAgo(epochSec: number, lang = "zh-CN"): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - epochSec);
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  if (s < 60) return rtf.format(0, "second");
  const m = Math.floor(s / 60);
  if (m < 60) return rtf.format(-m, "minute");
  const h = Math.floor(m / 60);
  if (h < 24) return rtf.format(-h, "hour");
  const d = Math.floor(h / 24);
  if (d < 7) return rtf.format(-d, "day");
  const w = Math.floor(d / 7);
  if (w < 5) return rtf.format(-w, "week");
  return new Intl.DateTimeFormat(lang, {
    year: "numeric",
    month: "numeric",
    day: "numeric",
  }).format(new Date(epochSec * 1000));
}

export function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
