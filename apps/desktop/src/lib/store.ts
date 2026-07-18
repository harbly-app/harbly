import { create } from "zustand";
import { api } from "./api";
import { initialLang, isLang, localizeError, setCurrentLang, tr } from "./i18n";
import type { Lang } from "./i18n";
import { applyThemePref, initialThemePref } from "./theme";
import type { ThemePref } from "./theme";
import type {
  AssetMeta,
  ImportResult,
  SortKey,
  TagInfo,
  TreeNode,
} from "./types";
import {
  creationDest,
  FAVORITES,
  INBOX,
  isTagView,
  tagOfView,
  tagView,
} from "./types";

export type Modal =
  | { kind: "move"; ids: string[]; label: string; fromFolder: string | null }
  | { kind: "newFolder"; parent: string }
  | { kind: "tags"; asset: AssetMeta }
  | { kind: "confirmDeleteFolder"; rel: string; label: string }
  | { kind: "settings" }
  /** Side-by-side version compare; `fromVer` is the baseline (null hides the left pane) */
  | {
      kind: "aiDiff";
      asset: AssetMeta;
      fromVer: number | null;
      toVer: number;
    };

export interface DragPayload {
  ids: string[];
  /** Library-relative paths matching ids (used to build absolute paths for Option-drag out to the system) */
  rels: string[];
  label: string;
  fromFolder: string;
}

export interface Toast {
  text: string;
  action?: { label: string; fn: () => void };
}

/** Bridge to the mounted Markdown editor so the native menu (⌘Z/⌘⇧Z) can drive
 * ProseMirror's own history, and quit/close can flush a pending autosave. */
export interface EditorHandle {
  undo(): void;
  redo(): void;
  /** Persist any pending debounced edit immediately. */
  flush(): Promise<void>;
}

// Suppress clicks for a short window after a drag ends (prevents accidental select/open/folder-switch right after dropping)
let dragEndAt = 0;
export const dragJustEnded = () => Date.now() - dragEndAt < 250;

interface S {
  phase: "loading" | "onboarding" | "main";
  root: string | null;
  tree: TreeNode | null;
  inbox: number;
  favCount: number;
  tags: TagInfo[];
  /** Current view: "" = all assets · "_inbox" = inbox · FAVORITES = starred · TAG_PREFIX+name = tag view · anything else = folder relative path (sentinels start with "/", which no real rel can) */
  folder: string;
  sort: SortKey;
  assets: AssetMeta[];
  /** Multi-select (Finder semantics: click to select, Cmd-click to toggle, Shift-click for range, Cmd+A for all) */
  selIds: string[];
  /** Anchor for Shift range selection */
  anchorId: string | null;
  /** Cmd+Backspace may delete the current folder ONLY after an explicit sidebar folder click; any grid interaction disarms it */
  folderArmed: boolean;
  /** Asset / folder currently being renamed in place */
  editingAsset: string | null;
  editingFolder: string | null;
  viewerAsset: AssetMeta | null;
  /** Registered while a Markdown editor is mounted; null otherwise */
  editorHandle: EditorHandle | null;
  paletteOpen: boolean;
  modal: Modal | null;
  toast: Toast | null;
  thumbEpoch: Record<string, number>;
  dragOver: boolean;
  dragAsset: DragPayload | null;
  dropTarget: string | null;
  sidebarOpen: boolean;
  /** AI panel visibility inside the viewer (⌘J), persisted across sessions */
  aiOpen: boolean;
  /** Bumped when AI credentials/config change in settings, so a mounted panel re-probes supplies */
  aiConfigEpoch: number;
  /** Markdown editor width: false = comfortable reading column, true = fill the pane */
  mdWide: boolean;
  /** UI language (six locales), kept in sync with the native menu */
  lang: Lang;
  /** Appearance preference; "system" follows the OS live */
  theme: ThemePref;

