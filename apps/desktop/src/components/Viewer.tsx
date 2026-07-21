import { ShieldCheck, ShieldOff } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { api, assetUrl } from "../lib/api";
import { makeT } from "../lib/i18n";
import type { TFn } from "../lib/i18n";
import { useStore } from "../lib/store";
import { isHdoc, isMd } from "../lib/types";
import type { AssetMeta } from "../lib/types";

// Lazy: the Milkdown editor (and its Vue/CodeMirror runtime) only loads once a
// Markdown file is actually opened, keeping HTML-only sessions lean.
const MarkdownEditor = lazy(() => import("./MarkdownEditor"));
// Lazy for the same reason: ProseMirror only loads when a page is opened.
const HdocEditor = lazy(() => import("./HdocEditor"));

/// Viewer embedded in the content area: file name and actions live in the window title bar (TitleBar),
/// so this is just the preview itself — no second mini title bar
export default function Viewer() {
  const a = useStore((s) => s.viewerAsset);
  const t = makeT(useStore((s) => s.lang));
  // While a drag is in progress, keep the iframe from eating mouse events so the ghost does not freeze over the preview area
  const dragging = useStore((s) => !!s.dragAsset);

  // Keyboard: Esc returns to the grid · Up/Down switches between files in the same folder
  // (lives here, above the per-file remounts, so it is registered once)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // An editor plugin already consumed this key (e.g. Esc closing the
      // slash menu) — don't also blur / close the viewer on the same press.
      if (e.defaultPrevented) return;
      const st = useStore.getState();
      const va = st.viewerAsset;
      if (st.paletteOpen || st.modal || !va) return;
      const el = e.target as HTMLElement | null;
      // Inside a text field or the Markdown editor: let it own typing and
      // navigation. First Esc leaves the editor; a second one closes the viewer.
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.isContentEditable)
      ) {
        if (e.key === "Escape" && el.isContentEditable) {
          e.preventDefault();
          el.blur();
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        // Ladder: an open find bar absorbs the first Escape
        if (st.findOpen) st.closeFind();
        else st.closeViewer();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        st.viewerStep(e.key === "ArrowDown" ? 1 : -1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!a) return null;
  // Markdown and pages open in their WYSIWYG editors; HTML in the sandboxed
  // preview. (The AI panel lives one level up, in App — it is library-scoped.)
  if (isMd(a.fileName)) {
    return (
      <Suspense
        fallback={<div className="flex-1 bg-paper" aria-hidden="true" />}
      >
        <MarkdownEditor key={a.id} asset={a} />
      </Suspense>
    );
  }
  if (isHdoc(a.fileName)) {
    return (
      <Suspense
        fallback={<div className="flex-1 bg-paper" aria-hidden="true" />}
      >
        <HdocEditor key={a.id} asset={a} />
      </Suspense>
    );
  }
  // Keyed by file: switching files remounts and thereby restores the sandbox
  return <ViewerBody key={a.id} a={a} t={t} dragging={dragging} />;
}

const ZOOM_MIN = 0.5;
const ZOOM_MAX = 2;

function ViewerBody({
  a,
  t,
  dragging,
}: {
  a: AssetMeta;
  t: TFn;
  dragging: boolean;
}) {
  // One-time allow token: only valid for this viewing session (component lifetime)
  const [allowToken, setAllowToken] = useState<string | null>(null);
  // Reading zoom, per viewing session (archived pages are often authored for
  // wide desktop layouts). Lives here, above PreviewPane's sandbox remounts.
  const [zoom, setZoom] = useState(1);

  const bumpZoom = (dir: 1 | -1) =>
    setZoom((z) =>
      Math.min(
        ZOOM_MAX,
        Math.max(ZOOM_MIN, Math.round((z + dir * 0.1) * 10) / 10),
      ),
    );

  // ⌘+ / ⌘− / ⌘0 (browser-standard zoom keys), scoped to the raw HTML viewer.
  // The same keys arrive via postMessage when focus sits inside the sandboxed
  // iframe — its keydowns never bubble to this window (see protocol.rs).
  useEffect(() => {
    const apply = (key: string): boolean => {
      if (key === "=" || key === "+") bumpZoom(1);
      else if (key === "-") bumpZoom(-1);
      else if (key === "0") setZoom(1);
      else return false;
      return true;
    };
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const el = e.target as HTMLElement | null;
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.isContentEditable)
      )
        return;
      if (apply(e.key)) e.preventDefault();
    };
    const onMsg = (e: MessageEvent) => {
      const d = e.data as { __harbly?: string; key?: string } | null;
      if (d?.__harbly === "key" && d.key) apply(d.key);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("message", onMsg);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("message", onMsg);
    };
  }, []);

  const allowOnce = () => {
    api
      .allowOnce(a.id)
      .then(setAllowToken)
      .catch((e: unknown) => useStore.getState().showToast(String(e)));
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-paper">
      {/* Document canvas stays literal white in both themes: assets are standalone pages that assume a white default background (Quick Look behaves the same) */}
      <div className="relative flex-1 overflow-hidden bg-white">
        {/* Keyed per content session (file version × sandbox state) so the CSP block counter starts from zero each time */}
        <PreviewPane
          key={`${a.currentHash}:${allowToken ?? "sandboxed"}`}
          a={a}
          t={t}
          dragging={dragging}
          allowToken={allowToken}
          zoom={zoom}
          onAllowOnce={allowOnce}
          onRestoreSandbox={() => setAllowToken(null)}
        />
        <div className="absolute bottom-3 left-3 flex items-center overflow-hidden rounded-full border border-line bg-card/90 text-[11px] text-sub shadow-sm backdrop-blur">
          <button
            onClick={() => bumpZoom(-1)}
            title={`${t("zoomOut")} (⌘−)`}
            className="px-2 py-1.5 transition hover:bg-side hover:text-ink"
          >
            −
          </button>
          <button
            onClick={() => setZoom(1)}
            title={`${t("zoomReset")} (⌘0)`}
            className="min-w-[44px] px-1 py-1.5 text-center tabular-nums transition hover:bg-side hover:text-ink"
          >
            {Math.round(zoom * 100)}%
          </button>
          <button
            onClick={() => bumpZoom(1)}
            title={`${t("zoomIn")} (⌘+)`}
            className="px-2 py-1.5 transition hover:bg-side hover:text-ink"
          >
            +
          </button>
        </div>
      </div>
    </main>
  );
}

