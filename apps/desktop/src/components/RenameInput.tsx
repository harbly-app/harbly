import { useRef, useState } from "react";
import { useImeGuard } from "../lib/ime";

/// Finder-style in-place rename: auto select-all, Enter commits, Esc cancels, blur commits (exits silently if unchanged)
export default function RenameInput(props: {
  initial: string;
  className?: string;
  onCommit: (v: string) => void;
  onCancel: () => void;
}) {
  const [v, setV] = useState(props.initial);
  const done = useRef(false); // The blur right after an Enter commit must not fire a second time
  const ime = useImeGuard();

  const commit = () => {
    if (done.current) return;
    done.current = true;
    const t = v.trim();
    if (t && t !== props.initial) props.onCommit(t);
    else props.onCancel();
  };
  const cancel = () => {
    if (done.current) return;
    done.current = true;
    props.onCancel();
  };

  return (
    <input
      autoFocus
      value={v}
      onChange={(e) => setV(e.target.value)}
      onFocus={(e) => e.target.select()}
      onBlur={commit}
      onCompositionEnd={ime.end}
      onKeyDown={(e) => {
        e.stopPropagation();
        // IME keys belong to the composer — a candidate-confirming Enter must
        // not commit a half-typed pinyin string as the real file name.
        if (ime.guarded(e.nativeEvent)) return;
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") cancel();
      }}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      className={
        props.className ??
        "h-6 w-full rounded border border-primary bg-card px-1.5 text-[12.5px] outline-none"
      }
    />
  );
}
