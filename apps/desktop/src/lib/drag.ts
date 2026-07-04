import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent as ReactMouseEvent } from "react";
import { tr } from "./i18n";
import { useStore } from "./store";
import type { DragPayload } from "./store";

/// Explicit window dragging (does not rely on the injected data-tauri-drag-region listener); double-click toggles maximize
export function windowDrag(e: ReactMouseEvent) {
  if (e.button !== 0) return;
  const w = getCurrentWindow();
  const op = e.detail === 2 ? w.toggleMaximize() : w.startDragging();
  op.catch((err: unknown) =>
    useStore.getState().showToast(tr("windowOpFail", { err: String(err) })),
  );
}

const DRAG_OUT_ICON =
  "iVBORw0KGgoAAAANSUhEUgAAABwAAAAcCAYAAAByDd+UAAAATklEQVR42mNgwAPyws6/JgczkALItYQsy6ltGV5LaWUZVktpbRmGpcPbQnpZBrd01MJRC0ctHLVw1MIhaOFoBTxk2zUD30wckIYwPZr6ALrl5NPFAgayAAAAAElFTkSuQmCC";

/// Entry point for custom mouse dragging: only enters drag mode after moving past a threshold (distinguishes from clicks).
/// The payload can be a factory function — for multi-select, it reads the latest selection at mousedown (may contain multiple items).
/// Option-drag = native drag out to the system (Finder/Mail etc.); otherwise it is an in-library move.
export function dragStartHandler(payload: DragPayload | (() => DragPayload)) {
  return (e: ReactMouseEvent) => {
    if (e.button !== 0) return;
    const resolve = () => (typeof payload === "function" ? payload() : payload);
    if (e.altKey) {
      const root = useStore.getState().root;
      if (!root) return;
      const items = resolve().rels.map((rel) => `${root}/${rel}`);
      if (!items.length) return;
      import("@crabnebula/tauri-plugin-drag")
        .then(({ startDrag }) =>
          startDrag({ item: items, icon: DRAG_OUT_ICON }),
        )
        .catch((err: unknown) =>
          useStore
            .getState()
            .showToast(tr("dragOutFail", { err: String(err) })),
        );
      return;
    }
    if (e.metaKey || e.shiftKey) return; // Modifier-click = adjust selection, do not start a drag
    const sx = e.clientX;
    const sy = e.clientY;
    const onMove = (me: MouseEvent) => {
      if (Math.abs(me.clientX - sx) + Math.abs(me.clientY - sy) > 6) {
        cleanup();
        useStore.getState().startDrag(resolve());
      }
    };
    const cleanup = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", cleanup);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", cleanup);
  };
}
