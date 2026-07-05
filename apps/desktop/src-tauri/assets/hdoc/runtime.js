/* Harbly hdoc runtime behavior, v1. Deliberately tiny and ES5-compatible
   (macOS 10.15 WKWebView floor). The document itself never executes scripts
   (CSP allows only this nonce'd runtime), so everything interactive in the
   vocabulary is implemented here: relative media resolution, heading anchors,
   table of contents, collapsible sections. */
(function () {
  var doc = document.querySelector("h-doc");
  if (!doc) return;

  // Served through the asset protocol the document URL is /current/<id>, so
  // relative image paths would miss; the shell passes the sibling-file route
  // base and we rewrite them onto it. Baked exports set no base: relative
  // paths resolve next to the exported file, as in any plain HTML.
  var base = window.__HDOC_REL_BASE;
  if (base) {
    var isRel = function (u) {
      return (
        !!u &&
        !/^[a-z][a-z0-9+.-]*:/i.test(u) &&
        u.charAt(0) !== "/" &&
        u.charAt(0) !== "#"
      );
    };
    Array.prototype.forEach.call(
      doc.querySelectorAll("img[src]"),
      function (img) {
        var u = img.getAttribute("src");
        if (isRel(u)) {
          img.setAttribute(
            "src",
            base + u.split("/").map(encodeURIComponent).join("/"),
          );
        }
      },
    );
  }

  // Heading anchors + table(s) of contents.
  var heads = Array.prototype.filter.call(
    doc.querySelectorAll("h1,h2,h3"),
    function (h) {
      return !h.closest("h-toc");
    },
  );
  heads.forEach(function (h, i) {
    if (!h.id) h.id = "hd-" + i;
  });
  Array.prototype.forEach.call(doc.querySelectorAll("h-toc"), function (toc) {
    toc.setAttribute("data-label", window.__HDOC_TOC_LABEL || "Contents");
    var ol = document.createElement("ol");
    ol.className = "hd-toc-list";
    heads.forEach(function (h) {
      var li = document.createElement("li");
      li.className = "hd-toc-" + h.tagName.toLowerCase();
      var a = document.createElement("a");
      a.href = "#" + h.id;
      a.textContent = h.textContent;
      li.appendChild(a);
      ol.appendChild(li);
    });
    toc.appendChild(ol);
  });

  // Collapsible sections: materialize the summary row from the attribute.
  Array.prototype.forEach.call(doc.querySelectorAll("h-details"), function (d) {
    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "hd-summary";
    btn.textContent = d.getAttribute("summary") || "";
    btn.addEventListener("click", function () {
      if (d.hasAttribute("open")) d.removeAttribute("open");
      else d.setAttribute("open", "");
    });
    d.insertBefore(btn, d.firstChild);
  });
})();
