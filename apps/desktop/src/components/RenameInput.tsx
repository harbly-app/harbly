import { useRef, useState } from "react";

/// Finder-style in-place rename: auto select-all, Enter commits, Esc cancels, blur commits (exits silently if unchanged)
export default function RenameInput(props: {
  initial: string;
  className?: string;
  onCommit: (v: string) => void;
  onCancel: () => void;
}) {
  const [v, setV] = useState(props.initial);
  const done = useRef(false); // The blur right after an Enter commit must not fire a second time

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
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") cancel();
      }}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      className={
        props.className ??
        "w-full h-6 px-1.5 rounded border border-primary bg-white outline-none text-[12.5px]"
      }
    />
  );
}
