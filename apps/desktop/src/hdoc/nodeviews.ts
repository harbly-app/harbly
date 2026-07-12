/**
 * NodeViews for the component vocabulary. Each view's outer DOM is the real
 * custom element (h-callout, h-card, …) so the shared runtime CSS styles the
 * editor exactly like the rendered page; attribute text (titles, captions,
 * stat values) is edited through small inline inputs that live outside the
 * contentEditable flow. The attr-driven ::before/::after pseudo content from
 * the runtime CSS is suppressed in the editor (see styles.css) because the
 * inputs take its place.
 */
import type { Node as PMNode } from "prosemirror-model";
import type {
  EditorView,
  NodeView,
  ViewMutationRecord,
} from "prosemirror-view";
import { relAssetUrl } from "../lib/api";
import { tr } from "../lib/i18n";
import { imageToDataUrl, pickImageFile } from "./image";

type GetPos = () => number | undefined;

interface Field {
  attr: string;
  /** i18n key for the placeholder */
  ph: string;
  className?: string;
  /** render as a <select> with these values instead of a text input */
  options?: string[];
  /** i18n key prefix for option labels (key = prefix + value) */
  optionLabel?: (v: string) => string;
}

/** Reflect string/boolean node attrs onto the element so the runtime CSS
 * (kind colors, open state …) applies inside the editor too. */
function reflectAttrs(dom: HTMLElement, node: PMNode) {
  for (const [k, v] of Object.entries(node.attrs)) {
    if (typeof v === "string") {
      if (v !== "") dom.setAttribute(k, v);
      else dom.removeAttribute(k);
    } else if (typeof v === "boolean") {
      if (v) dom.setAttribute(k, "");
      else dom.removeAttribute(k);
    }
  }
}

class ComponentView implements NodeView {
  dom: HTMLElement;
  contentDOM?: HTMLElement;
  protected node: PMNode;
  protected controls = new Map<string, HTMLInputElement | HTMLSelectElement>();

  constructor(
    tag: string,
    fields: Field[],
    node: PMNode,
    view: EditorView,
    getPos: GetPos,
    opts: { leaf?: boolean } = {},
  ) {
    this.node = node;
    this.dom = document.createElement(tag);
    reflectAttrs(this.dom, node);

    if (fields.length > 0) {
      const head = document.createElement("div");
      head.className = "hd-fields";
      head.contentEditable = "false";
      for (const f of fields) {
        let el: HTMLInputElement | HTMLSelectElement;
        if (f.options) {
          const sel = document.createElement("select");
          for (const v of f.options) {
            const o = document.createElement("option");
            o.value = v;
            o.textContent = f.optionLabel ? f.optionLabel(v) : v;
            sel.appendChild(o);
          }
          sel.value = String(node.attrs[f.attr] ?? "");
          sel.addEventListener("change", () =>
            this.setAttr(view, getPos, f.attr, sel.value),
          );
          el = sel;
        } else {
          const input = document.createElement("input");
          input.type = "text";
          input.placeholder = tr(f.ph);
          input.value = String(node.attrs[f.attr] ?? "");
          input.addEventListener("change", () =>
            this.setAttr(view, getPos, f.attr, input.value),
          );
          input.addEventListener("blur", () =>
            this.setAttr(view, getPos, f.attr, input.value),
          );
          input.addEventListener("keydown", (e) => {
            if (e.key === "Enter" || e.key === "Escape") {
              e.preventDefault();
              input.blur();
              if (e.key === "Enter") view.focus();
            }
          });
          el = input;
        }
        el.className = `hd-attr ${f.className ?? ""}`.trim();
        this.controls.set(f.attr, el);
        head.appendChild(el);
      }
      this.dom.appendChild(head);
    }

    if (!opts.leaf) {
      const body = document.createElement("div");
      body.className = "hd-body";
      this.dom.appendChild(body);
      this.contentDOM = body;
    }
  }

  protected setAttr(
    view: EditorView,
    getPos: GetPos,
    attr: string,
    value: string,
  ) {
    const pos = getPos();
    if (pos === undefined) return;
    if (this.node.attrs[attr] === value) return;
    view.dispatch(
      view.state.tr.setNodeMarkup(pos, null, {
        ...this.node.attrs,
        [attr]: value,
      }),
    );
  }

