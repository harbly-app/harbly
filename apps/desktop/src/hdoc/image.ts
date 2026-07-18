/**
 * All image ingestion for the hdoc editor lives here — paste, drop, picker.
 *
 * Local images become self-contained `data:` URLs so an hdoc stays a single
 * portable file: export, browser preview and thumbnails all work with no
 * sidecar files and no relative-path rewriting, and the CSP already allows
 * `data:` images. Oversized raster images are scaled down to keep the file
 * reasonable; SVG and within-bounds images are embedded byte-for-byte.
 *
 * Paste is owned entirely at the DOM paste event (`handleEditorPaste`), which
 * every paste trigger converges on — ⌘V (menu-forwarded `paste:`), the
 * webview's right-click → Paste, and a plain browser paste alike:
 * - Image exposed to JS (`clipboardData` file): embed it, one figure.
 * - Raw image on the macOS clipboard: WKWebView exposes nothing to JS and its
 *   default paste inserts one opaque `webkit-fake-url:` <img> per clipboard
 *   format (PNG + TIFF + …) — broken on reload and multiplied. Detected as an
 *   image-only paste, the event is cancelled and exactly one image is read
 *   from NSPasteboard over IPC instead.
 * - Anything else falls through to the normal text/rich paste, where
 *   `stripOpaqueImages` (transformPasted) drops `webkit-fake-url:`/`blob:`
 *   images that could never survive a save.
 */
import type { Node as PMNode } from "prosemirror-model";
import { Fragment, Slice } from "prosemirror-model";
import { TextSelection } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { api } from "../lib/api";
import { hdocSchema } from "./schema";

/** Cap the longest edge of an embedded raster image (retina-friendly for a
 * document, but far below multi-megapixel camera originals). */
const MAX_DIM = 1600;

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result as string);
    r.onerror = () => reject(r.error ?? new Error("read failed"));
    r.readAsDataURL(file);
  });
}

function loadImage(file: Blob): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      resolve(img);
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("decode failed"));
    };
    img.src = url;
  });
}

/** File → `data:` URL. SVG and images already within MAX_DIM embed verbatim
 * (lossless); larger raster images are scaled down and re-encoded — PNG stays
 * PNG (crisp text/UI, preserves alpha), JPEG stays JPEG. */
export async function imageToDataUrl(file: File): Promise<string> {
  if (file.type === "image/svg+xml") return readAsDataUrl(file);
  const img = await loadImage(file);
  const longest = Math.max(img.naturalWidth, img.naturalHeight);
  if (longest <= MAX_DIM) return readAsDataUrl(file);
  const scale = MAX_DIM / longest;
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(img.naturalWidth * scale);
  canvas.height = Math.round(img.naturalHeight * scale);
  const ctx = canvas.getContext("2d");
  if (!ctx) return readAsDataUrl(file);
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  return file.type === "image/jpeg"
    ? canvas.toDataURL("image/jpeg", 0.85)
    : canvas.toDataURL("image/png");
}

/** Open a transient native file picker; resolves null if the user cancels. */
export function pickImageFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.style.display = "none";
    const done = (f: File | null) => {
      input.remove();
      resolve(f);
    };
    input.addEventListener("change", () => done(input.files?.[0] ?? null));
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** Embed dropped/pasted image files as figures. Images are converted first,
 * then inserted in a single transaction (one undo step); non-images are
 * ignored. When `pos` is given (a drop point) insertion happens there,
 * otherwise at the current selection. */
export async function insertImageFiles(
  view: EditorView,
  files: File[],
  pos?: number,
): Promise<void> {
  const images = files.filter((f) => f.type.startsWith("image/"));
  if (images.length === 0) return;
  const urls = (
    await Promise.all(images.map((f) => imageToDataUrl(f).catch(() => null)))
  ).filter((u): u is string => u !== null);
  if (urls.length === 0) return;
  // The editor may have been torn down while we were decoding.
  if (!view.dom.isConnected) return;
  const { figure, image } = hdocSchema.nodes;
  let tr = view.state.tr;
  if (pos !== undefined) {
    tr = tr.setSelection(TextSelection.near(tr.doc.resolve(pos)));
  }
  for (const src of urls) {
    tr = tr.replaceSelectionWith(figure.create(null, image.create({ src })));
  }
  view.dispatch(tr.scrollIntoView());
  view.focus();
}

