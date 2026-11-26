// web_fetch（web.rs）：用 osascript 开离屏 WKWebView，输出渲染后的 DOM。
ObjC.import('AppKit');
ObjC.import('WebKit');
ObjC.import('Network');
// 系统的 BridgeSupport 里没有这个（macOS 14 起才有的接口），自己绑。
ObjC.bindFunction('nw_proxy_config_create_socksv5', ['id', ['id']]);

// 在页面脚本前登记 fetch/XHR 和 DOM 活动。资源计时也涵盖图片、脚本等加载。
function observeActivity() {
  let lastChange = performance.now();
  let pending = 0;
  const changed = () => { lastChange = performance.now(); };
  const completed = () => { pending--; changed(); };
  new MutationObserver(changed).observe(document, {
    childList: true, subtree: true, characterData: true, attributes: true,
  });
  new PerformanceObserver(changed).observe({ entryTypes: ['resource'] });
  const fetch = window.fetch;
  window.fetch = function () {
    pending++;
    return fetch.apply(this, arguments).finally(completed);
  };
  const send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function () {
    this.addEventListener('loadend', completed, { once: true });
    pending++;
    try {
      return send.apply(this, arguments);
    } catch (error) {
      this.removeEventListener('loadend', completed);
      completed();
      throw error;
    }
  };
  window.__peckWebQuiet = () => pending === 0 && performance.now() - lastChange >= 750;
}

function readDocument(limit) {
  const root = document.documentElement.cloneNode(true);
  root.querySelectorAll('script,style,noscript,template,iframe').forEach(el => el.remove());
  const bytes = new TextEncoder().encode(root.outerHTML);
  const partial = bytes.length > limit;
  const html = new TextDecoder().decode(bytes.subarray(0, limit), { stream: partial });
  return JSON.stringify({ url: location.href, html, partial });
}

