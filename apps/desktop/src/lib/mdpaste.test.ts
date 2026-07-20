import { describe, expect, it } from "vitest";
import { stripDoomedImageRefs } from "./mdpaste";

describe("stripDoomedImageRefs", () => {
  it("strips webkit-fake-url image references", () => {
    const md = "before\n\n![](webkit-fake-url://ABC-123/image.png)\n\nafter";
    expect(stripDoomedImageRefs(md)).toBe("before\n\n\n\nafter");
  });

  it("strips blob image references and keeps alt text out", () => {
    const md = "a ![screenshot](blob:http://localhost/uuid-1) b";
    expect(stripDoomedImageRefs(md)).toBe("a  b");
  });

  it("keeps data:, remote and relative images untouched", () => {
    const md = [
      "![x](data:image/png;base64,AAAA)",
      "![y](https://example.com/pic.png)",
      "![z](./pics/local.png)",
    ].join("\n");
    expect(stripDoomedImageRefs(md)).toBe(md);
  });

  it("handles several doomed refs in one document", () => {
    const md =
      "![a](blob:1) mid ![b](webkit-fake-url://2) end ![ok](https://e.com/i.png)";
    expect(stripDoomedImageRefs(md)).toBe(
      " mid  end ![ok](https://e.com/i.png)",
    );
  });

  it("is case-insensitive and tolerates angle-bracket destinations", () => {
    const md = "![](WEBKIT-FAKE-URL://X) ![](<blob:abc>)";
    // The angle form keeps a trailing ">" out of the match target; both
    // references must still be recognized as doomed and removed.
    expect(stripDoomedImageRefs(md)).not.toContain("webkit");
    expect(stripDoomedImageRefs(md).toLowerCase()).not.toContain("blob:");
  });

  it("returns clean markdown unchanged", () => {
    const md = "# Title\n\nplain **text**, no images.";
    expect(stripDoomedImageRefs(md)).toBe(md);
  });
});
