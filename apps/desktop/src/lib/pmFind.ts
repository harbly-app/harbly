/**
 * In-document find for the ProseMirror editors (hdoc + Markdown/Crepe): a
 * decoration plugin plus run/step/clear commands driven by the FindBar via
 * the registered FindHandle. Matching is a case-insensitive substring search
 * per textblock, so a match may span mark boundaries (bold/italic splits)
 * but never a block boundary — same behavior as browser find on a page.
 */
import { Plugin, PluginKey } from "prosemirror-state";
import type { EditorState } from "prosemirror-state";
import type { Node as PMNode } from "prosemirror-model";
import { Decoration, DecorationSet } from "prosemirror-view";
import type { EditorView } from "prosemirror-view";

export interface FindRange {
  from: number;
  to: number;
}

interface FindState {
  ranges: FindRange[];
  active: number;
}

const findKey = new PluginKey<FindState | null>("harblyFind");

export function findPlugin(): Plugin {
  return new Plugin({
    key: findKey,
    state: {
      init: () => null,
      apply(tr, prev): FindState | null {
        const meta = tr.getMeta(findKey) as FindState | null | undefined;
        if (meta !== undefined) return meta;
        if (!prev || !tr.docChanged) return prev;
        // Keep highlights roughly in place while the user edits; the FindBar
        // re-runs the query on its own debounce anyway.
        const ranges = prev.ranges
          .map((r) => ({
            from: tr.mapping.map(r.from),
            to: tr.mapping.map(r.to),
          }))
          .filter((r) => r.to > r.from);
        return ranges.length
          ? { ranges, active: Math.min(prev.active, ranges.length - 1) }
          : null;
      },
    },
    props: {
      decorations(state) {
        const s = findKey.getState(state);
        if (!s || s.ranges.length === 0) return null;
        return DecorationSet.create(
          state.doc,
          s.ranges.map((r, i) =>
            Decoration.inline(r.from, r.to, {
              class:
                i === s.active
                  ? "pm-find-match pm-find-active"
                  : "pm-find-match",
            }),
          ),
        );
      },
    },
  });
}

/** All match ranges of `q` in the document (case-insensitive). Exported for
 * tests. Inline leaf nodes (images, hard breaks) occupy one position and are
 * stood in by U+0000 so character offsets stay aligned with positions. */
export function collectMatches(doc: PMNode, q: string): FindRange[] {
  const ranges: FindRange[] = [];
  const ql = q.toLowerCase();
  if (!ql) return ranges;
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    const text = node
      .textBetween(0, node.content.size, undefined, "\u0000")
      .toLowerCase();
    let i = 0;
    for (;;) {
      i = text.indexOf(ql, i);
      if (i === -1) break;
      ranges.push({ from: pos + 1 + i, to: pos + 1 + i + ql.length });
      i += ql.length;
    }
    return false;
  });
  return ranges;
}

function scrollToActive(view: EditorView) {
  // The decoration lands in the DOM after this dispatch's render pass.
  requestAnimationFrame(() => {
    view.dom
      .querySelector(".pm-find-active")
      ?.scrollIntoView({ block: "center" });
  });
}

export interface FindResult {
  count: number;
  active: number; // 1-based; 0 = none
}

export function runFind(view: EditorView, q: string): FindResult {
  const ranges = collectMatches(view.state.doc, q);
  view.dispatch(
    view.state.tr.setMeta(
      findKey,
      ranges.length ? { ranges, active: 0 } : null,
    ),
  );
  if (ranges.length) scrollToActive(view);
  return { count: ranges.length, active: ranges.length ? 1 : 0 };
}

export function stepFind(view: EditorView, delta: 1 | -1): FindResult {
  const s = findKey.getState(view.state);
  if (!s || s.ranges.length === 0) return { count: 0, active: 0 };
  const active = (s.active + delta + s.ranges.length) % s.ranges.length;
  view.dispatch(view.state.tr.setMeta(findKey, { ranges: s.ranges, active }));
  scrollToActive(view);
  return { count: s.ranges.length, active: active + 1 };
}

export function clearFind(view: EditorView) {
  const s = findKey.getState(view.state);
  if (s) view.dispatch(view.state.tr.setMeta(findKey, null));
}

/** Helper for `EditorState`-level checks in tests. */
export function findStateOf(state: EditorState): FindState | null {
  return findKey.getState(state) ?? null;
}
