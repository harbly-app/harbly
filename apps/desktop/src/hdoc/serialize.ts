/**
 * PM document → hdoc source text. Hand-rolled instead of DOMSerializer so the
 * on-disk format is deterministic and diff-friendly: two-space indentation for
 * block structure, inline content on one line, attributes only when meaningful.
 * parse(serialize(doc)) must round-trip to an equal document — vitest covers it.
 */
import type { Mark, Node as PMNode } from "prosemirror-model";

const escText = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escAttr = (s: string) => escText(s).replace(/"/g, "&quot;");

function openTag(
  tag: string,
  attrs?: Record<string, string | boolean | null | undefined>,
): string {
  let out = `<${tag}`;
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v === true) out += ` ${k}`;
      else if (typeof v === "string" && v !== "")
        out += ` ${k}="${escAttr(v)}"`;
    }
  }
  return `${out}>`;
}

function markTag(m: Mark): { open: string; close: string } {
  switch (m.type.name) {
    case "link":
      return {
        open: openTag("a", { href: m.attrs.href as string }),
        close: "</a>",
      };
    case "strong":
      return { open: "<strong>", close: "</strong>" };
    case "em":
      return { open: "<em>", close: "</em>" };
    case "strike":
      return { open: "<s>", close: "</s>" };
    case "code":
      return { open: "<code>", close: "</code>" };
    default:
      return { open: "", close: "" };
  }
}

/** Inline children of a textblock, with properly nested mark tags. */
function inline(parent: PMNode): string {
  let out = "";
  let active: Mark[] = [];
  const closeTo = (n: number) => {
    for (let i = active.length - 1; i >= n; i--)
      out += markTag(active[i]).close;
    active = active.slice(0, n);
  };
  parent.forEach((child) => {
    const marks = child.marks;
    let same = 0;
    while (
      same < active.length &&
      same < marks.length &&
      active[same].eq(marks[same])
    ) {
      same++;
    }
    closeTo(same);
    for (let i = same; i < marks.length; i++) {
      out += markTag(marks[i]).open;
      active.push(marks[i]);
    }
    if (child.isText) out += escText(child.text ?? "");
    else if (child.type.name === "image")
      out += openTag("img", {
        src: child.attrs.src as string,
        alt: child.attrs.alt as string,
      });
    else if (child.type.name === "hard_break") out += "<br>";
  });
  closeTo(0);
  return out;
}

/** A container whose only child is a paragraph collapses to one line. */
function singleParagraph(node: PMNode): PMNode | null {
  return node.childCount === 1 && node.firstChild?.type.name === "paragraph"
    ? node.firstChild
    : null;
}

function wrap(
  out: string[],
  ind: string,
  tag: string,
  attrs: Record<string, string | boolean | null | undefined> | undefined,
  node: PMNode,
) {
  out.push(`${ind}${openTag(tag, attrs)}`);
  node.forEach((child) => block(child, `${ind}  `, out));
  out.push(`${ind}</${tag}>`);
}

function listItem(node: PMNode, ind: string, out: string[]) {
  const only = singleParagraph(node);
  if (only) {
    out.push(`${ind}<li>${inline(only)}</li>`);
    return;
  }
  const first = node.firstChild;
  out.push(
    `${ind}<li>${first?.type.name === "paragraph" ? inline(first) : ""}`,
  );
  node.forEach((child, _off, i) => {
    if (i === 0 && child.type.name === "paragraph") return;
    block(child, `${ind}  `, out);
  });
  out.push(`${ind}</li>`);
}

/** Span attributes on a table cell (from prosemirror-tables), emitted only when
 * they deviate from the 1×1 default so plain cells stay noise-free. */
function cellAttrs(node: PMNode): Record<string, string | undefined> {
  const colspan = node.attrs.colspan as number;
  const rowspan = node.attrs.rowspan as number;
  return {
    colspan: colspan > 1 ? String(colspan) : undefined,
    rowspan: rowspan > 1 ? String(rowspan) : undefined,
  };
}

