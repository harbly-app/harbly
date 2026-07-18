/**
 * The format's load-bearing property: parse ∘ serialize is lossless and
 * serialize ∘ parse is idempotent. If these break, opening + saving a page
 * could silently corrupt user files.
 */
import { describe, expect, it } from "vitest";
import { parseHdoc } from "./parse";
import { serializeHdoc } from "./serialize";

const FULL = `<h-doc v="1" theme="sepia">
  <h1>发布方案 <em>v2</em></h1>
  <p>正文包含 <strong>加粗</strong>、<em>斜体</em>、<code>code</code>、<s>删除</s> 与 <a href="https://example.com?a=1&amp;b=2">链接</a>。</p>
  <h-toc></h-toc>
  <h-callout kind="tip" title="一句话结论 &quot;引用&quot;">
    <p>按席位收费，<strong>次年 <em>续费</em></strong> 打折。</p>
  </h-callout>
  <h-columns>
    <h-column>
      <h-card title="方案 A">
        <p>低价切入</p>
      </h-card>
    </h-column>
    <h-column>
      <h-card title="方案 B">
        <p>高配定价</p>
        <ul>
          <li>项目一</li>
          <li>嵌套
            <ul>
              <li>子项</li>
            </ul>
          </li>
        </ul>
      </h-card>
    </h-column>
  </h-columns>
  <h-steps>
    <h-step title="准备">
      <p>写文档</p>
    </h-step>
    <h-step title="发布">
      <p>上线并<br>观察</p>
    </h-step>
  </h-steps>
  <h-stats>
    <h-stat value="98%" label="满意度"></h-stat>
    <h-stat value="3.2s" label="加载时间"></h-stat>
  </h-stats>
  <h-quote cite="某用户">
    <p>非常好用。</p>
  </h-quote>
  <h-details summary="更多细节" open>
    <p>展开后的内容。</p>
  </h-details>
  <h-figure width="60" align="left"><img src="images/arch.png" alt="arch"></h-figure>
  <blockquote>
    <p>原生引用块。</p>
  </blockquote>
  <pre><code>const x = 1;
if (x &lt; 2) { console.log("&lt;ok&gt;"); }</code></pre>
  <table>
    <tr>
      <th>列一</th>
      <th>列二</th>
    </tr>
    <tr>
      <td>甲</td>
      <td><strong>乙</strong></td>
    </tr>
  </table>
  <hr>
  <ol>
    <li>第一</li>
    <li>第二</li>
  </ol>
  <p></p>
</h-doc>
`;

/** The skeleton the Rust side writes for a brand-new page (keep in sync with
 * harbly-core's HDOC_NEW_TEMPLATE). */
const SKELETON = `<h-doc v="1" theme="paper">\n  <h1></h1>\n  <p></p>\n</h-doc>\n`;

