import { ChevronLeft, ExternalLink, FolderOpen, PanelLeft, Plus, RefreshCw, Search, Settings } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";
import { INBOX } from "../lib/types";
import { windowDrag } from "./menu";

export default function TitleBar() {
  const setPalette = useStore((s) => s.setPalette);
  const pickImport = useStore((s) => s.pickImport);
  const showToast = useStore((s) => s.showToast);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  // When viewing a file, the title bar switches to document context: the file name goes into the window title bar (macOS document-window convention),
  // and the viewer itself no longer has a second mini title bar
  const viewer = useStore((s) => s.viewerAsset);
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
      showToast(`${t("scanDone")} · ${parts.length ? parts.join(" · ") : t("scanNoChange")}`);
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
    <header className="relative h-[52px] shrink-0 flex items-center gap-3 border-b border-line bg-paper pl-[78px] pr-4">
      {/* Full-size transparent drag layer: calls startDragging explicitly; interactive controls sit above it via z-[1]; double-click maximizes */}
      <div className="absolute inset-0" onMouseDown={windowDrag} />

      <div className="flex items-center gap-2 pointer-events-none relative">
        <div className="w-7 h-7 rounded-[9px] bg-primary text-white grid place-items-center text-[11px] font-extrabold">
          {"</>"}
        </div>
        <span className="font-extrabold text-[14px]">Harbly</span>
      </div>

      <button
        onClick={toggleSidebar}
        title={`${sidebarOpen ? t("sidebarHide") : t("sidebarShow")} (⌘B)`}
        className={`relative z-[1] w-8 h-8 grid place-items-center rounded-ctl transition ${
          sidebarOpen ? "text-sub hover:bg-side hover:text-ink" : "text-primary bg-primary/10"
        }`}
      >
        <PanelLeft className="w-4 h-4" />
      </button>

      {viewer ? (
        <>
          <button
            onClick={() => useStore.getState().closeViewer()}
            title={`${t("back")} (esc)`}
            className="relative z-[1] w-8 h-8 grid place-items-center rounded-ctl text-sub hover:bg-side hover:text-ink transition"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
          <div className="relative z-[1] ml-1 min-w-0 flex-1 max-w-[520px] flex items-baseline gap-1.5">
            <button
              onClick={backToFolder}
              className="text-xs text-sub truncate shrink-0 hover:text-primary transition"
              title={t("backToFolder")}
            >
              {viewer.folder === INBOX ? t("inbox") : viewer.folder || t("libraryRoot")} /
            </button>
            <span className="text-[13.5px] font-extrabold truncate">{viewer.fileName}</span>
          </div>

          <div className="flex-1" />

          <button
            onClick={() => api.openInBrowser(viewer.id).catch(() => {})}
            title={t("openInBrowser")}
            className="relative z-[1] w-8 h-8 grid place-items-center rounded-ctl text-sub hover:bg-side hover:text-ink transition"
          >
            <ExternalLink className="w-4 h-4" />
          </button>
          <button
            onClick={() => api.revealAsset(viewer.id).catch(() => {})}
            title={t("revealInFinder")}
            className="relative z-[1] w-8 h-8 grid place-items-center rounded-ctl text-sub hover:bg-side hover:text-ink transition"
          >
            <FolderOpen className="w-4 h-4" />
          </button>
        </>
      ) : (
        <>
          <button
            onClick={() => setPalette(true)}
            className="relative z-[1] ml-4 flex-1 max-w-[420px] h-8 flex items-center gap-2 px-3 rounded-full bg-side border border-line text-sub text-xs hover:border-primary/40 transition"
          >
            <Search className="w-3.5 h-3.5" />
            <span className="flex-1 text-left">{t("searchPlaceholder")}</span>
            <kbd className="text-[10px] bg-card border border-line rounded px-1.5 py-0.5">⌘K</kbd>
          </button>

          <div className="flex-1" />

          <button
            onClick={() => useStore.getState().setModal({ kind: "settings" })}
            title={`${t("settings")} (⌘,)`}
            className="relative z-[1] w-8 h-8 grid place-items-center rounded-ctl text-sub hover:bg-side hover:text-ink transition"
          >
            <Settings className="w-4 h-4" />
          </button>

          <button
            onClick={rescan}
            title={t("rescanLibrary")}
            className="relative z-[1] w-8 h-8 grid place-items-center rounded-ctl text-sub hover:bg-side hover:text-ink transition"
          >
            <RefreshCw className={`w-4 h-4 ${scanning ? "animate-spin text-primary" : ""}`} />
          </button>

          <button
            onClick={pickImport}
            className="relative z-[1] h-8 flex items-center gap-1.5 px-3.5 rounded-full bg-primary text-white text-xs font-bold hover:bg-primary-light transition"
          >
            <Plus className="w-3.5 h-3.5" />
            {t("importBtn")}
          </button>
        </>
      )}
    </header>
  );
}
