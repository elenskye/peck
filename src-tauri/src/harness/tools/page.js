// screenshot_page（page.rs）：osascript -l JavaScript -e <本脚本> <url> <out.png> <宽> <高> <外观 light/dark/空> <停动画 1/0> <JS 或空> <等多少毫秒>
// 后台开一个放在屏幕外的无边框窗口，里面一个 WKWebView：不抢焦点、不要屏幕录制权限、不碰钥匙串。打印一行 JSON。
// JXA 收不到 callAsyncJavaScript 的回调，脚本包成 async 函数、结果放在 window.__peckShot 上轮询。
ObjC.import('AppKit');
ObjC.import('WebKit');
function run(argv) {
  const [url, out, w, h, look, still, js, waitMs] = argv;
  const W = Number(w), H = Number(h);
  const app = $.NSApplication.sharedApplication;
  app.setActivationPolicy($.NSApplicationActivationPolicyProhibited);
  const web = $.WKWebView.alloc.initWithFrameConfiguration($.NSMakeRect(0, 0, W, H), $.WKWebViewConfiguration.alloc.init);
  const win = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer($.NSMakeRect(-30000, -30000, W, H), $.NSWindowStyleMaskBorderless, $.NSBackingStoreBuffered, false);
  win.contentView = web;
  win.orderBack(null);
  if (look === 'dark') web.appearance = $.NSAppearance.appearanceNamed($.NSAppearanceNameDarkAqua);
  if (look === 'light') web.appearance = $.NSAppearance.appearanceNamed($.NSAppearanceNameAqua);
  const spin = (sec) => $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(sec));
  const evaluate = (code, limit) => {
    let done = false, value = null, error = '';
    web.evaluateJavaScriptCompletionHandler(code, (r, e) => {
      if (e && !e.isNil()) error = String(ObjC.unwrap(e.localizedDescription)) + ' ' + String((ObjC.deepUnwrap(e.userInfo) || {}).WKJavaScriptExceptionMessage || '');
      else value = r && !r.isNil() ? ObjC.unwrap(r) : null;
      done = true;
    });
    const t0 = Date.now();
    while (!done && Date.now() - t0 < limit) spin(0.05);
    return { done, value, error };
  };
  web.loadRequest($.NSURLRequest.requestWithURL($.NSURL.URLWithString(url)));
  const t0 = Date.now();
  spin(0.2);
  while (web.isLoading && Date.now() - t0 < 20000) spin(0.1);
  if (web.isLoading) return JSON.stringify({ error: '20 秒还没加载完。' });
  const ready = evaluate('location.href', 3000);
  if (!ready.value || String(ready.value).indexOf('about:blank') === 0) return JSON.stringify({ error: '打不开这个地址（dev server 起了吗？）' });
  spin(0.5);
  // 离屏的窗口里 CSS 动画不走帧、停在第一帧：默认关掉动画和过渡，截到的是最后的样子。
  if (still === '1') evaluate("(() => { const s = document.createElement('style'); s.textContent = '*,*::before,*::after{animation:none!important;transition:none!important}'; document.head.appendChild(s); })()", 3000);
  let result = '';
  if (js) {
    const start = evaluate("window.__peckShot = {done: false}; (async () => {\n" + js + "\n})().then(v => { window.__peckShot = {done: true, value: v === undefined || v === null ? '' : (typeof v === 'string' ? v : JSON.stringify(v))}; }, e => { window.__peckShot = {done: true, error: String((e && e.message) || e)}; }); 'started'", 5000);
    if (start.error) result = 'JS 出错：' + start.error;
    else {
      const t1 = Date.now();
      let got = null;
      while (Date.now() - t1 < 30000) {
        const r = evaluate('JSON.stringify(window.__peckShot)', 3000);
        got = r.value ? JSON.parse(r.value) : null;
        if (got && got.done) break;
        spin(0.1);
      }
      if (!got || !got.done) result = 'JS 30 秒没跑完，按当时的样子截了。';
      else result = got.error ? 'JS 出错：' + got.error : got.value;
    }
  }
  spin(Math.max(0, Number(waitMs || 0)) / 1000);
  let shot = null, err = '';
  web.takeSnapshotWithConfigurationCompletionHandler($.WKSnapshotConfiguration.alloc.init, (img, e) => {
    if (img && !img.isNil()) shot = img;
    else err = e && !e.isNil() ? String(ObjC.unwrap(e.localizedDescription)) : '没截到';
  });
  const t2 = Date.now();
  while (!shot && !err && Date.now() - t2 < 10000) spin(0.05);
  if (!shot) return JSON.stringify({ error: '截不了：' + (err || '超时') });
  const rep = $.NSBitmapImageRep.imageRepWithData(shot.TIFFRepresentation);
  rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $()).writeToFileAtomically(out, true);
  return JSON.stringify({ ok: true, url: String(ready.value), result: String(result) });
}
