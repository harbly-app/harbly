/**
 * Image-loss guards for the Markdown editor (Milkdown Crepe).
 *
 * Three leaks are closed together (the first in MarkdownEditor's feature
 * config, the last two here):
 * - JS-visible image files (paste/drop/upload button) go through Crepe's
 *   `onUpload`; without a config it stores `URL.createObjectURL` blob: URLs
 *   that die with the session — configured to embed as data: URLs instead.
 * - A raw image on the macOS clipboard (screenshots) is invisible to JS in
 *   WKWebView, and the default paste inserts session-local
 *   `webkit-fake-url:` <img>s. The plugin below detects the image-only paste
 *   and reads the bitmap from NSPasteboard over IPC instead (same strategy
 *   as the hdoc editor).
 * - Anything that still slips through as a doomed src (mixed rich pastes)
 *   is stripped from the serialized Markdown at save time, so a broken
 *   reference is never persisted as if it were content.
 */
import { Plugin } from "@milkdown/kit/prose/state";
import type { NodeType } from "@milkdown/kit/prose/model";
import type { EditorView } from "@milkdown/kit/prose/view";
import { isImageOnlyPaste } from "../hdoc/image";
import { api } from "./api";

/** `![alt](webkit-fake-url:…)` / `![alt](blob:…)` image references — already
 * unrecoverable (the bytes lived only in the dead session), so persisting
 * them would only fake content that renders broken on every future open. */
const DOOMED_IMAGE_REF =
  /!\[[^\]]*\]\(\s*<?\s*(?:webkit-fake-url|blob):[^)]*\)/gi;

export function stripDoomedImageRefs(markdown: string): string {
  return markdown.replace(DOOMED_IMAGE_REF, "");
}

function insertPastedImage(view: EditorView, src: string) {
  if (!view.dom.isConnected) return;
  const nodes: Record<string, NodeType | undefined> = view.state.schema.nodes;
  const type = nodes["image-block"] ?? nodes.image;
  const node = type?.createAndFill({ src });
  if (!node) return;
  view.dispatch(view.state.tr.replaceSelectionWith(node).scrollIntoView());
  view.focus();
}

/** ProseMirror plugin registered after Crepe's own clipboard/upload plugins:
 * both decline an image-only paste that exposes no file to JS (no text, no
 * files), which is exactly the WKWebView raw-bitmap case this one owns. */
export function mdNativeImagePastePlugin(): Plugin {
  return new Plugin({
    props: {
      handlePaste: (view, event) => {
        const cd = event.clipboardData;
        if (!cd) return false;
        const hasJsImageFile =
          Array.from(cd.files).some((f) => f.type.startsWith("image/")) ||
          Array.from(cd.items).some(
            (it) => it.kind === "file" && it.type.startsWith("image/"),
          );
        // Crepe's upload plugin (with onUpload configured) owns real files.
        if (hasJsImageFile) return false;
        if (!isImageOnlyPaste(cd)) return false;
        event.preventDefault();
        api
          .readClipboardImage()
          .then((url) => {
            // null = clipboard holds no image → nothing to paste.
            if (url) insertPastedImage(view, url);
          })
          .catch((e: unknown) => {
            // Outside Tauri (browser harness) or a stale dev binary: swallow
            // rather than let the broken multi-image default insertion run.
            console.warn("clipboard image read failed", e);
          });
        return true;
      },
    },
  });
}
