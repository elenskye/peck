// Peck 预览面板注进页面的小脚本（v0.8 Phase 12）：Rust 的预览代理在 HTML 里插这一段，`?mock` 的演示页也用它。
// 父窗口发 {peck:"annotate", on} 开关「点元素」；开着时悬停描边、点一下把这个元素的样子发回去
// （{peck:"pick", selector, tag, text, url}），页面本身不响应这次点击。页面换了地址也告诉父窗口（{peck:"ready"}）。
(function () {
  if (window.__peckPreview) return;
  window.__peckPreview = true;
  var on = false;
  var box = null;
  function esc(s) { return String(s).replace(/([^\w-])/g, "\\$1"); }
  function selector(el) {
    var parts = [];
    for (var n = el, depth = 0; n && n.nodeType === 1 && depth < 5; n = n.parentElement, depth++) {
      if (n.id) { parts.unshift("#" + esc(n.id)); break; }
      var tag = n.tagName.toLowerCase();
      var cls = (n.getAttribute("class") || "").trim().split(/\s+/).filter(Boolean).slice(0, 2).map(function (c) { return "." + esc(c); }).join("");
      var same = n.parentElement ? Array.prototype.filter.call(n.parentElement.children, function (c) { return c.tagName === n.tagName; }) : [];
      parts.unshift(tag + cls + (same.length > 1 ? ":nth-of-type(" + (same.indexOf(n) + 1) + ")" : ""));
      if (tag === "body") break;
    }
    return parts.join(" > ");
  }
  function outline(el) {
    if (!box) {
      box = document.createElement("div");
      box.setAttribute("data-peck", "box");
      box.style.cssText = "position:fixed;z-index:2147483647;pointer-events:none;border:2px solid #00E5FF;background:rgba(0,229,255,0.08);transition:all 60ms";
      document.documentElement.appendChild(box);
    }
    var r = el.getBoundingClientRect();
    box.style.left = r.left + "px"; box.style.top = r.top + "px"; box.style.width = r.width + "px"; box.style.height = r.height + "px";
  }
  window.addEventListener("message", function (e) {
    var d = e.data;
    if (!d || d.peck !== "annotate") return;
    on = !!d.on;
    document.documentElement.style.cursor = on ? "crosshair" : "";
    if (!on && box) { box.remove(); box = null; }
  });
  document.addEventListener("mouseover", function (e) { if (on && e.target && e.target.nodeType === 1) outline(e.target); }, true);
  document.addEventListener("click", function (e) {
    if (!on) return;
    e.preventDefault();
    e.stopPropagation();
    var el = e.target;
    if (!el || el.nodeType !== 1) return;
    parent.postMessage({ peck: "pick", selector: selector(el), tag: el.tagName.toLowerCase(), text: (el.innerText || el.getAttribute("alt") || el.getAttribute("aria-label") || "").trim().replace(/\s+/g, " ").slice(0, 160), url: location.href }, "*");
  }, true);
  parent.postMessage({ peck: "ready", url: location.href, title: document.title }, "*");
})();
