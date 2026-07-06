/**
 * The insertable block catalog — single source of truth shared by the slash
 * menu (vanilla DOM) and the insert palette (React). Icons are inline SVG
 * strings so both consumers render identically. Every item supports both
 * interactions: `run` for click/Enter (text blocks transform in place,
 * components insert top-level) and `make` for drag-to-place (the palette
 * hands ProseMirror a ready slice and its drop logic does the rest).
 */
import { setBlockType } from "prosemirror-commands";
import type { Node as PMNode } from "prosemirror-model";
import { Selection } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { wrapInList } from "prosemirror-schema-list";
import { hdocSchema } from "./schema";

const n = hdocSchema.nodes;
const p = () => n.paragraph.create();

const fill = (type: (typeof n)[string]) =>
  type.createAndFill() ?? type.create();

function makeColumns(count: number): PMNode {
  const cols = Array.from({ length: count }, () => n.column.create(null, p()));
  return n.columns.create(null, cols);
}

function makeTable(): PMNode {
  const header = n.table_row.create(null, [
    fill(n.table_header),
    fill(n.table_header),
    fill(n.table_header),
  ]);
  const row = () =>
    n.table_row.create(null, [
      fill(n.table_cell),
      fill(n.table_cell),
      fill(n.table_cell),
    ]);
  return n.table.create(null, [header, row(), row()]);
}

/** Insert a block at the top level: replace the current empty paragraph,
 * otherwise append after the current top-level block; cursor lands inside. */
