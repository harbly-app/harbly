/**
 * hdoc source text → PM document. Parsing is strict about the vocabulary:
 * any element outside the v1 whitelist flips the result to "unsupported" and
 * the editor falls back to a read-only preview — a later save must never be
 * able to silently drop content the editor didn't understand. (ProseMirror's
 * DOM parser treats unknown elements as transparent, so without this guard
 * their children would survive but the elements themselves would vanish.)
 */
import { DOMParser as PMDOMParser } from "prosemirror-model";
import type { Node as PMNode } from "prosemirror-model";
import { ALLOWED_TAGS, hdocSchema } from "./schema";

export type ParsedHdoc =
  | { ok: true; doc: PMNode }
  | { ok: false; reason: "no-root" | "unsupported"; tags?: string[] };

export function parseHdoc(text: string): ParsedHdoc {
  const dom = new window.DOMParser().parseFromString(text, "text/html");
  const root = dom.body.querySelector("h-doc");
  if (!root) return { ok: false, reason: "no-root" };

  const bad = new Set<string>();
  const walker = dom.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  for (let el = walker.nextNode(); el; el = walker.nextNode()) {
    const tag = (el as Element).tagName.toLowerCase();
    if (!ALLOWED_TAGS.has(tag)) bad.add(tag);
  }
  if (bad.size > 0) {
    return { ok: false, reason: "unsupported", tags: [...bad] };
  }

  // Unknown theme values are preserved as-is (forward compatibility): the CSS
  // simply falls back to the default token set.
  const attrs = {
    theme: root.getAttribute("theme") ?? "paper",
    v: root.getAttribute("v") ?? "1",
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
