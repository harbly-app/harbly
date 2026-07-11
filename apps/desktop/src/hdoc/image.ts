/**
 * Local images become self-contained `data:` URLs so an hdoc stays a single
 * portable file: export, browser preview and thumbnails all work with no
 * sidecar files and no relative-path rewriting, and the CSP already allows
 * `data:` images. Oversized raster images are scaled down to keep the file
 * reasonable; SVG and within-bounds images are embedded byte-for-byte.
 */
import { TextSelection } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
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
