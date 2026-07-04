import * as CM from "@radix-ui/react-context-menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { tr } from "../lib/i18n";
import { useStore } from "../lib/store";
import type { DragPayload } from "../lib/store";

/// Explicit window dragging (does not rely on the injected data-tauri-drag-region listener); double-click toggles maximize
export function windowDrag(e: React.MouseEvent) {
  if (e.button !== 0) return;
  const w = getCurrentWindow();
  const op = e.detail === 2 ? w.toggleMaximize() : w.startDragging();
  op.catch((err) => useStore.getState().showToast(tr("windowOpFail", { err: String(err) })));
}

export const menuContentCls =
  "z-50 min-w-[190px] bg-white border border-line rounded-xl shadow-xl p-1.5 text-[12.5px]";

export function MItem(props: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <CM.Item
      onSelect={props.onClick}
      className={`flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg outline-none cursor-default data-[highlighted]:bg-side ${
        props.danger ? "text-danger" : ""
      }`}
    >
      {props.icon}
      <span className="flex-1">{props.label}</span>
      {props.hint && <span className="text-[10px] text-sub">{props.hint}</span>}
    </CM.Item>
  );
}

export const MSep = () => <CM.Separator className="h-px bg-line my-1.5" />;

const DRAG_OUT_ICON =
  "iVBORw0KGgoAAAANSUhEUgAAABwAAAAcCAYAAAByDd+UAAAATklEQVR42mNgwAPyws6/JgczkALItYQsy6ltGV5LaWUZVktpbRmGpcPbQnpZBrd01MJRC0ctHLVw1MIhaOFoBTxk2zUD30wckIYwPZr6ALrl5NPFAgayAAAAAElFTkSuQmCC";

/// Entry point for custom mouse dragging: only enters drag mode after moving past a threshold (distinguishes from clicks).
/// The payload can be a factory function — for multi-select, it reads the latest selection at mousedown (may contain multiple items).
/// Option-drag = native drag out to the system (Finder/Mail etc.); otherwise it is an in-library move.
export function dragStartHandler(payload: DragPayload | (() => DragPayload)) {
  return (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const resolve = () => (typeof payload === "function" ? payload() : payload);
    if (e.altKey) {
      const root = useStore.getState().root;
      if (!root) return;
      const items = resolve().rels.map((rel) => `${root}/${rel}`);
      if (!items.length) return;
      import("@crabnebula/tauri-plugin-drag")
        .then(({ startDrag }) => startDrag({ item: items, icon: DRAG_OUT_ICON }))
        .catch((err) => useStore.getState().showToast(tr("dragOutFail", { err: String(err) })));
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
