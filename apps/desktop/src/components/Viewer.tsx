import { ShieldCheck, ShieldOff } from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";
import { api, assetUrl } from "../lib/api";
import { makeT } from "../lib/i18n";
import type { TFn } from "../lib/i18n";
import { useStore } from "../lib/store";
import { isMd } from "../lib/types";
import type { AssetMeta } from "../lib/types";

// Lazy: the Milkdown editor (and its Vue/CodeMirror runtime) only loads once a
// Markdown file is actually opened, keeping HTML-only sessions lean.
const MarkdownEditor = lazy(() => import("./MarkdownEditor"));

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
        st.closeViewer();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const list = st.assets;
        const i = list.findIndex((x) => x.id === va.id);
        if (i < 0) return;
        const ni = e.key === "ArrowDown" ? i + 1 : i - 1;
        if (ni < 0 || ni >= list.length) return;
        st.openViewer(list[ni].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!a) return null;
  // Markdown opens in the WYSIWYG editor; HTML in the sandboxed preview.
  // (The AI panel lives one level up, in App — it is library-scoped now.)
  if (isMd(a.fileName)) {
    return (
      <Suspense
        fallback={<div className="flex-1 bg-paper" aria-hidden="true" />}
      >
        <MarkdownEditor key={a.id} asset={a} />
      </Suspense>
    );
  }
  // Keyed by file: switching files remounts and thereby restores the sandbox
  return <ViewerBody key={a.id} a={a} t={t} dragging={dragging} />;
}

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

  const allowOnce = () => {
    api
      .allowOnce(a.id)
      .then(setAllowToken)
      .catch((e: unknown) => useStore.getState().showToast(String(e)));
  };

  return (
    <main className="flex min-w-0 flex-1 flex-col bg-paper">
      {/* Document canvas stays literal white in both themes: assets are standalone pages that assume a white default background (Quick Look behaves the same) */}
      <div className="relative flex-1 bg-white">
        {/* Keyed per content session (file version × sandbox state) so the CSP block counter starts from zero each time */}
        <PreviewPane
          key={`${a.currentHash}:${allowToken ?? "sandboxed"}`}
          a={a}
          t={t}
          dragging={dragging}
          allowToken={allowToken}
          onAllowOnce={allowOnce}
          onRestoreSandbox={() => setAllowToken(null)}
        />
      </div>
    </main>
  );
}

function PreviewPane({
  a,
  t,
  dragging,
  allowToken,
  onAllowOnce,
  onRestoreSandbox,
}: {
  a: AssetMeta;
  t: TFn;
  dragging: boolean;
  allowToken: string | null;
  onAllowOnce: () => void;
  onRestoreSandbox: () => void;
}) {
  // CSP block count reported by the script injected into the sandbox
  const [blocked, setBlocked] = useState(0);

  useEffect(() => {
    const on = (e: MessageEvent) => {
      const d = e.data as { __harbly?: string; count?: number } | null;
      if (d?.__harbly === "csp") setBlocked(d.count ?? 0);
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, []);

  return (
    <>
      <iframe
        src={`${assetUrl(a.id)}${allowToken ? `?allow=${allowToken}` : ""}`}
        sandbox="allow-scripts allow-same-origin"
        className={`absolute inset-0 h-full w-full border-0 ${dragging ? "pointer-events-none" : ""}`}
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
