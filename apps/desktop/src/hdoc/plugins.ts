/**
 * Editor plumbing: markdown-style input rules ("## " → heading, "**bold**"),
 * keymaps (lists, tables, heading-exit), and empty-document placeholders.
 * No Mod-B/Mod-I mark shortcuts: ⌘B/⌘K/⌘J are app-level shortcuts (sidebar /
 * palette / AI panel), matching the Markdown editor's behavior.
 */
import { chainCommands } from "prosemirror-commands";
import type { Command } from "prosemirror-state";
import {
  InputRule,
  inputRules,
  textblockTypeInputRule,
  undoInputRule,
  wrappingInputRule,
} from "prosemirror-inputrules";
import { keymap } from "prosemirror-keymap";
import type { MarkType } from "prosemirror-model";
import {
  liftListItem,
  sinkListItem,
  splitListItem,
} from "prosemirror-schema-list";
import { Plugin, TextSelection } from "prosemirror-state";
import { goToNextCell } from "prosemirror-tables";
import { Decoration, DecorationSet } from "prosemirror-view";
import { tr } from "../lib/i18n";
import { hdocSchema } from "./schema";

const n = hdocSchema.nodes;

/** "**bold**" / "*em*" / "`code`" / "~~strike~~" as you type. Written without
 * lookbehind (older WKWebView compatibility), so the match may include one
 * leading context character that must be preserved — `delim` is the delimiter
 * length, from which the markup's true start inside the match is derived. */
function markRule(
  regexp: RegExp,
  markType: MarkType,
  delim: number,
): InputRule {
  return new InputRule(regexp, (state, match, start, end) => {
    const content = match[1];
    if (!content) return null;
    const markupLen = content.length + delim * 2;
    const from = start + (match[0].length - markupLen);
    const t = state.tr;
    t.replaceWith(from, end, hdocSchema.text(content, [markType.create()]));
    t.removeStoredMark(markType);
    return t;
  });
}

export function hdocInputRules(): Plugin {
  return inputRules({
    rules: [
      textblockTypeInputRule(/^(#{1,3})\s$/, n.heading, (m) => ({
        level: m[1].length,
      })),
      wrappingInputRule(/^\s*[-*]\s$/, n.bullet_list),
      wrappingInputRule(/^1[.)]\s$/, n.ordered_list),
      wrappingInputRule(/^>\s$/, n.blockquote),
      textblockTypeInputRule(/^```$/, n.code_block),
      new InputRule(/^(?:---|\*\*\*)$/, (state, _m, start, end) => {
        return state.tr.replaceRangeWith(
          start,
          end,
          n.horizontal_rule.create(),
        );
      }),
      markRule(/(?:^|[^*])\*\*([^*]+)\*\*$/, hdocSchema.marks.strong, 2),
      markRule(/(?:^|[^*])\*([^*\s][^*]*)\*$/, hdocSchema.marks.em, 1),
      markRule(/(?:^|[^`])`([^`]+)`$/, hdocSchema.marks.code, 1),
      markRule(/~~([^~]+)~~$/, hdocSchema.marks.strike, 2),
    ],
  });
}

/** Enter at the end of a heading starts a paragraph (Notion/Typora behavior). */
const exitHeading: Command = (state, dispatch) => {
  const { $from, empty } = state.selection;
  if (
    !empty ||
    $from.parent.type !== n.heading ||
    $from.parentOffset !== $from.parent.content.size ||
    $from.parent.content.size === 0
  ) {
    return false;
  }
  if (dispatch) {
    const pos = $from.after();
    const t = state.tr.insert(pos, n.paragraph.create());
    t.setSelection(TextSelection.create(t.doc, pos + 1));
    dispatch(t.scrollIntoView());
  }
  return true;
};

export function hdocKeymap(): Plugin {
  return keymap({
    Enter: chainCommands(splitListItem(n.list_item), exitHeading),
    Tab: chainCommands(goToNextCell(1), sinkListItem(n.list_item), () => true),
    "Shift-Tab": chainCommands(
      goToNextCell(-1),
      liftListItem(n.list_item),
      () => true,
    ),
    Backspace: undoInputRule,
  });
}

/** Ghost text on the empty skeleton: title in the empty leading <h1>, body
 * hint on the empty paragraph that follows it. */
export function hdocPlaceholders(): Plugin {
  return new Plugin({
    props: {
      decorations(state) {
        const decos: Decoration[] = [];
        const first = state.doc.firstChild;
        if (
          first?.type === n.heading &&
          (first.attrs.level as number) === 1 &&
          first.content.size === 0
        ) {
          decos.push(
            Decoration.node(0, first.nodeSize, {
              class: "hd-ph",
              "data-ph": tr("hdocTitlePlaceholder"),
            }),
          );
          const second = state.doc.maybeChild(1);
          if (
            state.doc.childCount === 2 &&
            second?.type === n.paragraph &&
            second.content.size === 0
          ) {
            decos.push(
              Decoration.node(
                first.nodeSize,
                first.nodeSize + second.nodeSize,
                {
                  class: "hd-ph",
                  "data-ph": tr("mdPlaceholder"),
                },
              ),
            );
          }
        }
        return DecorationSet.create(state.doc, decos);
      },
    },
  });
}