function cell(node: PMNode, ind: string, out: string[]) {
  const tag = node.type.name === "table_header" ? "th" : "td";
  const attrs = cellAttrs(node);
  const only = singleParagraph(node);
  if (only) {
    out.push(`${ind}${openTag(tag, attrs)}${inline(only)}</${tag}>`);
    return;
  }
  wrap(out, ind, tag, attrs, node);
}

function block(node: PMNode, ind: string, out: string[]) {
  const a = node.attrs;
  switch (node.type.name) {
    case "paragraph":
      out.push(`${ind}<p>${inline(node)}</p>`);
      break;
    case "heading": {
      const l = a.level as number;
      out.push(`${ind}<h${l}>${inline(node)}</h${l}>`);
      break;
    }
    case "code_block":
      // No indentation inside pre: content is verbatim
      out.push(`${ind}<pre><code>${escText(node.textContent)}</code></pre>`);
      break;
    case "horizontal_rule":
      out.push(`${ind}<hr>`);
      break;
    case "bullet_list":
      wrapList(out, ind, "ul", node);
      break;
    case "ordered_list":
      wrapList(out, ind, "ol", node);
      break;
    case "blockquote":
      wrap(out, ind, "blockquote", undefined, node);
      break;
    case "figure": {
      const img = node.firstChild;
      const imgTag = img
        ? openTag("img", {
            src: img.attrs.src as string,
            alt: img.attrs.alt as string,
          })
        : "";
      out.push(
        `${ind}${openTag("h-figure", {
          width: a.width as string,
          align: a.align as string,
        })}${imgTag}</h-figure>`,
      );
      break;
    }
    case "callout":
      wrap(
        out,
        ind,
        "h-callout",
        { kind: a.kind as string, title: a.title as string },
        node,
      );
      break;
    case "card":
      wrap(out, ind, "h-card", { title: a.title as string }, node);
      break;
    case "quote":
      wrap(out, ind, "h-quote", { cite: a.cite as string }, node);
      break;
    case "details":
      wrap(
        out,
        ind,
        "h-details",
        { summary: a.summary as string, open: a.open as boolean },
        node,
      );
      break;
    case "columns":
      wrap(out, ind, "h-columns", undefined, node);
      break;
    case "column":
      wrap(out, ind, "h-column", undefined, node);
      break;
    case "steps":
      wrap(out, ind, "h-steps", undefined, node);
      break;
    case "step":
      wrap(out, ind, "h-step", { title: a.title as string }, node);
      break;
    case "stats":
      wrap(out, ind, "h-stats", undefined, node);
      break;
    case "stat":
      out.push(
        `${ind}${openTag("h-stat", {
          value: a.value as string,
          label: a.label as string,
        })}</h-stat>`,
      );
      break;
    case "toc":
      out.push(`${ind}<h-toc></h-toc>`);
      break;
    case "table":
      wrap(out, ind, "table", undefined, node);
      break;
    case "table_row":
      out.push(`${ind}<tr>`);
      node.forEach((c) => cell(c, `${ind}  `, out));
      out.push(`${ind}</tr>`);
      break;
    default:
      // Unknown node types cannot occur (schema-enforced); keep the switch total.
      break;
  }
}

function wrapList(out: string[], ind: string, tag: string, node: PMNode) {
  out.push(`${ind}<${tag}>`);
  node.forEach((li) => listItem(li, `${ind}  `, out));
  out.push(`${ind}</${tag}>`);
}

export function serializeHdoc(doc: PMNode): string {
  const out: string[] = [];
  const layout = doc.attrs.layout as string;
  out.push(
    openTag("h-doc", {
      v: (doc.attrs.v as string) || "1",
      theme: doc.attrs.theme as string,
      // default layout stays implicit so plain documents carry no noise
      layout: layout === "article" ? "" : layout,
    }),
  );
  doc.forEach((child) => block(child, "  ", out));
  out.push("</h-doc>");
  return `${out.join("\n")}\n`;
}
