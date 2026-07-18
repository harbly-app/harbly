/**
 * hdoc source text → PM document. Parsing is strict about the vocabulary:
 * any element outside the v1 whitelist — or vocabulary elements nested where
 * the schema cannot accept them — flips the result to "unsupported" and the
 * editor falls back to a read-only preview. A later save must never be able
 * to silently drop OR RESTRUCTURE content the editor didn't understand:
 * ProseMirror's DOM parser treats unknown elements as transparent, and on a
 * content-model mismatch it silently lifts/moves nodes into a valid shape
 * (a list hoisted out of a blockquote, a callout emptied) instead of
 * erroring, so both cases need explicit guards.
 */
import { DOMParser as PMDOMParser } from "prosemirror-model";
import type { Node as PMNode, NodeType } from "prosemirror-model";
import { ALLOWED_ATTRS, ALLOWED_TAGS, hdocSchema } from "./schema";

export type ParsedHdoc =
  | { ok: true; doc: PMNode }
  | { ok: false; reason: "no-root" | "unsupported"; tags?: string[] };

/** DOM tag → schema node for structure validation (marks/inline handled apart). */
const TAG_TYPE: Record<string, string | undefined> = {
  "h-doc": "doc",
  p: "paragraph",
  h1: "heading",
  h2: "heading",
  h3: "heading",
  ul: "bullet_list",
  ol: "ordered_list",
  li: "list_item",
  blockquote: "blockquote",
  pre: "code_block",
  hr: "horizontal_rule",
  img: "image",
  br: "hard_break",
  "h-figure": "figure",
  "h-callout": "callout",
  "h-card": "card",
  "h-quote": "quote",
  "h-details": "details",
  "h-columns": "columns",
  "h-column": "column",
  "h-steps": "steps",
  "h-step": "step",
  "h-stats": "stats",
  "h-stat": "stat",
  "h-toc": "toc",
  table: "table",
  tr: "table_row",
  th: "table_header",
  td: "table_cell",
};

/** Wrappers the HTML parser inserts around table rows; PM sees through them. */
const TRANSPARENT = new Set(["thead", "tbody"]);

/** Mark carriers + the <code> inside <pre> — inline content, never structure. */
const INLINE_TAGS = new Set([
  "a",
  "strong",
  "b",
  "em",
  "i",
  "s",
  "del",
  "code",
]);

/** The first vocabulary element whose CHILDREN the schema cannot accept where
 * they are — the spot where PM's parser would silently restructure instead of
 * erroring. Returns the offending container tag, or null when everything fits.
 *
 * Mid-sequence mismatches are the destructive ones (nodes get lifted out or
 * containers emptied); a merely INCOMPLETE tail (empty card, lone column) is
 * tolerated — PM fills those with empty required nodes without moving
 * anything, and loose text/inline in a block context is likewise only wrapped
 * in a paragraph in place. */
function misplacedIn(root: Element): string | null {
  const stack: Element[] = [root];
  for (let el = stack.pop(); el; el = stack.pop()) {
    const tag = el.tagName.toLowerCase();
    const typeName = TAG_TYPE[tag];
    if (typeName && !childrenFit(hdocSchema.nodes[typeName], el)) return tag;
    for (const c of Array.from(el.children)) stack.push(c);
  }
  return null;
}

/** True when an inline element's whole SUBTREE is inline content: mark tags
 * may nest marks, images and breaks, but never a block/component element —
 * PM would lift a smuggled `<strong><h-callout>…` out of the mark, which is
 * exactly the silent restructuring this validator exists to refuse. */
function inlineSubtreeOk(el: Element): boolean {
  const tag = el.tagName.toLowerCase();
  if (INLINE_TAGS.has(tag)) {
    return Array.from(el.children).every(inlineSubtreeOk);
  }
  const tn = TAG_TYPE[tag];
  return (
    tn !== undefined &&
    hdocSchema.nodes[tn].isInline &&
    el.children.length === 0
  );
}

/** Whether a text node holds content PM would keep. ASCII whitespace ONLY —
 * String.trim() also strips Unicode spaces (nbsp, ideographic space) that
 * ProseMirror treats as REAL content, and a node the validator waves through
 * as blank but PM cannot place in a strict container gets silently dropped. */
function hasRealText(n: Node): boolean {
  return /[^\t\n\f\r ]/.test(n.textContent ?? "");
}

