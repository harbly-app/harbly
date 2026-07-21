import { lazy, Suspense, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./lib/api";
import { makeT } from "./lib/i18n";
import { useStore } from "./lib/store";
import { creationDest, isHdoc, isMd } from "./lib/types";
import Onboarding from "./components/Onboarding";
import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import AssetGrid from "./components/AssetGrid";
import Viewer from "./components/Viewer";
import CommandPalette from "./components/CommandPalette";
import Modals from "./components/Modals";
import DragGhost from "./components/DragGhost";

// Lazy: sessions, transcripts and supply probing only load when the panel opens
const AiPanel = lazy(() => import("./components/AiPanel"));

export default function App() {
  const phase = useStore((s) => s.phase);
  const viewerOpen = useStore((s) => s.viewerAsset !== null);
  const aiOpen = useStore((s) => s.aiOpen);
  const dragOver = useStore((s) => s.dragOver);
  const toast = useStore((s) => s.toast);
  const t = makeT(useStore((s) => s.lang));

  useEffect(() => {
    void useStore.getState().boot();
  }, []);

  useEffect(() => {
    const unsubs: (() => void)[] = [];
    let alive = true;
    const keep = (u: () => void) => {
      if (alive) unsubs.push(u);
      else u();
    };

    void listen("library-changed", () => {
      const st = useStore.getState();
      if (st.phase === "main") void st.refresh();
    }).then(keep);

    void listen<{ assetId: string }>("thumb-updated", (e) => {
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
    // Focus is inside the Markdown or page editor: ⌘Z/⌘⇧Z must drive
    // ProseMirror's own history (execCommand has no effect on it), so route to
    // the editor handle
    const editorFocused = () =>
      !!(document.activeElement as HTMLElement | null)?.closest(
        ".milkdown, .hdoc-editor",
      );
    const hasTextSelection = () => {
      const sel = window.getSelection();
      return !!sel && !sel.isCollapsed;
    };

    // Native menu bar actions
    void listen<string>("menu-action", (e) => {
      const st = useStore.getState();
      if (st.phase !== "main" && e.payload !== "settings") return;
      switch (e.payload) {
        case "import":
          void st.pickImport();
          break;
        case "new-md":
          void st.newMarkdown();
          break;
        case "new-hdoc":
          void st.newHdoc();
          break;
        case "new-folder":
          st.setModal({
            kind: "newFolder",
            parent: creationDest(st.folder),
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
          // Markdown editor → ProseMirror history; plain input → text undo;
          // otherwise undo the last file operation (Finder-style Cmd+Z)
          if (editorFocused() && st.editorHandle) st.editorHandle.undo();
          // eslint-disable-next-line @typescript-eslint/no-deprecated -- no modern API triggers text-field undo programmatically
          else if (editableFocused()) document.execCommand("undo");
          else void st.undo();
          break;
        case "redo":
          if (editorFocused() && st.editorHandle) st.editorHandle.redo();
          // eslint-disable-next-line @typescript-eslint/no-deprecated -- no modern API triggers text-field redo programmatically
          else if (editableFocused()) document.execCommand("redo");
          else void st.redo();
          break;
        case "copy":
          // Text selected / editing → forward system copy: (text copy works as usual); otherwise copy selected files
          if (editableFocused() || hasTextSelection())
            api.forwardEdit("copy").catch(() => {});
          else void st.copyFiles();
          break;
        case "paste":
          // Editors own paste at the DOM paste event (the forwarded native
          // paste: dispatches one); the menu only routes text vs. files here.
          if (editableFocused()) api.forwardEdit("paste").catch(() => {});
          else void st.pasteFiles(false);
          break;
        case "paste-move":
          if (!editableFocused()) void st.pasteFiles(true);
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
          if (ids.length) void st.doTrash(ids);
          // Folder deletion only when armed by an explicit sidebar folder click — an empty
          // selection after deleting files must NOT fall through to the folder
          // (root / inbox / tag views are additionally rejected inside requestDeleteFolder)
          else if (st.folderArmed) void st.requestDeleteFolder(st.folder);
          break;
        }
      }
    }).then(keep);

    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          const st = useStore.getState();
          if (st.phase !== "main") return;
          // Inside the Markdown/page editor, dragging a block handle is an
          // internal reorder that Tauri's OS drag layer also reports; don't
          // hijack it with the file-import overlay (an internal drag carries no
          // file paths anyway).
          if (
            st.viewerAsset &&
            (isMd(st.viewerAsset.fileName) || isHdoc(st.viewerAsset.fileName))
          )
            return;
          const p = event.payload;
          if (p.type === "enter" || p.type === "over") st.setDragOver(true);
          else if (p.type === "drop") {
            st.setDragOver(false);
            void st.importFiles(p.paths);
          } else st.setDragOver(false);
        })
        .then(keep);
    } catch {
      // Outside Tauri (plain-browser dev) the webview handle throws synchronously; file drop is simply unavailable there
    }

    // Flush a pending Markdown autosave before the window closes (the 1s debounce
    // could otherwise drop the last edit on quit)
    try {
      void getCurrentWindow()
        .onCloseRequested(async (e) => {
          const h = useStore.getState().editorHandle;
          if (!h) return; // no editor mounted → close normally
          e.preventDefault();
          try {
            await h.flush();
          } catch {
            // best effort — never block quitting on a save error
          }
          void getCurrentWindow().destroy();
        })
        .then(keep);
    } catch {
      // Plain-browser dev: no window handle
    }

    // App shortcuts forwarded from inside the sandboxed preview iframe: it is
    // cross-origin, so its keydowns never bubble to this window — the injected
    // reporter script relays ⌘J/⌘K/⌘B plus Escape/arrows via postMessage.
    const onIframeKey = (e: MessageEvent) => {
      const d = e.data as { __harbly?: string; key?: string } | null;
      if (d?.__harbly !== "key") return;
      const st = useStore.getState();
      if (st.phase !== "main") return;
      if (d.key === "j") st.toggleAi();
      else if (d.key === "k") st.setPalette(true);
      else if (d.key === "b") st.toggleSidebar();
      else if (d.key === "escape") {
        // Mirror the in-app Escape ladder: overlays first, then the viewer
        if (st.modal) st.setModal(null);
        else if (st.paletteOpen) st.setPalette(false);
        else st.closeViewer();
      } else if (d.key === "arrowup" || d.key === "arrowdown") {
        if (!st.modal && !st.paletteOpen)
          st.viewerStep(d.key === "arrowdown" ? 1 : -1);
      }
    };
    window.addEventListener("message", onIframeKey);

    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        useStore.getState().setPalette(true);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        useStore.getState().toggleSidebar();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "j") {
        // AI panel is library-scoped: toggle anywhere
        e.preventDefault();
        const st = useStore.getState();
        if (st.phase === "main") st.toggleAi();
      } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        const st = useStore.getState();
        if (st.phase === "main") st.setModal({ kind: "settings" });
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        // Autosave already covers persistence; ⌘S is the reassurance flush —
        // the title-bar indicator flips to "saved" when it lands.
        e.preventDefault();
        void useStore.getState().editorHandle?.flush();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      alive = false;
      unsubs.forEach((u) => u());
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("message", onIframeKey);
    };
  }, []);

  if (phase === "loading") {
    return (
      <div
        className="grid h-screen place-items-center text-sub"
        data-tauri-drag-region
      >
        {t("booting")}
      </div>
    );
  }
  if (phase === "onboarding") return <Onboarding />;

  return (
    <div className="flex h-screen flex-col overflow-hidden">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        {/* `relative` anchors the AI panel's narrow-window overlay mode */}
        <div className="relative flex min-w-0 flex-1">
          {viewerOpen ? <Viewer /> : <AssetGrid />}
          {aiOpen && (
            <Suspense
              fallback={
                <div className="ai-panel shrink-0" aria-hidden="true" />
              }
            >
              <AiPanel />
            </Suspense>
          )}
        </div>
      </div>
      <CommandPalette />
      <Modals />
      <DragGhost />
      {dragOver && (
        <div className="pointer-events-none fixed inset-0 z-50 grid place-items-center rounded-xl border-4 border-primary/70 bg-primary/5">
          <div className="rounded-full bg-primary px-5 py-2.5 text-sm font-semibold text-white shadow-lg">
            {t("dropToImport")}
          </div>
        </div>
      )}
      {toast && (
        // ink/paper swap so the pill stays high-contrast in both themes (dark theme = light pill)
        <div className="fixed bottom-12 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full bg-ink px-4 py-2.5 text-xs text-paper shadow-lg">
          <span>{toast.text}</span>
          {toast.action && (
            <button
              onClick={toast.action.fn}
              className="shrink-0 font-bold text-primary-light transition hover:opacity-75"
            >
              {toast.action.label}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