/** Insert a single already-resolved image URL (e.g. a data: URL read from the
 * native clipboard) as a figure at the current selection. */
export function insertImageSrc(view: EditorView, src: string) {
  if (!view.dom.isConnected) return;
  const { figure, image } = hdocSchema.nodes;
  const node = figure.create(null, image.create({ src }));
  view.dispatch(view.state.tr.replaceSelectionWith(node).scrollIntoView());
  view.focus();
}

/** First image the clipboard exposes to JS — a paste is one logical image even
 * when the clipboard carries several formats of it. */
function firstImageFile(cd: DataTransfer): File | null {
  const fromFiles = Array.from(cd.files).find((f) =>
    f.type.startsWith("image/"),
  );
  if (fromFiles) return fromFiles;
  const item = Array.from(cd.items).find(
    (it) => it.kind === "file" && it.type.startsWith("image/"),
  );
  return item?.getAsFile() ?? null;
}

/** A paste with no JS-readable text is an image paste (or an empty clipboard,
 * where inserting nothing is the right outcome anyway). Rich pastes that mix
 * text with images keep their text path. Exported for tests. */
export function isImageOnlyPaste(cd: DataTransfer): boolean {
  if (cd.getData("text/plain").trim()) return false;
  const html = cd.getData("text/html");
  if (!html) return true;
  // A ProseMirror-authored fragment — the editor's own copy of textless
  // blocks (a figure, hr, toc, an empty table/stats) — is a structural paste,
  // not an image paste: PM's parser must receive it. NSPasteboard holds no
  // bitmap for it, so routing it to readClipboardImage would swallow the
  // paste into a silent no-op.
  if (html.includes("data-pm-slice")) return false;
  // DOMParser stays inert (no image fetches), unlike innerHTML on a live node.
  const doc = new DOMParser().parseFromString(html, "text/html");
  return !doc.body.textContent.trim();
}

/** The editor's ProseMirror `handlePaste`. Returning true cancels the
 * webview's default insertion — which is what multiplies a native image paste
 * into one <img> per clipboard format. */
export function handleEditorPaste(
  view: EditorView,
  event: ClipboardEvent,
): boolean {
  const cd = event.clipboardData;
  if (!cd) return false;
  const file = firstImageFile(cd);
  if (file) {
    event.preventDefault();
    void insertImageFiles(view, [file]);
    return true;
  }
  if (!isImageOnlyPaste(cd)) return false;
  event.preventDefault();
  api
    .readClipboardImage()
    .then((url) => {
      if (url) insertImageSrc(view, url);
      // null = clipboard holds no image → nothing to paste, like the platform.
    })
    .catch((e: unknown) => {
      // Outside Tauri (browser harness) or a stale dev binary: swallow rather
      // than let the broken multi-image default run.
      console.warn("clipboard image read failed", e);
    });
  return true;
}

const OPAQUE_SRC = /^(webkit-fake-url|blob):/i;

const isOpaqueImage = (n: PMNode) =>
  n.type === hdocSchema.nodes.image && OPAQUE_SRC.test(n.attrs.src as string);

/** transformPasted: drop pasted images whose src is session-local
 * (`webkit-fake-url:` from WKWebView, `blob:`) — they render once and break on
 * reload, so they must never enter the document. A figure wrapping one loses
 * its required child and is dropped whole. Text and real srcs pass through. */
export function stripOpaqueImages(slice: Slice): Slice {
  // Object ref: TS narrowing can't see the closure mutate a plain `let`.
  const dropped = { v: false };
  const clean = (frag: Fragment): Fragment => {
    const out: PMNode[] = [];
    frag.forEach((child) => {
      if (
        isOpaqueImage(child) ||
        (child.type === hdocSchema.nodes.figure &&
          child.content.firstChild !== null &&
          isOpaqueImage(child.content.firstChild))
      ) {
        dropped.v = true;
        return;
      }
      out.push(
        child.isText || child.isLeaf ? child : child.copy(clean(child.content)),
      );
    });
    return Fragment.from(out);
  };
  const content = clean(slice.content);
  // maxOpen recomputes valid open depths — dropping a node at a slice edge
  // could otherwise leave openStart/openEnd pointing past the content.
  return dropped.v ? Slice.maxOpen(content) : slice;
}