  // Actions as arrow-function properties (they never use `this`, and this
  // keeps references like useStore((s) => s.setPalette) bind-safe)
  setLang: (l: Lang) => void;
  setTheme: (t: ThemePref) => void;
  toggleSidebar: () => void;
  toggleAi: () => void;
  bumpAiConfig: () => void;
  /** Open the viewer on an asset with the AI panel expanded (grid menu / palette / ⌘J on a selection) */
  openAiFor: (id: string) => void;
  toggleMdWide: () => void;
  boot: () => Promise<void>;
  enterMain: () => Promise<void>;
  refresh: () => Promise<void>;
  setFolder: (rel: string) => void;
  setSort: (s: SortKey) => void;
  setSel: (ids: string[], anchor?: string | null) => void;
  selectAll: () => void;
  openViewer: (id: string) => void;
  closeViewer: () => void;
  setEditorHandle: (h: EditorHandle | null) => void;
  newMarkdown: (folder?: string) => Promise<void>;
  newHdoc: (folder?: string) => Promise<void>;
  doExportHdoc: (id: string) => Promise<void>;
  startEditAsset: (id: string) => void;
  startEditFolder: (rel: string) => void;
  stopEdit: () => void;
  doTrash: (ids: string[]) => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  doRename: (id: string, name: string) => Promise<void>;
  doMove: (ids: string[], dest: string) => Promise<void>;
  doCreateFolder: (parent: string, name: string) => Promise<void>;
  doRenameFolder: (rel: string, name: string) => Promise<void>;
  focusFolder: (rel: string) => void;
  requestDeleteFolder: (rel: string) => Promise<void>;
  doDeleteFolder: (rel: string) => Promise<void>;
  doDuplicateFolder: (rel: string) => Promise<void>;
  doDuplicateAsset: (id: string) => Promise<void>;
  doExportAsset: (id: string) => Promise<void>;
  doExportFolder: (rel: string) => Promise<void>;
  copyFiles: (ids?: string[]) => Promise<void>;
  pasteFiles: (move: boolean) => Promise<void>;
  startDrag: (d: DragPayload) => void;
  setDropTarget: (rel: string | null) => void;
  endDrag: () => Promise<void>;
  importFiles: (paths: string[]) => Promise<void>;
  pickImport: () => Promise<void>;
  setModal: (m: Modal | null) => void;
  setPalette: (b: boolean) => void;
  showToast: (t: string | Toast) => void;
  bumpThumb: (id: string) => void;
  setDragOver: (b: boolean) => void;
}

function viewExists(
  tree: TreeNode | null,
  tags: TagInfo[],
  folder: string,
): boolean {
  if (folder === "" || folder === INBOX || folder === FAVORITES) return true;
  if (isTagView(folder)) return tags.some((t) => tagView(t.name) === folder);
  if (!tree) return false;
  const walk = (n: TreeNode): boolean =>
    n.rel === folder || n.children.some(walk);
  return tree.children.some(walk);
}

function fetchAssets(folder: string, sort: SortKey): Promise<AssetMeta[]> {
  if (folder === FAVORITES) return api.favoriteAssets();
  return isTagView(folder)
    ? api.assetsByTag(tagOfView(folder))
    : api.listAssets(folder, sort);
}

/** Destination directory for import/paste: tag and starred views land in the library root, all other views in the current folder */
function importDest(folder: string) {
  return isTagView(folder) || folder === FAVORITES ? "" : folder;
}