  update(node: PMNode): boolean {
    if (node.type !== this.node.type) return false;
    this.node = node;
    reflectAttrs(this.dom, node);
    for (const [attr, el] of this.controls) {
      const v = String(node.attrs[attr] ?? "");
      if (document.activeElement !== el && el.value !== v) el.value = v;
    }
    return true;
  }

  stopEvent(e: Event): boolean {
    return (
      e.target instanceof HTMLElement && e.target.closest(".hd-fields") !== null
    );
  }

  ignoreMutation(m: ViewMutationRecord): boolean {
    if (m.type === "selection") return this.contentDOM === undefined;
    return !this.contentDOM?.contains(m.target);
  }
}

const FIG_MIN_PCT = 10;

const ALIGN_ICONS: Record<string, string> = {
  left: '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="3" y1="5" x2="21" y2="5"/><rect x="3" y="9" width="11" height="10" rx="1"/><line x1="3" y1="19" x2="21" y2="19"/></svg>',
  center:
    '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="3" y1="5" x2="21" y2="5"/><rect x="6.5" y="9" width="11" height="10" rx="1"/><line x1="3" y1="19" x2="21" y2="19"/></svg>',
  right:
    '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="3" y1="5" x2="21" y2="5"/><rect x="10" y="9" width="11" height="10" rx="1"/><line x1="3" y1="19" x2="21" y2="19"/></svg>',
};

/** h-figure: image preview with editor-only chrome — resize handles on the
 * image edges and a hover toolbar (align left/center/right + 1:1 natural
 * size). Sizing is stored on the figure node: width = % of the text column
 * ("" = natural), align = "left" | "right" ("" = centered). The src input
 * edits the child image node (position = own pos + 1) and only shows for
 * external URLs; embedded data: images hide it. */
class FigureView extends ComponentView {
  private pickBtn!: HTMLButtonElement;
  private toolBtns = new Map<string, HTMLButtonElement>();
  private handles: HTMLDivElement[] = [];
  private tools!: HTMLDivElement;

  constructor(node: PMNode, view: EditorView, getPos: GetPos) {
    super(
      "h-figure",
      [{ attr: "src", ph: "insImage", className: "hd-attr-src" }],
      node,
      view,
      getPos,
    );
    // src is not an attr of the figure node itself: route edits to the image child
    const srcInput = this.controls.get("src") as HTMLInputElement;
    this.syncSrcInput();
    const setSrc = (src: string) => {
      const pos = getPos();
      if (pos === undefined || !this.node.firstChild) return;
      const img = this.node.firstChild;
      if (img.attrs.src === src) return;
      view.dispatch(
        view.state.tr.setNodeMarkup(pos + 1, null, { ...img.attrs, src }),
      );
    };
    srcInput.addEventListener("change", () => setSrc(srcInput.value));

    // Picking a local file embeds it as a data: URL so the page stays a single
    // self-contained file; the src input remains for external URLs.
    const pick = async () => {
      const file = await pickImageFile();
      if (!file) return;
      const url = await imageToDataUrl(file).catch(() => null);
      if (url) setSrc(url);
    };
    this.pickBtn = document.createElement("button");
    this.pickBtn.type = "button";
    this.pickBtn.className = "hd-attr hd-pick-img";
    this.pickBtn.textContent = tr("hdocPickImage");
    this.pickBtn.addEventListener("click", () => void pick());
    this.dom.querySelector(".hd-fields")?.prepend(this.pickBtn);
    // The empty placeholder is itself a click target for picking.
    this.dom.addEventListener("click", (e) => {
      const el = e.target as HTMLElement;
      if (el.tagName === "IMG" && el.classList.contains("hd-img-empty"))
        void pick();
    });

    this.buildChrome(view, getPos);
    this.syncFigure();
    this.dom.addEventListener("mouseenter", () => this.placeChrome());
  }

  protected override setAttr(
    view: EditorView,
    getPos: GetPos,
    attr: string,
    value: string,
  ) {
    if (attr === "src") return; // handled by the dedicated listener above
    super.setAttr(view, getPos, attr, value);
  }

  private img(): HTMLImageElement | null {
    return this.dom.querySelector(".hd-body img");
  }

