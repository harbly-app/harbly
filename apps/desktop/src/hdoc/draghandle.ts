/**
 * Block drag handle: hovering a top-level block shows a grip in the left
 * gutter; dragging the grip moves the whole block (ProseMirror's own drop
 * logic + dropcursor handle the destination). The grip lives outside the
 * contentEditable, so the drag is wired manually through `view.dragging`.
 */
import { DOMSerializer } from "prosemirror-model";
import { NodeSelection, Plugin } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";

export function dragHandle(): Plugin {
  return new Plugin({
    view(view: EditorView) {
      const container = view.dom.parentElement;
      if (!container) return {};
      const handle = document.createElement("div");
      handle.className = "hd-drag";
      handle.draggable = true;
      handle.textContent = "⋮⋮";
      handle.style.display = "none";
      container.appendChild(handle);

      let blockPos: number | null = null;

      const hide = () => {
        handle.style.display = "none";
        blockPos = null;
      };

      const onMove = (e: MouseEvent) => {
        if (view.dragging || view.composing) return;
        // Find the top-level block whose vertical extent contains the pointer
        const contRect = container.getBoundingClientRect();
        const doc = view.state.doc;
        let found: { pos: number; rect: DOMRect } | null = null;
        for (let i = 0, offset = 0; i < doc.childCount && !found; i++) {
          const dom = view.nodeDOM(offset);
          if (dom instanceof HTMLElement) {
            const rect = dom.getBoundingClientRect();
            if (e.clientY >= rect.top && e.clientY <= rect.bottom) {
              found = { pos: offset, rect };
            }
          }
          offset += doc.child(i).nodeSize;
        }
        if (!found) {
          hide();
          return;
        }
        blockPos = found.pos;
        handle.style.display = "grid";
        handle.style.top = `${found.rect.top - contRect.top + 2}px`;
        handle.style.left = `${Math.max(0, found.rect.left - contRect.left - 26)}px`;
      };

      const onLeave = (e: MouseEvent) => {
        if (
          e.relatedTarget instanceof Node &&
          container.contains(e.relatedTarget)
        )
          return;
        hide();
      };

      const onDragStart = (e: DragEvent) => {
        if (blockPos === null || !e.dataTransfer) return;
        const sel = NodeSelection.create(view.state.doc, blockPos);
        view.dispatch(view.state.tr.setSelection(sel));
        const slice = sel.content();
        const frag = DOMSerializer.fromSchema(
          view.state.schema,
        ).serializeFragment(slice.content);
        const div = document.createElement("div");
        div.appendChild(frag);
        e.dataTransfer.setData("text/html", div.innerHTML);
        e.dataTransfer.setData("text/plain", sel.node.textContent);
        e.dataTransfer.effectAllowed = "copyMove";
        view.dragging = { slice, move: true };
      };

      const onDragEnd = () => {
        hide();
      };

      container.addEventListener("mousemove", onMove);
      container.addEventListener("mouseleave", onLeave);
      handle.addEventListener("dragstart", onDragStart);
      handle.addEventListener("dragend", onDragEnd);

      return {
        destroy() {
          container.removeEventListener("mousemove", onMove);
          container.removeEventListener("mouseleave", onLeave);
          handle.remove();
        },
      };
    },
  });
}
