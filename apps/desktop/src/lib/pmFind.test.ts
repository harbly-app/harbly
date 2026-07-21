import { describe, expect, it } from "vitest";
import { parseHdoc } from "../hdoc/parse";
import { collectMatches } from "./pmFind";

const doc = (body: string) => {
  const p = parseHdoc(`<h-doc v="1">${body}</h-doc>`);
  if (!p.ok) throw new Error("parse failed");
  return p.doc;
};

describe("collectMatches", () => {
  it("finds case-insensitive matches across mark boundaries", () => {
    const d = doc("<p>Hello <strong>World</strong> hello</p>");
    expect(collectMatches(d, "hello world")).toHaveLength(1);
    expect(collectMatches(d, "HELLO")).toHaveLength(2);
  });

  it("returns positions aligned with the document", () => {
    const d = doc("<p>abcabc</p>");
    expect(collectMatches(d, "abc")).toEqual([
      { from: 1, to: 4 },
      { from: 4, to: 7 },
    ]);
  });

  it("does not match across an inline leaf node", () => {
    const d = doc("<p>ab<br>cd</p>");
    expect(collectMatches(d, "abcd")).toHaveLength(0);
    expect(collectMatches(d, "cd")).toHaveLength(1);
  });

  it("searches nested blocks (callout, table cells)", () => {
    const d = doc(
      '<h-callout title="Note"><p>needle in callout</p></h-callout>',
    );
    expect(collectMatches(d, "needle")).toHaveLength(1);
  });

  it("empty query yields nothing", () => {
    expect(collectMatches(doc("<p>x</p>"), "")).toHaveLength(0);
  });
});