  /** Alignment toolbar + resize handles. Both live outside contentDOM, are
   * hidden until hover (CSS) and positioned against the rendered image. */
  private buildChrome(view: EditorView, getPos: GetPos) {
    this.tools = document.createElement("div");
    this.tools.className = "hd-fig-ui hd-fig-tools";
    this.tools.contentEditable = "false";
    const btn = (
      key: string,
      labelKey: string,
      content: string,
      apply: () => void,
    ) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "hd-fig-tool";
      b.title = tr(labelKey);
      if (content.startsWith("<svg")) b.innerHTML = content;
      else b.textContent = content;
      // preventDefault so the editor selection/focus stays put
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.addEventListener("click", apply);
      this.toolBtns.set(key, b);
      this.tools.appendChild(b);
    };
    btn("left", "hdocAlignLeft", ALIGN_ICONS.left, () =>
      this.setAttr(view, getPos, "align", "left"),
    );
    btn("center", "hdocAlignCenter", ALIGN_ICONS.center, () =>
      this.setAttr(view, getPos, "align", ""),
    );
    btn("right", "hdocAlignRight", ALIGN_ICONS.right, () =>
      this.setAttr(view, getPos, "align", "right"),
    );
    btn("reset", "hdocOriginalSize", "1:1", () =>
      this.setAttr(view, getPos, "width", ""),
    );
    this.dom.appendChild(this.tools);

    for (const side of [-1, 1] as const) {
      const h = document.createElement("div");
      h.className = "hd-fig-ui hd-fig-handle";
      h.contentEditable = "false";
      h.addEventListener("mousedown", (e) =>
        this.startResize(view, getPos, side, e),
      );
      this.handles.push(h);
      this.dom.appendChild(h);
    }
  }

  /** Drag a side handle → width as % of the text column, one transaction on
   * release (a single undo step). The CSS variable previews live. */
  private startResize(
    view: EditorView,
    getPos: GetPos,
    side: -1 | 1,
    e: MouseEvent,
  ) {
    const img = this.img();
    const colW = this.dom.clientWidth;
    if (!img || colW <= 0) return;
    e.preventDefault();
    const startX = e.clientX;
    const startW = img.getBoundingClientRect().width;
    // A centered image grows on both sides, so pointer travel counts double.
    const factor = String(this.node.attrs.align ?? "") === "" ? 2 : 1;
    let pct = 0;
    this.dom.classList.add("hd-resizing");
    const move = (ev: MouseEvent) => {
      const w = startW + (ev.clientX - startX) * side * factor;
      pct = Math.round(Math.max(FIG_MIN_PCT, Math.min(100, (w / colW) * 100)));
      this.dom.style.setProperty("--hd-fig-w", `${pct}%`);
      this.placeChrome();
    };
    const up = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      this.dom.classList.remove("hd-resizing");
      this.syncFigure(); // back to the attr-driven width…
      if (pct) this.setAttr(view, getPos, "width", String(pct)); // …then commit
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
  }

  /** Pin the handles to the image's edges and the toolbar to its top-right
   * (the image moves with alignment, so static CSS can't place them). */
  private placeChrome() {
    const img = this.img();
    if (!img) return;
    const fr = this.dom.getBoundingClientRect();
    const ir = img.getBoundingClientRect();
    this.handles.forEach((h, i) => {
      h.style.left = `${(i === 0 ? ir.left : ir.right) - fr.left}px`;
      h.style.top = `${ir.top - fr.top}px`;
      h.style.height = `${ir.height}px`;
    });
    this.tools.style.left = `${ir.right - fr.left - 6}px`;
    this.tools.style.top = `${ir.top - fr.top + 6}px`;
  }

  /** Mirror the image's src into the text input. An embedded data: URL
   * (thousands of chars) is not meaningfully editable, so the field hides
   * entirely; replace the image by deleting the figure. External URLs stay
   * visible and editable. */
  private syncSrcInput() {
    const srcInput = this.controls.get("src") as HTMLInputElement;
    if (document.activeElement === srcInput) return;
    const raw = String(this.node.firstChild?.attrs.src ?? "");
    const embedded = raw.startsWith("data:");
    srcInput.style.display = embedded ? "none" : "";
    const shown = embedded ? "" : raw;
    if (srcInput.value !== shown) srcInput.value = shown;
  }

  /** Attr-driven visual state: the width CSS variable, which chrome is
   * available (picker only while empty, resize/align only with an image),
   * and the active alignment button. */
  private syncFigure() {
    const has = !!String(this.node.firstChild?.attrs.src ?? "");
    this.pickBtn.style.display = has ? "none" : "";
    this.dom.classList.toggle("hd-fig-has-img", has);
    const w = parseInt(String(this.node.attrs.width ?? ""), 10);
    if (w >= FIG_MIN_PCT && w <= 100)
      this.dom.style.setProperty("--hd-fig-w", `${w}%`);
    else this.dom.style.removeProperty("--hd-fig-w");
    const align = String(this.node.attrs.align ?? "");
    for (const key of ["left", "center", "right"]) {
      this.toolBtns
        .get(key)
        ?.classList.toggle("active", align === (key === "center" ? "" : key));
    }
    const reset = this.toolBtns.get("reset");
    if (reset) reset.style.display = this.node.attrs.width ? "" : "none";
  }

  override stopEvent(e: Event): boolean {
    // instanceof Element, not HTMLElement: clicks land on the toolbar's SVG icons
    return (
      (e.target instanceof Element &&
        e.target.closest(".hd-fig-ui") !== null) ||
      super.stopEvent(e)
    );
  }

  override update(node: PMNode): boolean {
    if (!super.update(node)) return false;
    this.syncSrcInput();
    this.syncFigure();
    this.placeChrome();
    return true;
  }
}

