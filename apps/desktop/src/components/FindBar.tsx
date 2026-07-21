import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { makeT } from "../lib/i18n";
import { useImeGuard } from "../lib/ime";
import { useStore } from "../lib/store";

/// In-document find bar (⌘F), floating under the title bar. Talks to whatever
/// surface registered a FindHandle (PM editors or the sandboxed preview) and
/// stays open across arrow-key file switches — the query re-runs on the newly
/// registered handle, which makes ⌘F + ↓ a quick "scan the folder" flow.
export default function FindBar() {
  const open = useStore((s) => s.findOpen);
  const handle = useStore((s) => s.findHandle);
  const closeFind = useStore((s) => s.closeFind);
  const t = makeT(useStore((s) => s.lang));
  const [q, setQ] = useState("");
  const [count, setCount] = useState(0);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const ime = useImeGuard();

  // Debounced (re-)search: fires on typing, on open, and when a new surface
  // registers after a file switch. Stale async replies are ignored.
  useEffect(() => {
    if (!open || !handle) return;
    let live = true;
    const timer = setTimeout(() => {
      handle
        .search(q)
        .then((r) => {
          if (live) {
            setCount(r.count);
            setActive(r.active);
          }
        })
        .catch(() => {});
    }, 120);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [q, open, handle]);

  useEffect(() => {
    if (open) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [open]);

  const step = (delta: 1 | -1) => {
    handle
      ?.step(delta)
      .then((r) => {
        setCount(r.count);
        setActive(r.active);
      })
      .catch(() => {});
  };

  // ⌘G / ⌘⇧G step matches; a second ⌘F refocuses the input
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === "g") {
        e.preventDefault();
        step(e.shiftKey ? -1 : 1);
      } else if (k === "f") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, handle]);

  if (!open || !handle) return null;

  return (
    <div className="absolute top-2 right-3 z-20 flex items-center gap-1 rounded-ctl border border-line bg-card px-2 py-1.5 shadow-lg">
      <input
        ref={inputRef}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onCompositionEnd={ime.end}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (ime.guarded(e.nativeEvent)) return;
          if (e.key === "Enter") step(e.shiftKey ? -1 : 1);
          else if (e.key === "Escape") closeFind();
        }}
        placeholder={t("findPlaceholder")}
        className="h-6 w-44 bg-transparent px-1 text-[12.5px] outline-none placeholder:text-sub"
      />
      <span className="min-w-[44px] text-center text-[11px] text-sub tabular-nums">
        {count > 0 ? `${active} / ${count}` : q ? "0 / 0" : ""}
      </span>
      <button
        onClick={() => step(-1)}
        disabled={count === 0}
        title={`${t("findPrev")} (⇧↩)`}
        aria-label={t("findPrev")}
        className="grid h-6 w-6 place-items-center rounded text-sub transition hover:bg-side hover:text-ink disabled:opacity-40"
      >
        <ChevronUp className="h-3.5 w-3.5" />
      </button>
      <button
        onClick={() => step(1)}
        disabled={count === 0}
        title={`${t("findNext")} (↩)`}
        aria-label={t("findNext")}
        className="grid h-6 w-6 place-items-center rounded text-sub transition hover:bg-side hover:text-ink disabled:opacity-40"
      >
        <ChevronDown className="h-3.5 w-3.5" />
      </button>
      <button
        onClick={closeFind}
        title={`${t("findClose")} (esc)`}
        aria-label={t("findClose")}
        className="grid h-6 w-6 place-items-center rounded text-sub transition hover:bg-side hover:text-ink"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
