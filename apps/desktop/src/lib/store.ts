import { create } from "zustand";
import { api } from "./api";
import { initialLang, localizeError, setCurrentLang, tr } from "./i18n";
import type { Lang } from "./i18n";
import type { AssetMeta, ImportResult, SortKey, TagInfo, TreeNode } from "./types";
import { INBOX } from "./types";

export type Modal =
  | { kind: "move"; ids: string[]; label: string; fromFolder: string | null }
  | { kind: "newFolder"; parent: string }
  | { kind: "tags"; asset: AssetMeta }
  | { kind: "settings" };

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

// Suppress clicks for a short window after a drag ends (prevents accidental select/open/folder-switch right after dropping)
let dragEndAt = 0;
export const dragJustEnded = () => Date.now() - dragEndAt < 250;

interface S {
  phase: "loading" | "onboarding" | "main";
  root: string | null;
  tree: TreeNode | null;
  inbox: number;
  tags: TagInfo[];
  /** Current view: "" = all assets · "_inbox" = inbox · "#xx" = tag view · anything else = folder relative path */
  folder: string;
  sort: SortKey;
  assets: AssetMeta[];
  /** Multi-select (Finder semantics: click to select, Cmd-click to toggle, Shift-click for range, Cmd+A for all) */
  selIds: string[];
  /** Anchor for Shift range selection */
  anchorId: string | null;
  /** Asset / folder currently being renamed in place */
  editingAsset: string | null;
  editingFolder: string | null;
  viewerAsset: AssetMeta | null;
  paletteOpen: boolean;
  modal: Modal | null;
  toast: Toast | null;
  thumbEpoch: Record<string, number>;
  dragOver: boolean;
  dragAsset: DragPayload | null;
  dropTarget: string | null;
  sidebarOpen: boolean;
  /** UI language (six locales), kept in sync with the native menu */
  lang: Lang;

  setLang(l: Lang): void;
  toggleSidebar(): void;
  boot(): Promise<void>;
  enterMain(): Promise<void>;
  refresh(): Promise<void>;
  setFolder(rel: string): void;
  setSort(s: SortKey): void;
  setSel(ids: string[], anchor?: string | null): void;
  selectAll(): void;
  openViewer(id: string): void;
  closeViewer(): void;
  startEditAsset(id: string): void;
  startEditFolder(rel: string): void;
  stopEdit(): void;
  doTrash(ids: string[]): Promise<void>;
  undo(): Promise<void>;
  redo(): Promise<void>;
  doRename(id: string, name: string): Promise<void>;
  doMove(ids: string[], dest: string): Promise<void>;
  doCreateFolder(parent: string, name: string): Promise<void>;
  doRenameFolder(rel: string, name: string): Promise<void>;
  doDeleteFolder(rel: string): Promise<void>;
  doDuplicateFolder(rel: string): Promise<void>;
  doDuplicateAsset(id: string): Promise<void>;
  doExportAsset(id: string): Promise<void>;
  doExportFolder(rel: string): Promise<void>;
  copyFiles(ids?: string[]): Promise<void>;
  pasteFiles(move: boolean): Promise<void>;
  startDrag(d: DragPayload): void;
  setDropTarget(rel: string | null): void;
  endDrag(): Promise<void>;
  importFiles(paths: string[]): Promise<void>;
  pickImport(): Promise<void>;
  setModal(m: Modal | null): void;
  setPalette(b: boolean): void;
  showToast(t: string | Toast): void;
  bumpThumb(id: string): void;
  setDragOver(b: boolean): void;
}

function isTagView(folder: string) {
  return folder.startsWith("#");
}

function viewExists(tree: TreeNode | null, tags: TagInfo[], folder: string): boolean {
  if (folder === "" || folder === INBOX) return true;
  if (isTagView(folder)) return tags.some((t) => `#${t.name}` === folder);
  if (!tree) return false;
  const walk = (n: TreeNode): boolean => n.rel === folder || n.children.some(walk);
  return tree.children.some(walk);
}

function fetchAssets(folder: string, sort: SortKey): Promise<AssetMeta[]> {
  return isTagView(folder) ? api.assetsByTag(folder.slice(1)) : api.listAssets(folder, sort);
}

/** Destination directory for import/paste: tag views land in the library root, all other views in the current folder */
function importDest(folder: string) {
  return isTagView(folder) ? "" : folder;
}

