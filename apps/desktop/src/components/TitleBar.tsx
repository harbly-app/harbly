import {
  ChevronLeft,
  ExternalLink,
  FileDown,
  FolderOpen,
  MoveHorizontal,
  PanelLeft,
  Plus,
  RefreshCw,
  Search,
  Settings,
  Sparkles,
  SquarePen,
} from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import { INBOX, isHdoc, isMd } from "../lib/types";
import { windowDrag } from "../lib/drag";

export default function TitleBar() {
  const setPalette = useStore((s) => s.setPalette);
  const pickImport = useStore((s) => s.pickImport);
  const showToast = useStore((s) => s.showToast);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  // When viewing a file, the title bar switches to document context: the file name goes into the window title bar (macOS document-window convention),
  // and the viewer itself no longer has a second mini title bar
  const viewer = useStore((s) => s.viewerAsset);
  const mdWide = useStore((s) => s.mdWide);
  const toggleMdWide = useStore((s) => s.toggleMdWide);
  const aiOpen = useStore((s) => s.aiOpen);
  const toggleAi = useStore((s) => s.toggleAi);
  const doExportHdoc = useStore((s) => s.doExportHdoc);
  const t = makeT(useStore((s) => s.lang));
  const [scanning, setScanning] = useState(false);

  const rescan = async () => {
    if (scanning) return;
    setScanning(true);
    try {
      const s = await api.rescan();
      const parts: string[] = [];
      if (s.added) parts.push(t("scanAdded", { n: s.added }));
      if (s.updated) parts.push(t("scanUpdated", { n: s.updated }));
      if (s.moved) parts.push(t("scanMoved", { n: s.moved }));
      if (s.removed) parts.push(t("scanRemoved", { n: s.removed }));
      showToast(
        `${t("scanDone")} · ${parts.length ? parts.join(" · ") : t("scanNoChange")}`,
      );
    } catch (e) {
      showToast(String(e));
    } finally {
      setScanning(false);
    }
  };

  const backToFolder = () => {
    if (!viewer) return;
    const st = useStore.getState();
    st.closeViewer();
    st.setFolder(viewer.folder);
  };

  return (
    <header className="relative flex h-[52px] shrink-0 items-center gap-3 border-b border-line bg-paper pr-4 pl-[78px]">
      {/* Full-size transparent drag layer: calls startDragging explicitly; interactive controls sit above it via z-[1]; double-click maximizes */}
      <div className="absolute inset-0" onMouseDown={windowDrag} />

      <div className="pointer-events-none relative flex items-center gap-2">
        <div className="grid h-7 w-7 place-items-center rounded-[9px] bg-primary text-[11px] font-extrabold text-white">
          {"</>"}
        </div>
        <span className="text-[14px] font-extrabold">Harbly</span>
      </div>

      <button
        onClick={toggleSidebar}
        title={`${sidebarOpen ? t("sidebarHide") : t("sidebarShow")} (⌘B)`}
        className={`relative z-[1] grid h-8 w-8 place-items-center rounded-ctl transition ${
          sidebarOpen
            ? "text-sub hover:bg-side hover:text-ink"
            : "bg-primary/10 text-primary"
        }`}
      >
        <PanelLeft className="h-4 w-4" />
      </button>

      {viewer ? (
        <>
          <button
            onClick={() => useStore.getState().closeViewer()}
            title={`${t("back")} (esc)`}
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <ChevronLeft className="h-4 w-4" />
          </button>
          <div className="relative z-[1] ml-1 flex max-w-[520px] min-w-0 flex-1 items-baseline gap-1.5">
            <button
              onClick={backToFolder}
              className="shrink-0 truncate text-xs text-sub transition hover:text-primary"
              title={t("backToFolder")}
            >
              {viewer.folder === INBOX
                ? t("inbox")
                : viewer.folder || t("libraryRoot")}{" "}
              /
            </button>
            <span className="truncate text-[13.5px] font-extrabold">
              {viewer.fileName}
            </span>
          </div>

          <div className="flex-1" />

          {(isMd(viewer.fileName) || isHdoc(viewer.fileName)) && (
            <button
              onClick={toggleMdWide}
              title={t("mdEditorWidth")}
              className={`relative z-[1] grid h-8 w-8 place-items-center rounded-ctl transition ${
                mdWide
                  ? "bg-primary/10 text-primary"
                  : "text-sub hover:bg-side hover:text-ink"
              }`}
            >
              <MoveHorizontal className="h-4 w-4" />
            </button>
          )}

          <button
            onClick={toggleAi}
            title={`${aiOpen ? t("aiPanelHide") : t("aiPanelShow")} (⌘J)`}
            className={`relative z-[1] grid h-8 w-8 place-items-center rounded-ctl transition ${
              aiOpen
                ? "bg-primary/10 text-primary"
                : "text-sub hover:bg-side hover:text-ink"
            }`}
          >
            <Sparkles className="h-4 w-4" />
          </button>

          {isHdoc(viewer.fileName) && (
            <button
              onClick={() => doExportHdoc(viewer.id)}
              title={t("exportHdocCmd")}
              className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
            >
              <FileDown className="h-4 w-4" />
            </button>
          )}

          <button
            onClick={() =>
              isHdoc(viewer.fileName)
                ? api.previewHdoc(viewer.id).catch(() => {})
                : api.openInBrowser(viewer.id).catch(() => {})
            }
            title={
              isHdoc(viewer.fileName)
                ? t("previewInBrowser")
                : isMd(viewer.fileName)
                  ? t("openWithDefaultApp")
                  : t("openInBrowser")
            }
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <ExternalLink className="h-4 w-4" />
          </button>
          <button
            onClick={() => api.revealAsset(viewer.id).catch(() => {})}
            title={t("revealInFinder")}
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <FolderOpen className="h-4 w-4" />
          </button>
        </>
      ) : (
        <>
          <button
            onClick={() => setPalette(true)}
            className="relative z-[1] ml-4 flex h-8 max-w-[420px] flex-1 items-center gap-2 rounded-full border border-line bg-side px-3 text-xs text-sub transition hover:border-primary/40"
          >
            <Search className="h-3.5 w-3.5" />
            <span className="flex-1 text-left">{t("searchPlaceholder")}</span>
            <kbd className="rounded border border-line bg-card px-1.5 py-0.5 text-[10px]">
              ⌘K
            </kbd>
          </button>

          <div className="flex-1" />

          <button
            onClick={() => useStore.getState().setModal({ kind: "settings" })}
            title={`${t("settings")} (⌘,)`}
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <Settings className="h-4 w-4" />
          </button>

          <button
            onClick={rescan}
            title={t("rescanLibrary")}
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <RefreshCw
              className={`h-4 w-4 ${scanning ? "animate-spin text-primary" : ""}`}
            />
          </button>

          <button
            onClick={() => useStore.getState().newMarkdown()}
            title={`${t("newMarkdownCmd")} (⌘N)`}
            className="relative z-[1] grid h-8 w-8 place-items-center rounded-ctl text-sub transition hover:bg-side hover:text-ink"
          >
            <SquarePen className="h-4 w-4" />
          </button>

          <button
            onClick={pickImport}
            className="relative z-[1] flex h-8 items-center gap-1.5 rounded-full bg-primary px-3.5 text-xs font-bold text-white transition hover:bg-primary-light"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("importBtn")}
          </button>
        </>
      )}
    </header>
  );
}