function run(argv) {
  const [url, allowedHosts, rulesDir, localOrigin, limit, proxyPort] = argv;
  const deadline = Date.now() + 28000;
  const spin = () => $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.05));
  const fail = (error, url) => JSON.stringify({ error, url });
  const unwrap = v => v && !v.isNil() ? ObjC.unwrap(v) : null;
  $.NSApplication.sharedApplication.setActivationPolicy($.NSApplicationActivationPolicyProhibited);

  // 同站文档才让进；资源可来自 CDN。网址里写明的本机、内网地址先拦下（解析出来的由代理管）。
  const documentRule = (filter, type) => ({
    trigger: { 'url-filter': filter, 'resource-type': ['document'] }, action: { type },
  });
  const literal = value => value.replace(/[.*+?^$(){}|[\]\\]/g, '\\$&');
  const scheme = url.startsWith('https:') ? 'https' : 'https?';
  const rules = [documentRule('.*', 'block')];
  for (const host of allowedHosts.split(',')) {
    rules.push(documentRule('^' + scheme + '://' + literal(host) + '[:/]', 'ignore-previous-rules'));
  }
  const block = filter => ({ trigger: { 'url-filter': filter }, action: { type: 'block' } });
  rules.push(block('^file:'));
  const local = [
    '[^.:/]+\\.?[:/]', // 无点的主机（含 localhost）
    '[^/:]+\\.localhost\\.?[:/]', '[^/:]+\\.local\\.?[:/]', '[^/:]+\\.internal\\.?[:/]',
    '0\\.', '10\\.', '127\\.', '169\\.254\\.', '192\\.168\\.', '192\\.0\\.0\\.',
    '172\\.1[6-9]\\.', '172\\.2[0-9]\\.', '172\\.3[01]\\.',
    '100\\.[6-9][0-9]\\.', '100\\.1[01][0-9]\\.', '100\\.12[0-7]\\.',
    '22[4-9]\\.', '23[0-9]\\.', '255\\.255\\.255\\.255[:/]',
    '\\[::', '\\[f[cd]', '\\[fe[89ab]', '\\[ff',
  ];
  for (const protocol of ['https?', 'wss?']) {
    for (const prefix of local) rules.push(block('^' + protocol + '://' + prefix));
    rules.push(block('^' + protocol + '://[^/]*@'));
  }
  if (localOrigin) {
    // Rust 的 allow_local 仅在测试中给出一个夹具的 origin。
    rules.push({ trigger: { 'url-filter': '^' + literal(localOrigin) + '/' }, action: { type: 'ignore-previous-rules' } });
  }
  const store = $.WKContentRuleListStore.storeWithURL($.NSURL.fileURLWithPath(rulesDir));
  let compiled = false, list, ruleError;
  store.compileContentRuleListForIdentifierEncodedContentRuleListCompletionHandler('peck', JSON.stringify(rules), (result, error) => {
    list = result;
    ruleError = error;
    compiled = true;
  });
  const compileBy = Date.now() + 4000;
  while (!compiled && Date.now() < compileBy) spin();
  if (!compiled || !list || list.isNil()) {
    return fail('WebKit 拦截规则没装好：' + (unwrap(ruleError && ruleError.localizedDescription) || '超时'));
  }

  // 连接都走 Peck 的代理（web.rs 起的，查过解析出来的 IP 再连）；配不上就不渲染。WebRTC 不走代理，关掉。
  const data = $.WKWebsiteDataStore.nonPersistentDataStore;
  const proxy = $.nw_proxy_config_create_socksv5($.nw_endpoint_create_host('127.0.0.1', proxyPort));
  if (!proxyPort || !proxy || !data.respondsToSelector('setProxyConfigurations:')) return fail('WebKit 的代理没配上。');
  data.proxyConfigurations = $.NSArray.arrayWithObject(proxy);
  if (Number(data.proxyConfigurations.count) !== 1) return fail('WebKit 的代理没配上。');
  const config = $.WKWebViewConfiguration.alloc.init;
  config.websiteDataStore = data;
  config.preferences.javaScriptCanOpenWindowsAutomatically = false;
  if (!config.preferences.respondsToSelector('_setPeerConnectionEnabled:')) return fail('关不了 WebKit 的 WebRTC。');
  config.preferences._setPeerConnectionEnabled(false);
  config.userContentController.addContentRuleList(list);
  config.userContentController.addUserScript($.WKUserScript.alloc.initWithSourceInjectionTimeForMainFrameOnly(
    '(' + observeActivity.toString() + ')()', $.WKUserScriptInjectionTimeAtDocumentStart, true
  ));

  let loadedAt = 0, loadError = '', failedUrl;
  const failed = (view, navigation, error) => {
    if (Number(error.code) !== -999) {
      loadError = unwrap(error.localizedDescription) || '打开网页失败。';
      failedUrl = unwrap(error.userInfo.objectForKey($.NSURLErrorFailingURLStringErrorKey));
    }
  };
  ObjC.registerSubclass({
    name: 'PeckWebFetchNavigation', protocols: ['WKNavigationDelegate'],
    methods: {
      'webView:didStartProvisionalNavigation:': () => { loadedAt = 0; },
      'webView:didFinishNavigation:': () => { loadedAt = Date.now(); },
      'webView:didFailNavigation:withError:': failed,
      'webView:didFailProvisionalNavigation:withError:': failed,
      'webViewWebContentProcessDidTerminate:': () => { loadError = 'WebKit 网页进程退出了。'; },
    },
  });
  const frame = $.NSMakeRect(0, 0, 1360, 860);
  const web = $.WKWebView.alloc.initWithFrameConfiguration(frame, config);
  const delegate = $.PeckWebFetchNavigation.alloc.init;
  web.navigationDelegate = delegate;
  const win = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer(
    $.NSMakeRect(-30000, -30000, 1360, 860), $.NSWindowStyleMaskBorderless, $.NSBackingStoreBuffered, false
  );
  win.contentView = web;
  win.orderBack(null);

  const evaluate = code => {
    let done = false, value, problem;
    web.evaluateJavaScriptCompletionHandler(code, (result, error) => {
      value = unwrap(result);
      problem = unwrap(error && error.localizedDescription);
      done = true;
    });
    while (!done && !loadError && Date.now() < deadline) spin();
    return { value, problem };
  };
  web.loadRequest($.NSURLRequest.requestWithURL($.NSURL.URLWithString(url)));
  while (!loadError && Date.now() < deadline) {
    if (loadedAt && !web.isLoading) {
      const elapsed = Date.now() - loadedAt;
      if (elapsed >= 750 && (elapsed >= 5000 || evaluate('window.__peckWebQuiet?.() === true').value === true)) {
        const read = evaluate('(' + readDocument.toString() + ')(' + Number(limit) + ')');
        if (loadError) break;
        if (loadedAt && !web.isLoading) return read.value || fail(read.problem || '取渲染后的 DOM 超时。');
      }
    }
    spin();
  }
  return fail(loadError || 'WebKit 没在限时内加载完。', failedUrl);
}