function PreviewPane({
  a,
  t,
  dragging,
  allowToken,
  zoom,
  onAllowOnce,
  onRestoreSandbox,
}: {
  a: AssetMeta;
  t: TFn;
  dragging: boolean;
  allowToken: string | null;
  zoom: number;
  onAllowOnce: () => void;
  onRestoreSandbox: () => void;
}) {
  // CSP block count reported by the script injected into the sandbox
  const [blocked, setBlocked] = useState(0);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    const on = (e: MessageEvent) => {
      const d = e.data as { __harbly?: string; count?: number } | null;
      if (d?.__harbly === "csp") setBlocked(d.count ?? 0);
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, []);

  // In-page find: proxy the FindBar's commands into the sandboxed document
  // (the injected reporter script runs them and posts the counts back).
  useEffect(() => {
    interface FindReply {
      count: number;
      active: number;
    }
    // FIFO waiters: the injected runtime answers strictly in request order,
    // so the oldest waiter owns the next reply. (A single slot would mispair
    // replies whenever a search and a step overlap within the timeout.)
    const waiters: ((r: FindReply) => void)[] = [];
    const onMsg = (e: MessageEvent) => {
      const d = e.data as {
        __harbly?: string;
        count?: number;
        active?: number;
      } | null;
      if (d?.__harbly !== "findResult") return;
      waiters.shift()?.({ count: d.count ?? 0, active: d.active ?? 0 });
    };
    window.addEventListener("message", onMsg);
    const send = (msg: Record<string, unknown>) =>
      iframeRef.current?.contentWindow?.postMessage(
        { __harbly: "find", ...msg },
        "*",
      );
    const ask = (msg: Record<string, unknown>): Promise<FindReply> =>
      new Promise((resolve) => {
        waiters.push(resolve);
        send(msg);
        // The page may be mid-navigation or predate the runtime: fall back to
        // "no matches" instead of hanging the bar.
        setTimeout(() => {
          const i = waiters.indexOf(resolve);
          if (i !== -1) {
            waiters.splice(i, 1);
            resolve({ count: 0, active: 0 });
          }
        }, 600);
      });
    useStore.getState().setFindHandle({
      search: (q) => ask({ op: "search", q }),
      step: (d) => ask({ op: "step", delta: d }),
      clear: () => send({ op: "clear" }),
    });
    return () => {
      window.removeEventListener("message", onMsg);
      useStore.getState().setFindHandle(null);
    };
  }, []);

  return (
    <>
      <iframe
        ref={iframeRef}
        src={`${assetUrl(a.id)}${allowToken ? `?allow=${allowToken}` : ""}`}
        sandbox="allow-scripts allow-same-origin"
        className={`absolute inset-0 border-0 ${dragging ? "pointer-events-none" : ""}`}
        // Scale-with-compensation: the iframe is cross-origin, so zoom cannot
        // be applied inside the document; scaling the element (with inverse
        // width/height so the layout viewport matches) reads the same.
        style={{
          transform: zoom === 1 ? undefined : `scale(${zoom})`,
          transformOrigin: "0 0",
          width: `${100 / zoom}%`,
          height: `${100 / zoom}%`,
        }}
        title={a.title}
      />
      {allowToken ? (
        <button
          onClick={onRestoreSandbox}
          className="absolute right-3 bottom-3 flex items-center gap-1.5 rounded-full bg-warn px-2.5 py-1.5 text-[10.5px] font-bold text-white transition hover:opacity-90"
        >
          <ShieldOff className="h-3 w-3" />
          {t("allowedTemp")}
        </button>
      ) : (
        blocked > 0 && (
          <button
            onClick={onAllowOnce}
            className="absolute right-3 bottom-3 flex items-center gap-1.5 rounded-full bg-ink/85 px-2.5 py-1.5 text-[10.5px] text-paper transition hover:bg-ink"
          >
            <ShieldCheck className="h-3 w-3 text-ok" />
            {t("blockedN", { n: blocked })}
          </button>
        )
      )}
    </>
  );
}