const isRelativeUrl = (u: string) =>
  !!u &&
  !u.startsWith("/") &&
  !u.startsWith("#") &&
  !u.includes("://") &&
  !u.startsWith("data:");

/** Inline image: displayed through the protocol for relative paths. */
class ImageView implements NodeView {
  dom: HTMLImageElement;
  private node: PMNode;
  private readonly assetId: string;

  constructor(node: PMNode, assetId: string) {
    this.node = node;
    this.assetId = assetId;
    this.dom = document.createElement("img");
    this.render();
  }

  private render() {
    const src = String(this.node.attrs.src ?? "");
    const resolved = isRelativeUrl(src) ? relAssetUrl(this.assetId, src) : src;
    // Compare via getAttribute — the .src getter absolutizes relative URLs.
    if (this.dom.getAttribute("src") !== resolved) this.dom.src = resolved;
    this.dom.alt = String(this.node.attrs.alt ?? "");
    this.dom.classList.toggle("hd-img-empty", src === "");
  }

  update(node: PMNode): boolean {
    if (node.type !== this.node.type) return false;
    this.node = node;
    // Re-render: picking/pasting into an existing figure swaps the src.
    this.render();
    return true;
  }
}

/** h-toc placeholder: the real list is generated at render time. */
class TocView implements NodeView {
  dom: HTMLElement;
  constructor() {
    this.dom = document.createElement("h-toc");
    this.dom.setAttribute("data-label", tr("insToc"));
    this.dom.className = "hd-toc-editor";
  }
}

export function hdocNodeViews(assetId: string) {
  return {
    callout: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-callout",
        [
          {
            attr: "kind",
            ph: "insCallout",
            options: ["note", "tip", "warn", "danger"],
          },
          {
            attr: "title",
            ph: "hdocTitlePlaceholder",
            className: "hd-attr-title",
          },
        ],
        node,
        view,
        getPos,
      ),
    card: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-card",
        [
          {
            attr: "title",
            ph: "hdocTitlePlaceholder",
            className: "hd-attr-title",
          },
        ],
        node,
        view,
        getPos,
      ),
    quote: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-quote",
        [{ attr: "cite", ph: "insQuote", className: "hd-attr-cite" }],
        node,
        view,
        getPos,
      ),
    details: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-details",
        [{ attr: "summary", ph: "insDetails", className: "hd-attr-title" }],
        node,
        view,
        getPos,
      ),
    step: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-step",
        [
          {
            attr: "title",
            ph: "hdocTitlePlaceholder",
            className: "hd-attr-title",
          },
        ],
        node,
        view,
        getPos,
      ),
    stat: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new ComponentView(
        "h-stat",
        [
          { attr: "value", ph: "insStats", className: "hd-attr-value" },
          {
            attr: "label",
            ph: "hdocTitlePlaceholder",
            className: "hd-attr-label",
          },
        ],
        node,
        view,
        getPos,
        { leaf: true },
      ),
    figure: (node: PMNode, view: EditorView, getPos: GetPos) =>
      new FigureView(node, view, getPos),
    image: (node: PMNode) => new ImageView(node, assetId),
    toc: () => new TocView(),
  };
}
