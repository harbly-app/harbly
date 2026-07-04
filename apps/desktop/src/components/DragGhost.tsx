import { FileCode2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useStore } from "../lib/store";

/// Drag ghost that follows the cursor; a global mouseup commits the drag result
export default function DragGhost() {
  const drag = useStore((s) => s.dragAsset);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!drag) {
      setPos(null);
      return;
    }
    const onMove = (e: MouseEvent) => setPos({ x: e.clientX, y: e.clientY });
    const onUp = () => useStore.getState().endDrag();
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    document.body.classList.add("cursor-grabbing");
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.classList.remove("cursor-grabbing");
    };
  }, [drag]);

  if (!drag || !pos) return null;
  return (
    <div
      className="fixed z-[100] pointer-events-none flex items-center gap-1.5 bg-card border border-primary shadow-lg rounded-full px-3 py-1.5 text-xs font-bold text-ink"
      style={{ left: pos.x + 12, top: pos.y - 10 }}
    >
      <FileCode2 className="w-3.5 h-3.5 text-primary" />
      <span className="max-w-[220px] truncate">{drag.label}</span>
      {drag.ids.length > 1 && (
        <span className="min-w-5 h-5 px-1 grid place-items-center rounded-full bg-primary text-white text-[10px] font-bold">
          {drag.ids.length}
        </span>
      )}
    </div>
  );
}
