import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "./lib/api";
import { makeT } from "./lib/i18n";
import { useStore } from "./lib/store";
import { INBOX } from "./lib/types";
import Onboarding from "./components/Onboarding";
import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import AssetGrid from "./components/AssetGrid";
import Viewer from "./components/Viewer";
import CommandPalette from "./components/CommandPalette";
import Modals from "./components/Modals";
import DragGhost from "./components/DragGhost";

export default function App() {
  const phase = useStore((s) => s.phase);
  const viewerOpen = useStore((s) => s.viewerAsset !== null);
  const dragOver = useStore((s) => s.dragOver);
  const toast = useStore((s) => s.toast);
  const t = makeT(useStore((s) => s.lang));

  useEffect(() => {
    useStore.getState().boot();
  }, []);

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    let alive = true;
    const keep = (u: () => void) => {
      if (alive) unsubs.push(u);
      else u();
    };

    listen("library-changed", () => {
      const st = useStore.getState();
      if (st.phase === "main") st.refresh();
    }).then(keep);

    listen<{ assetId: string }>("thumb-updated", (e) => {
      useStore.getState().bumpThumb(e.payload.assetId);
    }).then(keep);

    // When an input/textarea/viewer iframe is focused, edit shortcuts should act on text, not files
    const editableFocused = () => {
      const el = document.activeElement as HTMLElement | null;
      return (
        !!el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "IFRAME" ||
          el.isContentEditable)
      );
    };
    const hasTextSelection = () => {
      const sel = window.getSelection();
      return !!sel && !sel.isCollapsed;
    };

    // Native menu bar actions
    listen<string>("menu-action", (e) => {
      const st = useStore.getState();
      if (st.phase !== "main" && e.payload !== "settings") return;
      switch (e.payload) {
        case "import":
          st.pickImport();
          break;
        case "new-folder":
          st.setModal({
            kind: "newFolder",
            parent: st.folder.startsWith("#") || st.folder === INBOX ? "" : st.folder,
          });
          break;
        case "reveal-library":
          api.revealFolder("").catch(() => {});
          break;
        case "settings":
          if (st.phase === "main") st.setModal({ kind: "settings" });
          break;
        case "toggle-sidebar":
          st.toggleSidebar();
          break;
        case "search":
          st.setPalette(true);
          break;
        case "rescan":
          api.rescan().catch(() => {});
          break;
        case "undo":
          // Input focused = text undo; otherwise undo the last file operation (Finder-style Cmd+Z)
          if (editableFocused()) document.execCommand("undo");
          else st.undo();
          break;
        case "redo":
          if (editableFocused()) document.execCommand("redo");
          else st.redo();
          break;
        case "copy":
          // Text selected / editing → forward system copy: (text copy works as usual); otherwise copy selected files
          if (editableFocused() || hasTextSelection()) api.forwardEdit("copy").catch(() => {});
          else st.copyFiles();
          break;
        case "paste":
          if (editableFocused()) api.forwardEdit("paste").catch(() => {});
          else st.pasteFiles(false);
          break;
        case "paste-move":
          if (!editableFocused()) st.pasteFiles(true);
          break;
        case "select-all":
          if (editableFocused()) api.forwardEdit("selectAll").catch(() => {});
          else st.selectAll();
          break;
        case "trash": {
          // Cmd+Backspace in an input = delete to line start (system semantics); otherwise trash selection, or the current file when the viewer is open
          if (editableFocused()) {
            api.forwardEdit("deleteToLineStart").catch(() => {});
            break;
          }
          const ids = st.selIds.length
            ? st.selIds
            : st.viewerAsset
              ? [st.viewerAsset.id]
              : [];
          if (ids.length) st.doTrash(ids);
          // Folder deletion only when armed by an explicit sidebar folder click — an empty
          // selection after deleting files must NOT fall through to the folder
          // (root / inbox / tag views are additionally rejected inside requestDeleteFolder)
          else if (st.folderArmed) st.requestDeleteFolder(st.folder);
          break;
        }
      }
    }).then(keep);

    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const st = useStore.getState();
          if (st.phase !== "main") return;
          const p = event.payload;
          if (p.type === "enter" || p.type === "over") st.setDragOver(true);
          else if (p.type === "drop") {
            st.setDragOver(false);
            st.importFiles(p.paths);
          } else st.setDragOver(false);
        })
        .then(keep);
    } catch {
      // Outside Tauri (plain-browser dev) the webview handle throws synchronously; file drop is simply unavailable there
    }

    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        useStore.getState().setPalette(true);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        useStore.getState().toggleSidebar();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        const st = useStore.getState();
        if (st.phase === "main") st.setModal({ kind: "settings" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      alive = false;
      unsubs.forEach((u) => u());
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  if (phase === "loading") {
    return (
      <div className="h-screen grid place-items-center text-sub" data-tauri-drag-region>
        {t("booting")}
      </div>
    );
  }
  if (phase === "onboarding") return <Onboarding />;

  return (
    <div className="h-screen flex flex-col overflow-hidden">
      <TitleBar />
      <div className="flex-1 flex min-h-0">
        <Sidebar />
        {viewerOpen ? <Viewer /> : <AssetGrid />}
      </div>
      <CommandPalette />
      <Modals />
      <DragGhost />
      {dragOver && (
        <div className="fixed inset-0 z-50 pointer-events-none bg-primary/5 border-4 border-primary/70 rounded-xl grid place-items-center">
          <div className="bg-primary text-white text-sm font-semibold px-5 py-2.5 rounded-full shadow-lg">
            {t("dropToImport")}
          </div>
        </div>
      )}
      {toast && (
        // ink/paper swap so the pill stays high-contrast in both themes (dark theme = light pill)
        <div className="fixed bottom-12 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 bg-ink text-paper text-xs px-4 py-2.5 rounded-full shadow-lg">
          <span>{toast.text}</span>
          {toast.action && (
            <button
              onClick={toast.action.fn}
              className="font-bold text-primary-light hover:opacity-75 transition shrink-0"
            >
              {toast.action.label}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
