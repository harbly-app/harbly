/**
 * ProseMirror schema for hdoc page documents — the editor-side mirror of the
 * h-* vocabulary (v1). Every node's parseDOM/toDOM maps 1:1 onto the file
 * format, so the file is the serialization: no intermediate representation,
 * no lossy conversion.
 *
 * Group conventions:
 * - "block": allowed at the document top level.
 * - "inner": allowed inside container components (callout/card/column/step/…).
 *   Grid-like containers (columns, stats, steps, toc) are top-level only,
 *   which keeps nesting sane without a separate validator.
 */
import { Schema } from "prosemirror-model";
import type { NodeSpec, MarkSpec } from "prosemirror-model";
import { tableNodes } from "prosemirror-tables";

/** Serialize only meaningful attributes (skip empty strings). */
const attrsOut = (
  pairs: Record<string, string | boolean | null | undefined>,
): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(pairs)) {
    if (v === true) out[k] = "";
    else if (typeof v === "string" && v !== "") out[k] = v;
  }
  return out;
};

const container = (tag: string, extraAttrs: string[] = []): NodeSpec => ({
  content: "inner+",
  group: "block inner",
  defining: true,
  attrs: Object.fromEntries(extraAttrs.map((a) => [a, { default: "" }])),
  parseDOM: [
    {
      tag,
      getAttrs: (dom) =>
        Object.fromEntries(
          extraAttrs.map((a) => [a, dom.getAttribute(a) ?? ""]),
        ),
    },
  ],
  toDOM(node) {
    return [tag, attrsOut(node.attrs), 0] as const as readonly [
      string,
      Record<string, string>,
      number,
    ];
  },
});

const nodes: Record<string, NodeSpec> = {
  doc: {
    content: "block+",
    attrs: { theme: { default: "paper" }, v: { default: "1" } },
  },

  paragraph: {
    content: "inline*",
    group: "block inner",
    parseDOM: [{ tag: "p" }],
    toDOM: () => ["p", 0],
  },

  heading: {
    attrs: { level: { default: 1 } },
    content: "inline*",
    group: "block inner",
    defining: true,
    parseDOM: [1, 2, 3].map((level) => ({
      tag: `h${level}`,
      attrs: { level },
    })),
    toDOM: (node) => [`h${node.attrs.level}`, 0],
  },

  bullet_list: {
    content: "list_item+",
    group: "block inner",
    parseDOM: [{ tag: "ul" }],
    toDOM: () => ["ul", 0],
  },

  ordered_list: {
    content: "list_item+",
    group: "block inner",
    parseDOM: [{ tag: "ol" }],
    toDOM: () => ["ol", 0],
  },

  list_item: {
    content: "paragraph (paragraph | bullet_list | ordered_list)*",
    defining: true,
    parseDOM: [{ tag: "li" }],
    toDOM: () => ["li", 0],
  },

  blockquote: {
    content: "paragraph+",
    group: "block inner",
    defining: true,
    parseDOM: [{ tag: "blockquote" }],
    toDOM: () => ["blockquote", 0],
  },

  code_block: {
    content: "text*",
    marks: "",
    group: "block inner",
    code: true,
    defining: true,
    parseDOM: [{ tag: "pre", preserveWhitespace: "full" }],
    toDOM: () => ["pre", ["code", 0]],
  },

  horizontal_rule: {
    group: "block inner",
    parseDOM: [{ tag: "hr" }],
    toDOM: () => ["hr"],
  },

  /** Inline image; usually lives inside h-figure but plain <img> in a
   * paragraph is part of the native whitelist too. */
  image: {
    inline: true,
    group: "inline",
    draggable: true,
    attrs: { src: { default: "" }, alt: { default: "" } },
    parseDOM: [
      {
        tag: "img",
        getAttrs: (dom) => ({
          src: dom.getAttribute("src") ?? "",
          alt: dom.getAttribute("alt") ?? "",
        }),
      },
    ],
    toDOM: (node) => [
      "img",
      { src: node.attrs.src as string, alt: node.attrs.alt as string },
    ],
  },

  figure: {
    content: "image",
    group: "block inner",
    attrs: { caption: { default: "" } },
    parseDOM: [
      {
        tag: "h-figure",
        getAttrs: (dom) => ({
          caption: dom.getAttribute("caption") ?? "",
        }),
      },
    ],
    toDOM: (node) => [
      "h-figure",
      attrsOut({ caption: node.attrs.caption as string }),
      0,
    ],
  },

  callout: {
    ...container("h-callout", ["title"]),
    attrs: { kind: { default: "note" }, title: { default: "" } },
    parseDOM: [
      {
        tag: "h-callout",
        getAttrs: (dom) => ({
          kind: dom.getAttribute("kind") ?? "note",
          title: dom.getAttribute("title") ?? "",
        }),
      },
    ],
    toDOM: (node) => [
      "h-callout",
      {
        kind: node.attrs.kind as string,
        ...attrsOut({ title: node.attrs.title as string }),
      },
      0,
    ],
  },

  card: container("h-card", ["title"]),
  quote: container("h-quote", ["cite"]),

  details: {
    ...container("h-details", ["summary"]),
    attrs: { summary: { default: "" }, open: { default: false } },
    parseDOM: [
      {
        tag: "h-details",
        getAttrs: (dom) => ({
          summary: dom.getAttribute("summary") ?? "",
          open: dom.hasAttribute("open"),
        }),
      },
    ],
    toDOM: (node) => [
      "h-details",
      attrsOut({
        summary: node.attrs.summary as string,
        open: node.attrs.open as boolean,
      }),
      0,
    ],
  },

  columns: {
    content: "column{2,4}",
    group: "block",
    defining: true,
    parseDOM: [{ tag: "h-columns" }],
    toDOM: () => ["h-columns", 0],
  },

  column: {
    content: "inner+",
    defining: true,
    parseDOM: [{ tag: "h-column" }],
    toDOM: () => ["h-column", 0],
  },

  steps: {
    content: "step+",
    group: "block",
    defining: true,
    parseDOM: [{ tag: "h-steps" }],
    toDOM: () => ["h-steps", 0],
  },

  step: {
    ...container("h-step", ["title"]),
    group: undefined,
  },

  stats: {
    content: "stat+",
    group: "block",
    defining: true,
    parseDOM: [{ tag: "h-stats" }],
    toDOM: () => ["h-stats", 0],
  },

  stat: {
    atom: true,
    selectable: true,
    attrs: { value: { default: "" }, label: { default: "" } },
    parseDOM: [
      {
        tag: "h-stat",
        getAttrs: (dom) => ({
          value: dom.getAttribute("value") ?? "",
          label: dom.getAttribute("label") ?? "",
        }),
      },
    ],
    toDOM: (node) => [
      "h-stat",
      attrsOut({
        value: node.attrs.value as string,
        label: node.attrs.label as string,
      }),
    ],
  },

  toc: {
    atom: true,
    selectable: true,
    group: "block",
    parseDOM: [{ tag: "h-toc" }],
    toDOM: () => ["h-toc"],
  },

  hard_break: {
    inline: true,
    group: "inline",
    selectable: false,
    parseDOM: [{ tag: "br" }],
    toDOM: () => ["br"],
  },

  text: { group: "inline" },

  // table / table_row / table_cell / table_header
  ...tableNodes({
    tableGroup: "block inner",
    cellContent: "inner+",
    cellAttributes: {},
  }),
};

