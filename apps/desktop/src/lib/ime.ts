import { useCallback, useRef } from "react";

/** IME-safe key gating for inputs that commit on Enter / cancel on Escape.
 *
 * While an IME is composing, Enter confirms the candidate word and Escape
 * dismisses it — neither belongs to the UI. A bare `isComposing` check is not
 * enough: WKWebView delivers the candidate-confirming key AFTER
 * compositionend with `isComposing` already false, so we also remember when
 * composition last ended and swallow keys landing hard on its heels (the same
 * trick the AI panel input uses).
 *
 * Usage: `const ime = useImeGuard()` — put `onCompositionEnd={ime.end}` on
 * the input and bail out of keydown handlers when `ime.guarded(e.nativeEvent)`.
 */
export function useImeGuard() {
  const endAt = useRef(0);
  const end = useCallback(() => {
    endAt.current = Date.now();
  }, []);
  const guarded = useCallback(
    (e: { isComposing?: boolean }) =>
      e.isComposing === true || Date.now() - endAt.current < 100,
    [],
  );
  return { end, guarded };
}