export function insertTopLevel(view: EditorView, node: PMNode) {
  const { state } = view;
  const { $from } = state.selection;
  let t = state.tr;
  let at: number;
  if (
    $from.depth === 1 &&
    $from.parent.type === n.paragraph &&
    $from.parent.content.size === 0
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

export interface HdocItem {
  key: string;
  /** i18n key for the display label */
  labelKey: string;
  group: "basic" | "component";
  /** inline SVG markup (24×24 viewBox, currentColor strokes) */
  icon: string;
  /** click / Enter behavior */
  run: (view: EditorView) => void;
  /** node factory for drag-to-place */
  make: () => PMNode;
}

const svg = (body: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
const textIcon = (label: string) =>
  `<svg viewBox="0 0 24 24"><text x="12" y="16.5" text-anchor="middle" font-size="12" font-weight="700" fill="currentColor">${label}</text></svg>`;

const transform =
  (cmd: (view: EditorView) => boolean) => (view: EditorView) => {
    cmd(view);
    view.focus();
  };

export function hdocItems(): HdocItem[] {
  return [
    {
      key: "h1",
      labelKey: "insH1",
      group: "basic",
      icon: textIcon("H1"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 1 })(v.state, v.dispatch),
      ),
      make: () => n.heading.create({ level: 1 }),
    },
    {
      key: "h2",
      labelKey: "insH2",
      group: "basic",
      icon: textIcon("H2"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 2 })(v.state, v.dispatch),
      ),
      make: () => n.heading.create({ level: 2 }),
    },
    {
      key: "h3",
      labelKey: "insH3",
      group: "basic",
      icon: textIcon("H3"),
      run: transform((v) =>
        setBlockType(n.heading, { level: 3 })(v.state, v.dispatch),
      ),
      make: () => n.heading.create({ level: 3 }),
    },
    {
      key: "bullet",
      labelKey: "insBullet",
      group: "basic",
      icon: svg(
        '<path d="M9 6h12M9 12h12M9 18h12"/><circle cx="4" cy="6" r="1" fill="currentColor"/><circle cx="4" cy="12" r="1" fill="currentColor"/><circle cx="4" cy="18" r="1" fill="currentColor"/>',
      ),
      run: transform((v) => wrapInList(n.bullet_list)(v.state, v.dispatch)),
      make: () => n.bullet_list.create(null, n.list_item.create(null, p())),
    },
    {
      key: "numbered",
      labelKey: "insNumbered",
      group: "basic",
      icon: svg(
        '<path d="M10 6h11M10 12h11M10 18h11"/><path d="M3 5.5 4.5 4v4M3.5 14h2l-2 3h2" stroke-width="1.4"/>',
      ),
      run: transform((v) => wrapInList(n.ordered_list)(v.state, v.dispatch)),
      make: () => n.ordered_list.create(null, n.list_item.create(null, p())),
    },
    {
      key: "code",
      labelKey: "insCode",
      group: "basic",
      icon: svg(
        '<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>',
      ),
      run: transform((v) => setBlockType(n.code_block)(v.state, v.dispatch)),
      make: () => n.code_block.create(),
    },
    {
      key: "table",
      labelKey: "insTable",
      group: "basic",
      icon: svg(
        '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 10h18M10 4v16"/>',
      ),
      run: (v) => insertTopLevel(v, makeTable()),
      make: makeTable,
    },
    {
      key: "image",
      labelKey: "insImage",
      group: "basic",
      icon: svg(
        '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>',
      ),
      run: (v) => insertTopLevel(v, n.figure.create(null, n.image.create())),
      make: () => n.figure.create(null, n.image.create()),
    },
    {
      key: "divider",
      labelKey: "insDivider",
      group: "basic",
      icon: svg('<path d="M3 12h18"/>'),
      run: (v) => insertTopLevel(v, n.horizontal_rule.create()),
      make: () => n.horizontal_rule.create(),
    },
    {
      key: "callout",
      labelKey: "insCallout",
      group: "component",
      icon: svg(
        '<circle cx="12" cy="12" r="9"/><path d="M12 16v-4M12 8h.01"/>',
      ),
      run: (v) => insertTopLevel(v, n.callout.create({ kind: "note" }, p())),
      make: () => n.callout.create({ kind: "note" }, p()),
    },
    {
      key: "columns2",
      labelKey: "insColumns2",
      group: "component",
      icon: svg(
        '<rect x="3" y="4" width="8" height="16" rx="1.5"/><rect x="13" y="4" width="8" height="16" rx="1.5"/>',
      ),
      run: (v) => insertTopLevel(v, makeColumns(2)),
      make: () => makeColumns(2),
    },
    {
      key: "columns3",
      labelKey: "insColumns3",
      group: "component",
      icon: svg(
        '<rect x="2" y="4" width="5.4" height="16" rx="1.2"/><rect x="9.3" y="4" width="5.4" height="16" rx="1.2"/><rect x="16.6" y="4" width="5.4" height="16" rx="1.2"/>',
      ),
      run: (v) => insertTopLevel(v, makeColumns(3)),
      make: () => makeColumns(3),
    },
    {
      key: "card",
      labelKey: "insCard",
      group: "component",
      icon: svg(
        '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 9.5h7"/>',
      ),
      run: (v) => insertTopLevel(v, n.card.create(null, p())),
      make: () => n.card.create(null, p()),
    },
    {
      key: "steps",
      labelKey: "insSteps",
      group: "component",
      icon: svg(
        '<circle cx="5" cy="6" r="2.4"/><circle cx="5" cy="17" r="2.4"/><path d="M11 6h10M11 17h10M5 8.5v6"/>',
      ),
      run: (v) =>
        insertTopLevel(
          v,
          n.steps.create(null, [
            n.step.create(null, p()),
            n.step.create(null, p()),
            n.step.create(null, p()),
          ]),
        ),
      make: () =>
        n.steps.create(null, [
          n.step.create(null, p()),
          n.step.create(null, p()),
          n.step.create(null, p()),
        ]),
    },
    {
      key: "quote",
      labelKey: "insQuote",
      group: "component",
      icon: svg(
        '<path d="M8 11H4.5A1.5 1.5 0 0 1 3 9.5v-2A1.5 1.5 0 0 1 4.5 6H7a1 1 0 0 1 1 1v6c0 2.5-1.2 4-3.5 4.5M21 11h-3.5A1.5 1.5 0 0 1 16 9.5v-2A1.5 1.5 0 0 1 17.5 6H20a1 1 0 0 1 1 1v6c0 2.5-1.2 4-3.5 4.5"/>',
      ),
      run: (v) => insertTopLevel(v, n.quote.create(null, p())),
      make: () => n.quote.create(null, p()),
    },
    {
      key: "stats",
      labelKey: "insStats",
      group: "component",
      icon: svg('<path d="M3 3v18h18"/><path d="M7 16v-5M12 16V8M17 16v-3"/>'),
      run: (v) =>
        insertTopLevel(
          v,
          n.stats.create(null, [
            n.stat.create(),
            n.stat.create(),
            n.stat.create(),
          ]),
        ),
      make: () =>
        n.stats.create(null, [
          n.stat.create(),
          n.stat.create(),
          n.stat.create(),
        ]),
    },
    {
      key: "details",
      labelKey: "insDetails",
      group: "component",
      icon: svg(
        '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="m8 10 4 4 4-4"/>',
      ),
      run: (v) => insertTopLevel(v, n.details.create(null, p())),
      make: () => n.details.create(null, p()),
    },
    {
      key: "toc",
      labelKey: "insToc",
      group: "component",
      icon: svg('<path d="M4 6h16M7 12h13M10 18h10"/>'),
      run: (v) => insertTopLevel(v, n.toc.create()),
      make: () => n.toc.create(),
    },
  ];
}
