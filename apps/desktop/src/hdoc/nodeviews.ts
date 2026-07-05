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

/** h-figure: image preview + src/caption inputs. The src input edits the child
 * image node (position = own pos + 1); relative paths display through the
 * sibling-file protocol route without changing what is stored. */
class FigureView extends ComponentView {
  constructor(node: PMNode, view: EditorView, getPos: GetPos) {
    super(
      "h-figure",
      [
        { attr: "src", ph: "insImage", className: "hd-attr-src" },
        {
          attr: "caption",
          ph: "hdocTitlePlaceholder",
          className: "hd-attr-caption",
        },
      ],
      node,
      view,
      getPos,
    );
    // src is not an attr of the figure node itself: route edits to the image child
    const srcInput = this.controls.get("src") as HTMLInputElement;
    srcInput.value = String(node.firstChild?.attrs.src ?? "");
    srcInput.addEventListener("change", () => {
      const pos = getPos();
      if (pos === undefined || !this.node.firstChild) return;
      const img = this.node.firstChild;
      if (img.attrs.src === srcInput.value) return;
      view.dispatch(
        view.state.tr.setNodeMarkup(pos + 1, null, {
          ...img.attrs,
          src: srcInput.value,
        }),
      );
    });
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

  override update(node: PMNode): boolean {
    if (!super.update(node)) return false;
    const srcInput = this.controls.get("src") as HTMLInputElement;
    const v = String(node.firstChild?.attrs.src ?? "");
    if (document.activeElement !== srcInput && srcInput.value !== v)
      srcInput.value = v;
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

  constructor(node: PMNode, assetId: string) {
    this.node = node;
    this.dom = document.createElement("img");
    this.render(assetId);
  }

  private render(assetId: string) {
    const src = String(this.node.attrs.src ?? "");
    this.dom.src = isRelativeUrl(src) ? relAssetUrl(assetId, src) : src;
    this.dom.alt = String(this.node.attrs.alt ?? "");
    this.dom.classList.toggle("hd-img-empty", src === "");
  }

  update(node: PMNode, _decos: unknown, view?: unknown): boolean {
    void view;
    if (node.type !== this.node.type) return false;
    this.node = node;
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