function importToast(get: () => S, r: ImportResult): Toast {
  const parts: string[] = [];
  if (r.added) parts.push(tr("importedN", { n: r.added }));
  if (r.duplicates) parts.push(tr("dupSkippedN", { n: r.duplicates }));
  if (r.renamed) parts.push(tr("renamedSuffixN", { n: r.renamed }));
  if (r.skipped) parts.push(tr("skippedUnsupportedN", { n: r.skipped }));
  const toast: Toast = { text: parts.join(" · ") || tr("nothingImported") };
  if (r.duplicates > 0 && r.dupOf.length > 0) {
    toast.action = {
      label: tr("viewExisting"),
      fn: () => get().openViewer(r.dupOf[0]),
    };
  }
  return toast;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

const bootLang = initialLang();
setCurrentLang(bootLang);

// The pre-paint script in index.html already set the .dark class; this re-applies it
// (harmless) and attaches the system-appearance listener + native window sync
const bootTheme = initialThemePref();
applyThemePref(bootTheme);

export const useStore = create<S>((set, get) => ({
  phase: "loading",
  root: null,
  tree: null,
  inbox: 0,
  favCount: 0,
  tags: [],
  folder: "",
  sort: "recent",
  assets: [],
  selIds: [],
  anchorId: null,
  folderArmed: false,
  editingAsset: null,
  editingFolder: null,
  viewerAsset: null,
  editorHandle: null,
  paletteOpen: false,
  modal: null,
  toast: null,
  thumbEpoch: {},
  dragOver: false,
  dragAsset: null,
  dropTarget: null,
  sidebarOpen: localStorage.getItem("harbly.sidebar") !== "0",
  aiOpen: localStorage.getItem("harbly.aiPanel") === "1",
  aiConfigEpoch: 0,
  mdWide: localStorage.getItem("harbly.mdWide") === "1",
  lang: bootLang,
  theme: bootTheme,

  setLang: (l) => {
    localStorage.setItem("harbly.lang", l);
    setCurrentLang(l);
    set({ lang: l });
    api.setLanguage(l).catch(() => {}); // Rebuild native menu + persist to config
  },

  setTheme: (t) => {
    localStorage.setItem("harbly.theme", t);
    set({ theme: t });
    applyThemePref(t);
  },

  toggleSidebar: () =>
    set((s) => {
      const v = !s.sidebarOpen;
      localStorage.setItem("harbly.sidebar", v ? "1" : "0");
      return { sidebarOpen: v };
    }),

  toggleAi: () =>
    set((s) => {
      const v = !s.aiOpen;
      localStorage.setItem("harbly.aiPanel", v ? "1" : "0");
      return { aiOpen: v };
    }),

  bumpAiConfig: () => set((s) => ({ aiConfigEpoch: s.aiConfigEpoch + 1 })),

  openAiFor: (id) => {
    localStorage.setItem("harbly.aiPanel", "1");
    set({ aiOpen: true });
    get().openViewer(id);
  },

  toggleMdWide: () =>
    set((s) => {
      const v = !s.mdWide;
      localStorage.setItem("harbly.mdWide", v ? "1" : "0");
      return { mdWide: v };
    }),

  boot: async () => {
    // Language sync: if a local preference exists, the frontend wins; otherwise (first launch or WebView storage reset) adopt the value saved in config
    if (localStorage.getItem("harbly.lang")) {
      api.setLanguage(get().lang).catch(() => {});
    } else {
      const saved = await api.getLanguage().catch(() => null);
      if (saved && isLang(saved) && saved !== get().lang) get().setLang(saved);
      else api.setLanguage(get().lang).catch(() => {});
    }
    try {
      const st = await api.libraryStatus();
      if (st.root) {
        set({ root: st.root, phase: "main" });
        await get().refresh();
        api.scanLibrary().catch(() => {});
      } else {
        set({ phase: "onboarding" });
      }
    } catch {
      set({ phase: "onboarding" });
    }
  },

  enterMain: async () => {
    const st = await api.libraryStatus();
    set({ root: st.root, phase: "main" });
    await get().refresh();
  },

  refresh: async () => {
    const { folder, sort } = get();
    const [tree, inbox, favCount, tags] = await Promise.all([
      api.dirTree(),
      api.inboxCount(),
      api.favoriteCount(),
      api.allTags(),
    ]);
    const f = viewExists(tree, tags, folder) ? folder : "";
    const assets = await fetchAssets(f, sort);
    // The user may have navigated or re-sorted while we awaited: committing
    // the captured view now would yank it back (and the navigation's own
    // guarded fetch would then discard its result — the user ends up
    // stranded). Metadata is view-independent and always safe to adopt; the
    // asset list is committed only if the folder AND sort it was fetched for
    // are still current.
    if (get().folder !== folder || get().sort !== sort) {
      set({ tree, inbox, favCount, tags });
      return;
    }
    // Drop assets that no longer exist from the selection
    const alive = new Set(assets.map((a) => a.id));
    set((s) => ({
      tree,
      inbox,
      favCount,
      tags,
      folder: f,
      assets,
      selIds: s.selIds.filter((id) => alive.has(id)),
      editingAsset:
        s.editingAsset && alive.has(s.editingAsset) ? s.editingAsset : null,
    }));
    api.requestThumbs(assets.map((a) => a.id)).catch(() => {});
    // While the viewer is open, refresh its metadata too (external edit → preview auto-updates; deleted → close)
    const va = get().viewerAsset;
    if (va) {
      try {
        set({ viewerAsset: await api.assetGet(va.id) });
      } catch {
        get().closeViewer();
      }
    }
  },

  setFolder: (rel) => {
    const sort = get().sort;
    set({
      folder: rel,
      selIds: [],
      anchorId: null,
      editingAsset: null,
      editingFolder: null,
      folderArmed: false,
    });
    fetchAssets(rel, sort)
      .then((assets) => {
        // Commit only if BOTH the view and the sort are still what this
        // request was issued for — a stale response must never clobber the
        // list a later navigation/sort change already owns.
        if (get().folder === rel && get().sort === sort) {
          set({ assets });
          api.requestThumbs(assets.map((a) => a.id)).catch(() => {});
        }
      })
      .catch(() => {});
  },

  // Sidebar folder click: navigation + "selection" — arms Cmd+Backspace folder deletion
  focusFolder: (rel) => {
    get().closeViewer();
    get().setFolder(rel);
    set({ folderArmed: true });
  },

  setSort: (sort) => {
    const folder = get().folder;
    set({ sort });
    fetchAssets(folder, sort)
      .then((assets) => {
        // Same latest-wins rule as setFolder: a stale response for an old
        // folder/sort must not overwrite the current view's list.
        if (get().folder === folder && get().sort === sort) set({ assets });
      })
      .catch(() => {});
  },

  setSel: (ids, anchor) =>
    set((s) => ({
      selIds: ids,
      anchorId: anchor !== undefined ? anchor : s.anchorId,
      folderArmed: false,
    })),

  selectAll: () => {
    const ids = get().assets.map((a) => a.id);
    set({ selIds: ids, anchorId: ids[0] ?? null, folderArmed: false });
  },

  openViewer: (id) => {
    api
      .assetGet(id)
      .then((a) => set({ viewerAsset: a, folderArmed: false }))
      .catch(() => {});
  },

  closeViewer: () => set({ viewerAsset: null }),

  setEditorHandle: (h) => set({ editorHandle: h }),

  // New Markdown lands in `folder` when given (folder context menu), else the
  // current folder — tag/inbox views fall back to the library root, mirroring the
  // New Folder rule. It then opens straight in the editor.
  newMarkdown: async (folder) => {
    const dest = folder ?? creationDest(get().folder);
    try {
      const a = await api.newMarkdown(dest);
      get().setFolder(a.folder);
      get().openViewer(a.id);
    } catch (e) {
      get().showToast(String(e));
    }
  },

  // New page (.hdoc): same destination rule as New Markdown, opens in the editor
  newHdoc: async (folder) => {
    const dest = folder ?? creationDest(get().folder);
    try {
      const a = await api.newHdoc(dest);
      get().setFolder(a.folder);
      get().openViewer(a.id);
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doExportHdoc: async (id) => {
    try {
      const dest = await api.exportHdocHtml(id);
      if (dest) get().showToast(tr("exportedTo", { dest }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  startEditAsset: (id) => set({ editingAsset: id, editingFolder: null }),
  startEditFolder: (rel) => set({ editingFolder: rel, editingAsset: null }),
  stopEdit: () => set({ editingAsset: null, editingFolder: null }),

  // Real deletion: goes straight to the system Trash (counts stay consistent immediately); Cmd+Z moves it back from the Trash along the same path (same mechanism as Finder)
  doTrash: async (ids) => {
    if (!ids.length) return;
    const st = get();
    if (st.viewerAsset && ids.includes(st.viewerAsset.id)) st.closeViewer();
    // Finder behavior: after trashing, select the item that followed the deleted ones,
    // so repeated Cmd+Backspace walks through files (and never falls through to the folder)
    const gone = new Set(ids);
    const firstIdx = st.assets.findIndex((a) => gone.has(a.id));
    set((s) => ({
      modal: null,
      folderArmed: false,
      selIds: s.selIds.filter((x) => !gone.has(x)),
    }));
    try {
      const r = await api.assetsTrash(ids);
      await get().refresh();
      if (firstIdx >= 0 && !get().selIds.length && !get().viewerAsset) {
        const after = get().assets;
        const next = after.at(Math.min(firstIdx, after.length - 1));
        if (next) set({ selIds: [next.id], anchorId: next.id });
      }
      const text =
        r.count === 1 ? tr("trashedOne") : tr("trashedN", { n: r.count });
      get().showToast(
        r.undoable
          ? {
              text,
              action: { label: tr("undoAction"), fn: () => void get().undo() },
            }
          : text,
      );
    } catch (e) {
      get().showToast(String(e));
    }
  },

  undo: async () => {
    try {
      const r = await api.undoOp();
      if (!r) get().showToast(tr("nothingToUndo"));
      else if (r.count === 0)
        get().showToast(tr("cannotUndo", { label: r.label }));
      else get().showToast(tr("undone", { label: r.label }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  redo: async () => {
    try {
      const r = await api.redoOp();
      if (!r) get().showToast(tr("nothingToRedo"));
      else if (r.count === 0)
        get().showToast(tr("cannotRedo", { label: r.label }));
      else get().showToast(tr("redone", { label: r.label }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doRename: async (id, name) => {
    set({ editingAsset: null });
    try {
      await api.rename(id, name);
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doMove: async (ids, dest) => {
    try {
      const n = await api.assetsMove(ids, dest);
      set({ modal: null });
      if (n > 0) {
        const where =
          dest === "" ? tr("libraryRoot") : dest === INBOX ? tr("inbox") : dest;
        get().showToast({
          text:
            n === 1
              ? tr("movedTo", { dest: where })
              : tr("movedNTo", { n, dest: where }),
          action: { label: tr("undoAction"), fn: () => void get().undo() },
        });
      }
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doCreateFolder: async (parent, name) => {
    try {
      const rel = await api.createFolder(parent, name);
      set({ modal: null });
      get().setFolder(rel);
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doRenameFolder: async (rel, name) => {
    set({ editingFolder: null });
    try {
      const newRel = await api.folderRename(rel, name);
      if (get().folder === rel) set({ folder: newRel });
    } catch (e) {
      get().showToast(String(e));
    }
  },

  // Deletion entry point (sidebar context menu + Cmd+Backspace on the highlighted folder):
  // empty folders trash immediately; non-empty ones confirm first, Enter = fast confirm
  requestDeleteFolder: async (rel) => {
    if (!rel || rel === INBOX || rel === FAVORITES || isTagView(rel)) return;
    const label = rel.split("/").pop() ?? rel;
    let hasContent = true; // if the probe fails, err on the side of asking
    try {
      hasContent = await api.folderHasContent(rel);
    } catch {
      // keep hasContent = true
    }
    if (!hasContent) return get().doDeleteFolder(rel);
    set({ modal: { kind: "confirmDeleteFolder", rel, label } });
  },

  // The actual delete — always undoable via Cmd+Z (the folder lands in the Trash whole)
  doDeleteFolder: async (rel) => {
    try {
      const undoable = await api.folderDelete(rel);
      set({ folderArmed: false });
      if (get().folder.startsWith(rel)) get().setFolder("");
      get().showToast(
        undoable
          ? {
              text: tr("folderTrashed"),
              action: { label: tr("undoAction"), fn: () => void get().undo() },
            }
          : tr("folderTrashed"),
      );
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doDuplicateFolder: async (rel) => {
    try {
      await api.folderDuplicate(rel);
      get().showToast(tr("folderDuplicated"));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doDuplicateAsset: async (id) => {
    try {
      const a = await api.assetDuplicate(id);
      get().showToast(tr("duplicatedAs", { name: a.fileName }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doExportAsset: async (id) => {
    try {
      const dest = await api.exportAsset(id);
      if (dest) get().showToast(tr("exportedTo", { dest }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  doExportFolder: async (rel) => {
    try {
      const dest = await api.exportFolder(rel);
      if (dest) get().showToast(tr("exportedZipTo", { dest }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  // Cmd+C: write file URLs to the system pasteboard (can Cmd+V directly in Finder); copies the current file when the viewer is open
  copyFiles: async (ids) => {
    const st = get();
    const list = ids?.length
      ? ids
      : st.selIds.length
        ? st.selIds
        : st.viewerAsset
          ? [st.viewerAsset.id]
          : [];
    if (!list.length) return;
    try {
      const n = await api.pasteboardCopy(list);
      get().showToast(
        n === 1 ? tr("copiedPasteboard") : tr("copiedPasteboardN", { n }),
      );
    } catch (e) {
      get().showToast(String(e));
    }
  },

  // Cmd+V copy / Option+Cmd+V move: paste files from the pasteboard into the current folder
  pasteFiles: async (move) => {
    const st = get();
    if (st.phase !== "main") return;
    try {
      const r = await api.pasteboardPaste(importDest(st.folder), move);
      if (r.count === 0) return;
      const text =
        r.moved > 0 && r.copied > 0
          ? tr("movedAndCopied", { m: r.moved, c: r.copied })
          : r.moved > 0
            ? tr("movedN", { n: r.moved })
            : tr("pastedN", { n: r.copied });
      get().showToast({
        text,
        action: { label: tr("undoAction"), fn: () => void get().undo() },
      });
    } catch (e) {
      const msg = String(e);
      // The pasteboard often has no files (maybe just text) — stay quiet in that case
      if (!msg.includes("剪贴板中没有文件")) get().showToast(msg);
    }
  },

  startDrag: (d) => set({ dragAsset: d, dropTarget: null }),
  setDropTarget: (rel) => set({ dropTarget: rel }),

  endDrag: async () => {
    const { dragAsset, dropTarget } = get();
    if (!dragAsset) return;
    set({ dragAsset: null, dropTarget: null });
    dragEndAt = Date.now();
    if (dropTarget == null || dropTarget === dragAsset.fromFolder) return;
    await get().doMove(dragAsset.ids, dropTarget);
  },

  importFiles: async (paths) => {
    if (!paths.length) return;
    try {
      const r = await api.importPaths(paths, importDest(get().folder));
      get().showToast(importToast(get, r));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  pickImport: async () => {
    try {
      const r = await api.pickAndImport(importDest(get().folder));
      if (r.added || r.duplicates || r.renamed || r.skipped)
        get().showToast(importToast(get, r));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  setModal: (m) => set({ modal: m }),
  setPalette: (b) => set({ paletteOpen: b }),

  showToast: (t) => {
    // Plain strings usually come from backend errors — known error strings are exact-mapped to the current language
    const toast = typeof t === "string" ? { text: localizeError(t) } : t;
    set({ toast });
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => set({ toast: null }), 5000);
  },

  bumpThumb: (id) =>
    set((s) => ({
      thumbEpoch: { ...s.thumbEpoch, [id]: (s.thumbEpoch[id] || 0) + 1 },
    })),

  setDragOver: (b) => set({ dragOver: b }),
}));