function importToast(get: () => S, r: ImportResult): Toast {
  const parts: string[] = [];
  if (r.added) parts.push(tr("importedN", { n: r.added }));
  if (r.duplicates) parts.push(tr("dupSkippedN", { n: r.duplicates }));
  if (r.renamed) parts.push(tr("renamedSuffixN", { n: r.renamed }));
  if (r.skipped) parts.push(tr("skippedNonHtmlN", { n: r.skipped }));
  const toast: Toast = { text: parts.join(" · ") || tr("nothingImported") };
  if (r.duplicates > 0 && r.dupOf.length > 0) {
    toast.action = { label: tr("viewExisting"), fn: () => get().openViewer(r.dupOf[0]) };
  }
  return toast;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

const bootLang = initialLang();
setCurrentLang(bootLang);

export const useStore = create<S>((set, get) => ({
  phase: "loading",
  root: null,
  tree: null,
  inbox: 0,
  tags: [],
  folder: "",
  sort: "recent",
  assets: [],
  selIds: [],
  anchorId: null,
  editingAsset: null,
  editingFolder: null,
  viewerAsset: null,
  paletteOpen: false,
  modal: null,
  toast: null,
  thumbEpoch: {},
  dragOver: false,
  dragAsset: null,
  dropTarget: null,
  sidebarOpen: localStorage.getItem("harbly.sidebar") !== "0",
  lang: bootLang,

  setLang: (l) => {
    localStorage.setItem("harbly.lang", l);
    setCurrentLang(l);
    set({ lang: l });
    api.setLanguage(l).catch(() => {}); // Rebuild native menu + persist to config
  },

  toggleSidebar: () =>
    set((s) => {
      const v = !s.sidebarOpen;
      localStorage.setItem("harbly.sidebar", v ? "1" : "0");
      return { sidebarOpen: v };
    }),

  boot: async () => {
    // Language sync: if a local preference exists, the frontend wins; otherwise (first launch or WebView storage reset) adopt the value saved in config
    if (localStorage.getItem("harbly.lang")) {
      api.setLanguage(get().lang).catch(() => {});
    } else {
      const saved = await api.getLanguage().catch(() => null);
      if (saved && saved !== get().lang) get().setLang(saved as Lang);
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
    const [tree, inbox, tags] = await Promise.all([api.dirTree(), api.inboxCount(), api.allTags()]);
    const f = viewExists(tree, tags, folder) ? folder : "";
    const assets = await fetchAssets(f, sort);
    // Drop assets that no longer exist from the selection
    const alive = new Set(assets.map((a) => a.id));
    set((s) => ({
      tree,
      inbox,
      tags,
      folder: f,
      assets,
      selIds: s.selIds.filter((id) => alive.has(id)),
      editingAsset: s.editingAsset && alive.has(s.editingAsset) ? s.editingAsset : null,
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
    set({ folder: rel, selIds: [], anchorId: null, editingAsset: null, editingFolder: null });
    fetchAssets(rel, get().sort)
      .then((assets) => {
        if (get().folder === rel) {
          set({ assets });
          api.requestThumbs(assets.map((a) => a.id)).catch(() => {});
        }
      })
      .catch(() => {});
  },

  setSort: (sort) => {
    set({ sort });
    fetchAssets(get().folder, sort)
      .then((assets) => set({ assets }))
      .catch(() => {});
  },

  setSel: (ids, anchor) =>
    set((s) => ({ selIds: ids, anchorId: anchor !== undefined ? anchor : s.anchorId })),

  selectAll: () => {
    const ids = get().assets.map((a) => a.id);
    set({ selIds: ids, anchorId: ids[0] ?? null });
  },

  openViewer: (id) => {
    api
      .assetGet(id)
      .then((a) => set({ viewerAsset: a }))
      .catch(() => {});
  },

  closeViewer: () => set({ viewerAsset: null }),

  startEditAsset: (id) => set({ editingAsset: id, editingFolder: null }),
  startEditFolder: (rel) => set({ editingFolder: rel, editingAsset: null }),
  stopEdit: () => set({ editingAsset: null, editingFolder: null }),

  // Real deletion: goes straight to the system Trash (counts stay consistent immediately); Cmd+Z moves it back from the Trash along the same path (same mechanism as Finder)
  doTrash: async (ids) => {
    if (!ids.length) return;
    const st = get();
    if (st.viewerAsset && ids.includes(st.viewerAsset.id)) st.closeViewer();
    set((s) => ({ modal: null, selIds: s.selIds.filter((x) => !ids.includes(x)) }));
    try {
      const r = await api.assetsTrash(ids);
      const text = r.count === 1 ? tr("trashedOne") : tr("trashedN", { n: r.count });
      get().showToast(
        r.undoable ? { text, action: { label: tr("undoAction"), fn: () => get().undo() } } : text
      );
    } catch (e) {
      get().showToast(String(e));
    }
  },

  undo: async () => {
    try {
      const r = await api.undoOp();
      if (!r) get().showToast(tr("nothingToUndo"));
      else if (r.count === 0) get().showToast(tr("cannotUndo", { label: r.label }));
      else get().showToast(tr("undone", { label: r.label }));
    } catch (e) {
      get().showToast(String(e));
    }
  },

  redo: async () => {
    try {
      const r = await api.redoOp();
      if (!r) get().showToast(tr("nothingToRedo"));
      else if (r.count === 0) get().showToast(tr("cannotRedo", { label: r.label }));
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
        const where = dest === "" ? tr("libraryRoot") : dest === INBOX ? tr("inbox") : dest;
        get().showToast({
          text: n === 1 ? tr("movedTo", { dest: where }) : tr("movedNTo", { n, dest: where }),
          action: { label: tr("undoAction"), fn: () => get().undo() },
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

  // Undoable via Cmd+Z at any time, so no confirmation dialog needed (Finder never confirms moving to Trash)
  doDeleteFolder: async (rel) => {
    try {
      const undoable = await api.folderDelete(rel);
      if (get().folder.startsWith(rel)) get().setFolder("");
      get().showToast(
        undoable
          ? { text: tr("folderTrashed"), action: { label: tr("undoAction"), fn: () => get().undo() } }
          : tr("folderTrashed")
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
    const list = ids?.length ? ids : st.selIds.length ? st.selIds : st.viewerAsset ? [st.viewerAsset.id] : [];
    if (!list.length) return;
    try {
      const n = await api.pasteboardCopy(list);
      get().showToast(n === 1 ? tr("copiedPasteboard") : tr("copiedPasteboardN", { n }));
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
      get().showToast({ text, action: { label: tr("undoAction"), fn: () => get().undo() } });
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
      if (r.added || r.duplicates || r.renamed || r.skipped) get().showToast(importToast(get, r));
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
    set((s) => ({ thumbEpoch: { ...s.thumbEpoch, [id]: (s.thumbEpoch[id] || 0) + 1 } })),

  setDragOver: (b) => set({ dragOver: b }),
}));
