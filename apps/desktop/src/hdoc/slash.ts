/**
 * Slash insert menu: typing "/" at the start of an (empty or space-preceded)
 * position opens a filterable block palette fed by the shared item catalog
 * (items.ts). The slash itself is never typed into the document; while the
 * menu is open, printable keys build the filter, ↑/↓ navigate (with scroll
 * follow), ↵ inserts, Esc cancels.
 */
import { Plugin, PluginKey } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { tr } from "../lib/i18n";
import { hdocItems } from "./items";
import type { HdocItem } from "./items";
import { hdocSchema } from "./schema";

export const slashKey = new PluginKey("hdoc-slash");

const GROUPS: { id: HdocItem["group"]; labelKey: string }[] = [
  { id: "basic", labelKey: "slashBasic" },
  { id: "component", labelKey: "slashComponents" },
];

export function slashMenu(): Plugin {
  let menu: HTMLDivElement | null = null;
  let open = false;
  let query = "";
  let index = 0;
  let all: HdocItem[] = [];
  let filtered: HdocItem[] = [];
  let viewRef: EditorView | null = null;

  const close = () => {
    open = false;
    query = "";
    index = 0;
    if (menu) menu.style.display = "none";
  };

  const render = () => {
    const m = menu;
    if (!m) return;
    filtered = all.filter((it) =>
      tr(it.labelKey).toLowerCase().includes(query.toLowerCase()),
    );
    if (index >= filtered.length) index = Math.max(0, filtered.length - 1);
    m.replaceChildren();
    if (query) {
      const q = document.createElement("div");
      q.className = "hd-slash-q";
      q.textContent = `/${query}`;
      m.appendChild(q);
    }
    for (const g of GROUPS) {
      const members = filtered.filter((it) => it.group === g.id);
      if (members.length === 0) continue;
      const head = document.createElement("div");
      head.className = "hd-slash-group";
      head.textContent = tr(g.labelKey);
      m.appendChild(head);
      for (const it of members) {
        const i = filtered.indexOf(it);
        const row = document.createElement("div");
        row.className = `hd-slash-item${i === index ? " sel" : ""}`;
        const ico = document.createElement("span");
        ico.className = "hd-ico";
        ico.innerHTML = it.icon;
        const label = document.createElement("span");
        label.textContent = tr(it.labelKey);
        row.append(ico, label);
        row.addEventListener("mousedown", (e) => {
          e.preventDefault();
          const v = viewRef;
          close();
          if (v) it.run(v);
        });
        row.addEventListener("mousemove", () => {
          if (index !== i) {
            index = i;
            render();
          }
        });
        m.appendChild(row);
      }
    }
    // Keyboard navigation follows into the scrolled region
    m.querySelector(".hd-slash-item.sel")?.scrollIntoView({ block: "nearest" });
    if (filtered.length === 0) close();
  };

  const openAt = (view: EditorView) => {
    if (!menu) return;
    const coords = view.coordsAtPos(view.state.selection.from);
    open = true;
    query = "";
    index = 0;
    all = hdocItems();
    menu.style.display = "block";
    // position: fixed → viewport coordinates straight from coordsAtPos
    const menuH = 320;
    const below = coords.bottom + 6;
    const top =
      below + menuH > window.innerHeight
        ? Math.max(8, coords.top - menuH - 6)
        : below;
    menu.style.left = `${Math.min(coords.left, window.innerWidth - 250)}px`;
    menu.style.top = `${top}px`;
    render();
  };

  return new Plugin({
    key: slashKey,
    view(view) {
      viewRef = view;
      menu = document.createElement("div");
      menu.className = "hd-slash";
      menu.style.display = "none";
      document.body.appendChild(menu);
      return {
        destroy() {
          menu?.remove();
          menu = null;
          viewRef = null;
        },
      };
    },
    props: {
      handleKeyDown(view, event) {
        if (event.isComposing) return false;
        if (!open) {
          if (
            event.key !== "/" ||
            event.metaKey ||
            event.ctrlKey ||
            event.altKey
          )
            return false;
          const { $from, empty } = view.state.selection;
          if (!empty || !$from.parent.isTextblock) return false;
          if ($from.parent.type === hdocSchema.nodes.code_block) return false;
          const before = $from.parent.textBetween(
            Math.max(0, $from.parentOffset - 1),
            $from.parentOffset,
          );
          if ($from.parentOffset !== 0 && before !== " ") return false;
          event.preventDefault();
          openAt(view);
          return true;
        }
        // menu open: intercept navigation and filtering
        if (event.key === "Escape") {
          close();
          return true;
        }
        if (event.key === "Enter") {
          const it = filtered.at(index);
          close();
          if (it) it.run(view);
          return true;
        }
        if (event.key === "ArrowDown") {
          index = (index + 1) % Math.max(1, filtered.length);
          render();
          return true;
        }
        if (event.key === "ArrowUp") {
          index =
            (index - 1 + Math.max(1, filtered.length)) %
            Math.max(1, filtered.length);
          render();
          return true;
        }
        if (event.key === "Backspace") {
          if (query === "") close();
          else {
            query = query.slice(0, -1);
            render();
          }
          return true;
        }
        if (event.key.length === 1 && !event.metaKey && !event.ctrlKey) {
          if (event.key === " ") {
            close();
            return false;
          }
          query += event.key;
          render();
          return true;
        }
        return false;
      },
      handleDOMEvents: {
        blur() {
          close();
          return false;
        },
        mousedown() {
          if (open) close();
          return false;
        },
      },
    },
  });
}
