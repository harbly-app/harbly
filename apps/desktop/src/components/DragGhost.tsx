import { FileCode2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useStore } from "../lib/store";
import type { DragPayload } from "../lib/store";

/// Drag ghost that follows the cursor; a global mouseup commits the drag result
export default function DragGhost() {
  const drag = useStore((s) => s.dragAsset);
  if (!drag) return null;
  // Mounted only while a drag is active; unmount discards the stale position
  return <Ghost drag={drag} />;
}

function Ghost({ drag }: { drag: DragPayload }) {
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const onMove = (e: MouseEvent) => setPos({ x: e.clientX, y: e.clientY });
    const onUp = () => {
      void useStore.getState().endDrag();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    document.body.classList.add("cursor-grabbing");
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.classList.remove("cursor-grabbing");
    };
  }, []);

  if (!pos) return null;
  return (
    <div
      className="pointer-events-none fixed z-[100] flex items-center gap-1.5 rounded-full border border-primary bg-card px-3 py-1.5 text-xs font-bold text-ink shadow-lg"
      style={{ left: pos.x + 12, top: pos.y - 10 }}
    >
      <FileCode2 className="h-3.5 w-3.5 text-primary" />
      <span className="max-w-[220px] truncate">{drag.label}</span>
      {drag.ids.length > 1 && (
        <span className="grid h-5 min-w-5 place-items-center rounded-full bg-primary px-1 text-[10px] font-bold text-white">
          {drag.ids.length}
        </span>
      )}
    </div>
  );
}
