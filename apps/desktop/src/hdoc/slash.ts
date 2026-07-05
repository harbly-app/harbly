/**
 * Slash insert menu: typing "/" at the start of an (empty or space-preceded)
 * position opens a filterable block palette. The slash itself is never typed
 * into the document; while the menu is open, printable keys build the filter,
 * ↑/↓ navigate, ↵ inserts, Esc cancels. Component blocks insert at the top
 * level (after the current block, or replacing it when it is an empty
 * paragraph); text blocks transform in place.
 */
import { setBlockType } from "prosemirror-commands";
import type { Node as PMNode } from "prosemirror-model";
import { Plugin, PluginKey, Selection } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { wrapInList } from "prosemirror-schema-list";
import { tr } from "../lib/i18n";
import { hdocSchema } from "./schema";

const n = hdocSchema.nodes;
const p = () => n.paragraph.create();

const fill = (type: (typeof n)[string], attrs?: Record<string, unknown>) =>
  type.createAndFill(attrs ?? null) ?? type.create(attrs ?? null);

function makeColumns(count: number): PMNode {
  const cols = Array.from({ length: count }, () => n.column.create(null, p()));
  return n.columns.create(null, cols);
}

function makeTable(): PMNode {
  const cell = (t: (typeof n)[string]) => fill(t);
  const header = n.table_row.create(null, [
    cell(n.table_header),
    cell(n.table_header),
    cell(n.table_header),
  ]);
  const row = () =>
    n.table_row.create(null, [
      cell(n.table_cell),
      cell(n.table_cell),
      cell(n.table_cell),
    ]);
  return n.table.create(null, [header, row(), row()]);
}

interface SlashItem {
  key: string;
  label: () => string;
  /** "transform": change the current textblock; "insert": add a block node */
  run: (view: EditorView) => void;
}

/** Insert a block: replace the current empty top-level paragraph, otherwise
 * append after the current top-level block; cursor lands inside. */
function insertTopLevel(view: EditorView, node: PMNode) {
  const { state } = view;
  const { $from } = state.selection;
  let t = state.tr;
  let at: number;
  if (
    $from.depth >= 1 &&
    $from.parent.type === n.paragraph &&
    $from.parent.content.size === 0 &&
    $from.depth === 1
  ) {
    at = $from.before(1);
    t = t.replaceRangeWith(at, $from.after(1), node);
  } else {
    at = $from.depth >= 1 ? $from.after(1) : state.selection.to;
    t = t.insert(at, node);
  }
  const sel = Selection.near(t.doc.resolve(at + 1), 1);
  t = t.setSelection(sel).scrollIntoView();
  view.dispatch(t);
  view.focus();
}

function transform(cmd: (view: EditorView) => boolean) {
  return (view: EditorView) => {
    cmd(view);
    view.focus();
  };
}

function items(): SlashItem[] {
  return [
    {
      key: "h1",
      label: () => tr("insH1"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 1 })(v.state, v.dispatch),
      ),
    },
    {
      key: "h2",
      label: () => tr("insH2"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 2 })(v.state, v.dispatch),
      ),
    },
    {
      key: "h3",
      label: () => tr("insH3"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 3 })(v.state, v.dispatch),
      ),
    },
    {
      key: "bullet",
      label: () => tr("insBullet"),
      run: transform((v) => wrapInList(n.bullet_list)(v.state, v.dispatch)),
    },
    {
      key: "numbered",
      label: () => tr("insNumbered"),
      run: transform((v) => wrapInList(n.ordered_list)(v.state, v.dispatch)),
    },
    {
      key: "code",
      label: () => tr("insCode"),
      run: transform((v) => setBlockType(n.code_block)(v.state, v.dispatch)),
    },
    {
      key: "callout",
      label: () => tr("insCallout"),
      run: (v) => insertTopLevel(v, n.callout.create({ kind: "note" }, p())),
    },
    {
      key: "columns2",
      label: () => tr("insColumns2"),
      run: (v) => insertTopLevel(v, makeColumns(2)),
    },
    {
      key: "columns3",
      label: () => tr("insColumns3"),
      run: (v) => insertTopLevel(v, makeColumns(3)),
    },
    {
      key: "card",
      label: () => tr("insCard"),
      run: (v) => insertTopLevel(v, n.card.create(null, p())),
    },
    {
      key: "steps",
      label: () => tr("insSteps"),
      run: (v) =>
        insertTopLevel(
          v,
          n.steps.create(null, [
            n.step.create(null, p()),
            n.step.create(null, p()),
            n.step.create(null, p()),
          ]),
        ),
    },
    {
      key: "quote",
      label: () => tr("insQuote"),
      run: (v) => insertTopLevel(v, n.quote.create(null, p())),
    },
    {
      key: "stats",
      label: () => tr("insStats"),
      run: (v) =>
        insertTopLevel(
          v,
          n.stats.create(null, [
            n.stat.create(),
            n.stat.create(),
            n.stat.create(),
          ]),
        ),
    },
    {
      key: "details",
      label: () => tr("insDetails"),
      run: (v) => insertTopLevel(v, n.details.create(null, p())),
    },
    {
      key: "image",
      label: () => tr("insImage"),
      run: (v) => insertTopLevel(v, n.figure.create(null, n.image.create())),
    },
    {
      key: "table",
      label: () => tr("insTable"),
      run: (v) => insertTopLevel(v, makeTable()),
    },
    {
      key: "toc",
      label: () => tr("insToc"),
      run: (v) => insertTopLevel(v, n.toc.create()),
    },
    {
      key: "divider",
      label: () => tr("insDivider"),
      run: (v) => insertTopLevel(v, n.horizontal_rule.create()),
    },
  ];
}

export const slashKey = new PluginKey("hdoc-slash");

export function slashMenu(): Plugin {
  let menu: HTMLDivElement | null = null;
  let open = false;
  let query = "";
  let index = 0;
  let all: SlashItem[] = [];
  let filtered: SlashItem[] = [];
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
      it.label().toLowerCase().includes(query.toLowerCase()),
    );
    if (index >= filtered.length) index = Math.max(0, filtered.length - 1);
    m.replaceChildren();
    if (query) {
      const q = document.createElement("div");
      q.className = "hd-slash-q";
      q.textContent = `/${query}`;
      m.appendChild(q);
    }
    filtered.forEach((it, i) => {
      const row = document.createElement("div");
      row.className = `hd-slash-item${i === index ? " sel" : ""}`;
      row.textContent = it.label();
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
    });
    if (filtered.length === 0) close();
  };

  const openAt = (view: EditorView) => {
    if (!menu) return;
    const coords = view.coordsAtPos(view.state.selection.from);
    open = true;
    query = "";
    index = 0;
    all = items();
    menu.style.display = "block";
    // position: fixed → viewport coordinates straight from coordsAtPos
    const menuH = 320;
    const below = coords.bottom + 6;
    const top =
      below + menuH > window.innerHeight
        ? Math.max(8, coords.top - menuH - 6)
        : below;
    menu.style.left = `${Math.min(coords.left, window.innerWidth - 240)}px`;
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
          if ($from.parent.type === n.code_block) return false;
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
