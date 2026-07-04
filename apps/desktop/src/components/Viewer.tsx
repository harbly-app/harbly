import { ShieldCheck, ShieldOff } from "lucide-react";
import { useEffect, useState } from "react";
import { api, assetUrl } from "../lib/api";
import { makeT } from "../lib/i18n";
import { useStore } from "../lib/store";

/// Viewer embedded in the content area: file name and actions live in the window title bar (TitleBar),
/// so this is just the preview itself — no second mini title bar
export default function Viewer() {
  const a = useStore((s) => s.viewerAsset);
  const t = makeT(useStore((s) => s.lang));
  // While a drag is in progress, keep the iframe from eating mouse events so the ghost does not freeze over the preview area
  const dragging = useStore((s) => !!s.dragAsset);
  const [blocked, setBlocked] = useState(0);
  // One-time allow token: only valid for this viewing session; switching files restores the sandbox
  const [allowToken, setAllowToken] = useState<string | null>(null);

  useEffect(() => setAllowToken(null), [a?.id]);

  // CSP block count reported by the script injected into the sandbox
  useEffect(() => {
    setBlocked(0);
    const on = (e: MessageEvent) => {
      const d = e.data as { __harbly?: string; count?: number };
      if (d && d.__harbly === "csp") setBlocked(d.count ?? 0);
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, [a?.id, a?.currentHash, allowToken]);

  const allowOnce = () => {
    if (!a) return;
    api
      .allowOnce(a.id)
      .then(setAllowToken)
      .catch((e) => useStore.getState().showToast(String(e)));
  };

  // Keyboard: Esc returns to the grid · Up/Down switches between files in the same folder
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      if (st.paletteOpen || st.modal || !st.viewerAsset) return;
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      if (e.key === "Escape") {
        e.preventDefault();
        st.closeViewer();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const list = st.assets;
        const i = list.findIndex((x) => x.id === st.viewerAsset!.id);
        if (i < 0) return;
        const next = list[e.key === "ArrowDown" ? i + 1 : i - 1];
        if (next) st.openViewer(next.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!a) return null;

  return (
    <main className="flex-1 min-w-0 flex flex-col bg-paper">
      <div className="flex-1 relative bg-white">
        <iframe
          key={`${a.id}:${a.currentHash}:${allowToken ?? "sandboxed"}`}
          src={`${assetUrl(a.id)}${allowToken ? `?allow=${allowToken}` : ""}`}
          sandbox="allow-scripts allow-same-origin"
          className={`absolute inset-0 w-full h-full border-0 ${dragging ? "pointer-events-none" : ""}`}
          title={a.title}
        />
        {allowToken ? (
          <button
            onClick={() => setAllowToken(null)}
            className="absolute bottom-3 right-3 flex items-center gap-1.5 bg-warn text-white text-[10.5px] font-bold px-2.5 py-1.5 rounded-full hover:opacity-90 transition"
          >
            <ShieldOff className="w-3 h-3" />
            {t("allowedTemp")}
          </button>
        ) : (
          blocked > 0 && (
            <button
              onClick={allowOnce}
              className="absolute bottom-3 right-3 flex items-center gap-1.5 bg-ink/85 text-white text-[10.5px] px-2.5 py-1.5 rounded-full hover:bg-ink transition"
            >
              <ShieldCheck className="w-3 h-3 text-ok" />
              {t("blockedN", { n: blocked })}
            </button>
          )
        )}
      </div>
    </main>
  );
}