function childrenFit(type: NodeType, el: Element): boolean {
  const { code_block, figure, paragraph: para } = hdocSchema.nodes;
  // Atoms (h-stat, h-toc, hr, img, br) hold nothing; any real content inside
  // would be moved out by the parser.
  if (type.isLeaf) {
    return Array.from(el.childNodes).every(
      (n) => n.nodeType === Node.TEXT_NODE && !hasRealText(n),
    );
  }
  // Code blocks are "text*" with NO marks: only the canonical <code> wrapper
  // (itself text-only) may appear — a <strong> would be silently unwrapped
  // and an <img>/<br> lifted out or dropped on the next save.
  if (type === code_block) {
    return Array.from(el.childNodes).every((n) => {
      if (n.nodeType !== Node.ELEMENT_NODE) return true;
      const c = n as Element;
      return (
        c.tagName.toLowerCase() === "code" &&
        Array.from(c.children).length === 0
      );
    });
  }
  // A figure holds exactly one image (empty = fill-tolerated): caption text
  // would be moved out into a sibling paragraph, a second image lifted out.
  if (type === figure) {
    const hasText = Array.from(el.childNodes).some(
      (n) => n.nodeType === Node.TEXT_NODE && hasRealText(n),
    );
    const kids = Array.from(el.children);
    return (
      !hasText &&
      kids.length <= 1 &&
      kids.every((c) => c.tagName.toLowerCase() === "img")
    );
  }
  // Textblocks (p, headings): children must be inline all the way down — a
  // block element smuggled inside (HTML parsing does NOT auto-close <p>
  // before custom elements), even wrapped in a mark tag, would be split out.
  if (type.inlineContent) {
    return Array.from(el.children).every(inlineSubtreeOk);
  }
  // Block containers: replay the child sequence against the schema's own
  // content model. Loose text/inline children stand in as the paragraph the
  // parser would wrap them into — provided their subtree really is inline.
  const seq: NodeType[] = [];
  for (const n of Array.from(el.childNodes)) {
    if (n.nodeType === Node.TEXT_NODE) {
      if (hasRealText(n)) seq.push(para);
      continue;
    }
    if (n.nodeType !== Node.ELEMENT_NODE) continue;
    const c = n as Element;
    const t = c.tagName.toLowerCase();
    if (TRANSPARENT.has(t)) {
      for (const r of Array.from(c.children)) {
        const rt = TAG_TYPE[r.tagName.toLowerCase()];
        seq.push(rt ? hdocSchema.nodes[rt] : para);
      }
      continue;
    }
    if (INLINE_TAGS.has(t)) {
      if (!inlineSubtreeOk(c)) return false;
      seq.push(para);
      continue;
    }
    const tn = TAG_TYPE[t];
    if (!tn) return false; // unknown tags are rejected before this runs
    const nt = hdocSchema.nodes[tn];
    seq.push(nt.isInline ? para : nt);
  }
  let m = type.contentMatch;
  for (const nt of seq) {
    const next = m.matchType(nt);
    if (!next) return false;
    m = next;
  }
  return true;
}

/** Mirror of the Rust validator's URL check (hdoc.rs safe_url): http(s)/
 * mailto, #fragments and relative paths; img src additionally data:image/*.
 * Anything else (javascript:, non-image data:, unknown schemes) must flip the
 * editor to readonly — export refuses such files, and the editor must not
 * autosave them onward as if it understood them. */
function safeUrl(value: string, allowDataImage: boolean): boolean {
  // Strip ASCII whitespace and control characters (0x00-0x20 + DEL) before
  // reading the scheme - HTML URL parsing tolerates them inside it
  // ("java\tscript:" still executes). ASCII-ONLY on purpose, exactly matching
  // the Rust safe_url (is_ascii_whitespace/is_ascii_control): JS \s would
  // also strip Unicode spaces that browsers do NOT strip, and the two
  // validators must agree on every input.
  // eslint-disable-next-line no-control-regex
  const normalized = value.replace(/[\u0000-\u0020\u007f]+/g, "").toLowerCase();
  const colon = normalized.indexOf(":");
  if (colon === -1) return true;
  if (/[/?#]/.test(normalized.slice(0, colon))) return true;
  const scheme = normalized.slice(0, colon);
  if (scheme === "http" || scheme === "https" || scheme === "mailto")
    return true;
  return (
    allowDataImage &&
    scheme === "data" &&
    normalized.slice(colon + 1).startsWith("image/")
  );
}

export function parseHdoc(text: string): ParsedHdoc {
  const dom = new window.DOMParser().parseFromString(text, "text/html");
  const root = dom.body.querySelector("h-doc");
  if (!root) return { ok: false, reason: "no-root" };

  const bad = new Set<string>();
  const walker = dom.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  for (let el = walker.nextNode(); el; el = walker.nextNode()) {
    const element = el as Element;
    const tag = element.tagName.toLowerCase();
    if (!ALLOWED_TAGS.has(tag)) {
      bad.add(tag);
      continue;
    }
    const allowed = ALLOWED_ATTRS.get(tag);
    for (const name of element.getAttributeNames()) {
      const attr = name.toLowerCase();
      if (!allowed?.has(attr)) bad.add(`${tag}[${attr}]`);
    }
    // The schema's only link rule is a[href]: a href-LESS <a> parses
    // transparently (its text survives, the element vanishes on save), so it
    // must flip to readonly like any content the editor can't represent.
    if (tag === "a" && !element.hasAttribute("href")) bad.add("a[href]");
    if (tag === "a" && !safeUrl(element.getAttribute("href") ?? "", false))
      bad.add("a[href]");
    if (tag === "img" && !safeUrl(element.getAttribute("src") ?? "", true))
      bad.add("img[src]");
  }
  if (bad.size > 0) {
    return { ok: false, reason: "unsupported", tags: [...bad] };
  }

  const misplaced = misplacedIn(root);
  if (misplaced !== null) {
    return { ok: false, reason: "unsupported", tags: [misplaced] };
  }

  // Unknown theme/layout values are preserved as-is (forward compatibility):
  // the CSS simply falls back to the default rendering.
  const attrs = {
    theme: root.getAttribute("theme") ?? "paper",
    v: root.getAttribute("v") ?? "1",
    layout: root.getAttribute("layout") ?? "article",
  };
  try {
    const doc = PMDOMParser.fromSchema(hdocSchema).parse(root, {
      topNode: hdocSchema.topNodeType.create(attrs),
    });
    if (doc.childCount === 0) {
      return {
        ok: true,
        doc:
          hdocSchema.topNodeType.createAndFill(attrs) ??
          hdocSchema.topNodeType.create(
            attrs,
            hdocSchema.nodes.paragraph.create(),
          ),
      };
    }
    return { ok: true, doc };
  } catch {
    // Structurally unparseable under the schema (e.g. a component in an
    // impossible position) — same protection as unknown tags.
    return { ok: false, reason: "unsupported" };
  }
}