describe("hdoc round-trip", () => {
  it("round-trips a full-vocabulary document losslessly", () => {
    const p1 = parseHdoc(FULL);
    expect(p1.ok).toBe(true);
    if (!p1.ok) return;
    const s1 = serializeHdoc(p1.doc);
    const p2 = parseHdoc(s1);
    expect(p2.ok).toBe(true);
    if (!p2.ok) return;
    // Documents are structurally identical across the round-trip…
    expect(p2.doc.eq(p1.doc)).toBe(true);
    // …and re-serialization is byte-stable (no drift on repeated saves).
    expect(serializeHdoc(p2.doc)).toBe(s1);
  });

  it("preserves text content, attributes and marks", () => {
    const p = parseHdoc(FULL);
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    const s = serializeHdoc(p.doc);
    expect(s).toContain('theme="sepia"');
    expect(s).toContain('kind="tip"');
    expect(s).toContain("一句话结论 &quot;引用&quot;");
    expect(s).toContain('<h-stat value="98%" label="满意度"></h-stat>');
    expect(s).toContain('href="https://example.com?a=1&amp;b=2"');
    expect(s).toContain("<strong>次年 <em>续费</em></strong>");
    expect(s).toContain('<h-details summary="更多细节" open>');
    expect(s).toContain('<img src="images/arch.png" alt="arch">');
    expect(s).toContain('if (x &lt; 2) { console.log("&lt;ok&gt;"); }');
    expect(s).toContain("上线并<br>观察");
  });

  it("parses the new-file skeleton and keeps it stable", () => {
    const p = parseHdoc(SKELETON);
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    expect(serializeHdoc(p.doc)).toBe(SKELETON);
  });

  it("rejects unknown elements instead of dropping them", () => {
    expect(
      parseHdoc(`<h-doc v="1"><h-chart data="x"></h-chart></h-doc>`),
    ).toMatchObject({
      ok: false,
      reason: "unsupported",
    });
    expect(
      parseHdoc(`<h-doc v="1"><div><p>wrapped</p></div></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
  });

  it("rejects script/style content as unsupported (defense in depth)", () => {
    expect(
      parseHdoc(`<h-doc v="1"><p>x</p><script>alert(1)</script></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(`<h-doc v="1"><style>p{display:none}</style><p>x</p></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
  });

  it("requires an h-doc root", () => {
    expect(parseHdoc(`<p>plain html</p>`)).toMatchObject({
      ok: false,
      reason: "no-root",
    });
  });

  it("survives an empty document", () => {
    const p = parseHdoc(`<h-doc v="1" theme="night"></h-doc>`);
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    expect(p.doc.childCount).toBeGreaterThan(0); // filled with an empty paragraph
    expect(p.doc.attrs.theme).toBe("night");
  });

  it("round-trips the docs layout attribute; default stays implicit", () => {
    const p = parseHdoc(
      `<h-doc v="1" theme="paper" layout="docs"><h1>t</h1></h-doc>`,
    );
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    expect(p.doc.attrs.layout).toBe("docs");
    const s = serializeHdoc(p.doc);
    expect(s).toContain('layout="docs"');
    // default layout carries no attribute noise
    const d = parseHdoc(`<h-doc v="1"><p>x</p></h-doc>`);
    expect(d.ok).toBe(true);
    if (!d.ok) return;
    expect(serializeHdoc(d.doc)).not.toContain("layout=");
  });

  it("preserves unknown theme values (forward compatibility)", () => {
    const p = parseHdoc(`<h-doc v="2" theme="future"><p>x</p></h-doc>`);
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    const s = serializeHdoc(p.doc);
    expect(s).toContain('theme="future"');
    expect(s).toContain('v="2"');
  });

  it("round-trips a figure with an embedded data: URL image", () => {
    // Local images are embedded as data: URLs; the base64 payload must survive
    // parse → serialize verbatim so the page stays a self-contained file.
    const data =
      "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M8AAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    const p = parseHdoc(
      `<h-doc v="1"><h-figure><img src="${data}" alt="a"></h-figure></h-doc>`,
    );
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    const s = serializeHdoc(p.doc);
    expect(s).toContain(`<img src="${data}" alt="a">`);
    const p2 = parseHdoc(s);
    expect(p2.ok).toBe(true);
    if (!p2.ok) return;
    expect(p2.doc.eq(p.doc)).toBe(true);
  });

  it("dissolves thead/tbody wrappers without losing rows", () => {
    const p = parseHdoc(
      `<h-doc v="1"><table><thead><tr><th>a</th></tr></thead><tbody><tr><td>b</td></tr></tbody></table></h-doc>`,
    );
    expect(p.ok).toBe(true);
    if (!p.ok) return;
    const s = serializeHdoc(p.doc);
    expect(s).toContain("<th>a</th>");
    expect(s).toContain("<td>b</td>");
  });

  it("round-trips merged table cells (colspan/rowspan)", () => {
    const SPAN = `<h-doc v="1">
  <table>
    <tr>
      <th colspan="2">合并表头</th>
    </tr>
    <tr>
      <td rowspan="2">跨两行</td>
      <td>右上</td>
    </tr>
    <tr>
      <td>右下</td>
    </tr>
  </table>
</h-doc>
`;
    const p1 = parseHdoc(SPAN);
    expect(p1.ok).toBe(true);
    if (!p1.ok) return;
    const s1 = serializeHdoc(p1.doc);
    // Spans survive the save instead of collapsing to plain cells…
    expect(s1).toContain('<th colspan="2">合并表头</th>');
    expect(s1).toContain('<td rowspan="2">跨两行</td>');
    // …and the document is stable across a full round-trip.
    const p2 = parseHdoc(s1);
    expect(p2.ok).toBe(true);
    if (!p2.ok) return;
    expect(p2.doc.eq(p1.doc)).toBe(true);
    expect(serializeHdoc(p2.doc)).toBe(s1);
  });

  it("rejects unknown attributes on known tags", () => {
    // A stray attribute on an allowed element must flip to read-only, not be
    // silently dropped on the next save.
    expect(
      parseHdoc(`<h-doc v="1"><p style="color:red">x</p></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><p><a href="x" onclick="alert(1)">l</a></p></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
  });

  it("rejects executable URL values (lockstep with the Rust validator)", () => {
    expect(
      parseHdoc(
        `<h-doc v="1"><p><a href="javascript:alert(1)">x</a></p></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(`<h-doc v="1"><img src="javascript:1" alt=""></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    // Benign URLs keep parsing
    expect(
      parseHdoc(
        `<h-doc v="1"><p><a href="https://a/b#c">x</a> <a href="#t">y</a> <a href="mailto:a@b">z</a></p><h-figure><img src="data:image/png;base64,AA" alt=""></h-figure></h-doc>`,
      ),
    ).toMatchObject({ ok: true });
  });

  it("rejects structurally misplaced vocabulary instead of restructuring it", () => {
    // PM's DOMParser silently lifts/moves nodes on a content-model mismatch;
    // an edit + autosave would then write the restructured shape back. Every
    // one of these must open read-only instead.
    expect(
      parseHdoc(
        `<h-doc v="1"><blockquote><p>a</p><ul><li>b</li></ul></blockquote></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-callout kind="note"><h-columns><h-column><p>x</p></h-column><h-column><p>y</p></h-column></h-columns></h-callout></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(`<h-doc v="1"><h-stats><p>x</p></h-stats></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-stat value="1" label="l"><p>x</p></h-stat></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    // A component inside a paragraph (HTML parsing does not auto-close <p>
    // before custom elements) would be split out — refuse it too.
    expect(
      parseHdoc(
        `<h-doc v="1"><p>a<h-callout kind="note"><p>x</p></h-callout></p></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    // …including when the component hides inside a MARK tag (PM would empty
    // the callout and lift its content into sibling paragraphs).
    expect(
      parseHdoc(
        `<h-doc v="1"><p>a<strong><h-callout kind="note"><p>x</p></h-callout></strong>b</p></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-card title="t"><strong><h-stat value="1" label="x"></h-stat></strong></h-card></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    // Restricted textblocks: pre is text-only (a mark would be silently
    // unwrapped, an img lifted out); a figure is exactly one image (captions
    // and extra images get moved out; a br gets dropped).
    expect(
      parseHdoc(
        `<h-doc v="1"><pre><code>x<strong>bold</strong>y</code></pre></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><pre><code>x</code><img src="a.png" alt=""></pre></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-figure><img src="a.png" alt="">caption</h-figure></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-figure><img src="a.png" alt=""><img src="b.png" alt=""></h-figure></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(`<h-doc v="1"><h-figure><br></h-figure></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
  });

  it("rejects content PM would drop silently (href-less <a>, unicode-space in strict containers)", () => {
    // The schema's only link rule is a[href] — a bare <a> would vanish on save
    expect(
      parseHdoc(`<h-doc v="1"><p>see <a>the appendix</a></p></h-doc>`),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    // U+00A0 is real content to PM but un-placeable in h-stats ("stat+") —
    // it would be dropped on save. ASCII whitespace stays ignorable.
    expect(
      parseHdoc(
        `<h-doc v="1"><h-stats>\u00a0<h-stat value="1" label="l"></h-stat></h-stats></h-doc>`,
      ),
    ).toMatchObject({ ok: false, reason: "unsupported" });
    expect(
      parseHdoc(
        `<h-doc v="1"><h-stats>\n  <h-stat value="1" label="l"></h-stat>\n</h-stats></h-doc>`,
      ),
    ).toMatchObject({ ok: true });
  });

  it("still accepts benign non-canonical shapes (fill, not restructure)", () => {
    // Loose text in a container is wrapped in place, an empty card is filled
    // with an empty paragraph — nothing moves, so these stay editable.
    expect(
      parseHdoc(`<h-doc v="1"><h-card title="t">loose text</h-card></h-doc>`),
    ).toMatchObject({ ok: true });
    expect(
      parseHdoc(`<h-doc v="1"><h-card title="t"></h-card></h-doc>`),
    ).toMatchObject({ ok: true });
  });
});