const marks: Record<string, MarkSpec> = {
  link: {
    attrs: { href: { default: "" } },
    inclusive: false,
    parseDOM: [
      {
        tag: "a[href]",
        getAttrs: (dom) => ({
          href: dom.getAttribute("href") ?? "",
        }),
      },
    ],
    toDOM: (mark) => ["a", { href: mark.attrs.href as string }, 0],
  },
  strong: {
    parseDOM: [{ tag: "strong" }, { tag: "b" }],
    toDOM: () => ["strong", 0],
  },
  em: {
    parseDOM: [{ tag: "em" }, { tag: "i" }],
    toDOM: () => ["em", 0],
  },
  strike: {
    parseDOM: [{ tag: "s" }, { tag: "del" }],
    toDOM: () => ["s", 0],
  },
  code: {
    parseDOM: [{ tag: "code" }],
    toDOM: () => ["code", 0],
  },
};

export const hdocSchema = new Schema({ nodes, marks });

/** Element tags a v1 document may contain. Anything else switches the editor
 * to the read-only preview so a later save can never destroy content. */
export const ALLOWED_TAGS = new Set([
  "h-doc",
  "h-callout",
  "h-columns",
  "h-column",
  "h-card",
  "h-steps",
  "h-step",
  "h-figure",
  "h-quote",
  "h-stats",
  "h-stat",
  "h-details",
  "h-toc",
  "p",
  "h1",
  "h2",
  "h3",
  "ul",
  "ol",
  "li",
  "blockquote",
  "pre",
  "code",
  "hr",
  "img",
  "br",
  "a",
  "strong",
  "b",
  "em",
  "i",
  "s",
  "del",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
]);

export const THEMES = ["paper", "sepia", "night"] as const;
export type HdocTheme = (typeof THEMES)[number];
