/**
 * Every catalog item must visibly change the document when activated — a
 * palette/slash click that silently does nothing is a bug (this originally
 * caught list-wrap commands no-oping inside an empty heading).
 */
import { expect, it } from "vitest";
import { EditorState, TextSelection } from "prosemirror-state";
import type { Transaction } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { hdocItems } from "./items";
import { parseHdoc } from "./parse";
import { serializeHdoc } from "./serialize";

function fakeView(state: EditorState): EditorView {
  const v = {
    state,
    focus: () => undefined,
    dispatch(tr: Transaction) {
      v.state = v.state.apply(tr);
    },
  };
  return v as unknown as EditorView;
}

it("every catalog item changes the document from both skeleton positions", () => {
  const failures: string[] = [];
  for (const item of hdocItems()) {
    for (const cursor of ["h1", "p"] as const) {
      const parsed = parseHdoc(
        `<h-doc v="1">\n  <h1></h1>\n  <p></p>\n</h-doc>\n`,
      );
      if (!parsed.ok) throw new Error("skeleton parse failed");
      let state = EditorState.create({ doc: parsed.doc });
      if (cursor === "p") {
        const first = parsed.doc.firstChild;
        if (!first) throw new Error("empty skeleton");
        state = state.apply(
          state.tr.setSelection(
            TextSelection.create(state.doc, first.nodeSize + 1),
          ),
        );
      }
      const v = fakeView(state);
      const before = serializeHdoc(v.state.doc);
      try {
        item.run(v);
      } catch (e) {
        failures.push(`${item.key}@${cursor}: threw ${String(e)}`);
        continue;
      }
      if (serializeHdoc(v.state.doc) === before) {
        failures.push(`${item.key}@${cursor}: no-op`);
      }
      // Whatever was inserted must survive a save/load cycle
      const round = parseHdoc(serializeHdoc(v.state.doc));
      if (!round.ok) failures.push(`${item.key}@${cursor}: broke round-trip`);
    }
  }
  expect(failures).toEqual([]);
});
