/**
 * Paste ownership invariants: an image paste is detected even when the
 * clipboard exposes nothing to JS (WKWebView raw-image paste), and
 * session-local image srcs (webkit-fake-url:/blob:) can never enter a
 * document through transformPasted.
 */
import { describe, expect, it } from "vitest";
import { Fragment, Slice } from "prosemirror-model";
import { isImageOnlyPaste, stripOpaqueImages } from "./image";
import { hdocSchema } from "./schema";

const cd = (data: Record<string, string>): DataTransfer =>
  ({
    getData: (type: string) => data[type] ?? "",
  }) as unknown as DataTransfer;

describe("isImageOnlyPaste", () => {
  it("plain text paste is not an image paste", () => {
    expect(isImageOnlyPaste(cd({ "text/plain": "hello" }))).toBe(false);
  });

  it("rich paste with visible text keeps the text path", () => {
    expect(
      isImageOnlyPaste(
        cd({ "text/html": '<p>hi</p><img src="webkit-fake-url://x/y.png">' }),
      ),
    ).toBe(false);
  });

  it("nothing JS-readable = a native raw-image (or empty) paste", () => {
    expect(isImageOnlyPaste(cd({}))).toBe(true);
  });

  it("html that is only images counts as an image paste", () => {
    expect(
      isImageOnlyPaste(cd({ "text/html": '<img src="https://a/b.png">' })),
    ).toBe(true);
  });

  it("whitespace-only text does not defeat detection", () => {
    expect(
      isImageOnlyPaste(
        cd({
          "text/plain": "  \n",
          "text/html": '<img src="webkit-fake-url://x">',
        }),
      ),
    ).toBe(true);
  });
});

describe("stripOpaqueImages", () => {
  const { doc, paragraph, figure, image, text } = {
    doc: hdocSchema.nodes.doc,
    paragraph: hdocSchema.nodes.paragraph,
    figure: hdocSchema.nodes.figure,
    image: hdocSchema.nodes.image,
    text: (s: string) => hdocSchema.text(s),
  };

  it("drops webkit-fake-url figures, keeps text and data: figures", () => {
    const slice = new Slice(
      Fragment.from([
        figure.create(null, image.create({ src: "webkit-fake-url://a/1" })),
        paragraph.create(null, text("kept")),
        figure.create(null, image.create({ src: "data:image/png;base64,AA" })),
      ]),
      0,
      0,
    );
    const out = stripOpaqueImages(slice);
    const kinds: string[] = [];
    out.content.forEach((n) => kinds.push(n.type.name));
    expect(kinds).toEqual(["paragraph", "figure"]);
    expect(out.content.child(1).firstChild?.attrs.src).toMatch(/^data:/);
  });

  it("drops blob: inline images inside a paragraph, keeps the text", () => {
    const slice = new Slice(
      Fragment.from(
        paragraph.create(null, [
          text("before "),
          image.create({ src: "blob:null/123" }),
          text(" after"),
        ]),
      ),
      0,
      0,
    );
    const out = stripOpaqueImages(slice);
    expect(out.content.textBetween(0, out.content.size, " ")).toContain(
      "before",
    );
    let imgs = 0;
    out.content.descendants((n) => {
      if (n.type === image) imgs++;
      return true;
    });
    expect(imgs).toBe(0);
  });

  it("returns the slice unchanged when nothing is opaque", () => {
    const slice = new Slice(
      Fragment.from(
        doc.create(null, paragraph.create(null, text("x"))).content,
      ),
      0,
      0,
    );
    expect(stripOpaqueImages(slice)).toBe(slice);
  });
});
