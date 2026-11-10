// S47 (#655): the frame facts in a real WKWebView on macOS, the desktop app's
// engine. Off-screen window, a throwaway data store, nothing kept. Every name
// goes through the CONNECT proxy to serve.mjs, so nothing leaves the tunnel.
//   swiftc -o wk-probe wk-probe.swift && launchctl asuser $(id -u) ./wk-probe out.json

import Cocoa
import WebKit
import Network

let CONTROL = "https://control.example.com"
let APP = "https://app.example.com"
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "wk-probe.json"
var results: [String: Any] = [:]

final class Nav: NSObject, WKNavigationDelegate {
  var loaded: ((Error?) -> Void)?
  func webView(_ w: WKWebView, respondTo c: URLAuthenticationChallenge) -> (URLSession.AuthChallengeDisposition, URLCredential?) {
    if let t = c.protectionSpace.serverTrust { return (.useCredential, URLCredential(trust: t)) }
    return (.performDefaultHandling, nil)
  }
  func webView(_ w: WKWebView, didReceive c: URLAuthenticationChallenge, completionHandler h: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
    let (d, cr) = webView(w, respondTo: c); h(d, cr)
  }
  func webView(_ w: WKWebView, didFinish n: WKNavigation!) { loaded?(nil); loaded = nil }
  func webView(_ w: WKWebView, didFailProvisionalNavigation n: WKNavigation!, withError e: Error) { loaded?(e); loaded = nil }
}

let HELPERS = """
const addFrame = (src, attrs = {}, timeout = 4000) => new Promise((ok) => {
  const f = document.createElement("iframe");
  f.id = "f"; for (const k in attrs) f.setAttribute(k, attrs[k]); f.src = src;
  const done = (v) => { removeEventListener("message", on); ok(v); };
  const on = (e) => { if (e.source === f.contentWindow && e.data && (e.data.loaded || e.data.hop)) done({ loaded: e.data.loaded || "hop page", origin: e.origin, docRequest: e.data.docRequest }); };
  addEventListener("message", on);
  document.body.append(f);
  setTimeout(() => done({ blocked: true }), timeout);
});
const inFrame = (code, timeout = 5000) => new Promise((ok) => {
  const id = Math.random();
  const f = document.getElementById("f");
  const on = (e) => { if (e.data && e.data.id === id) { removeEventListener("message", on); ok(e.data.err ? { err: e.data.err } : e.data.ok); } };
  addEventListener("message", on);
  f.contentWindow.postMessage({ run: code, id }, "*");
  setTimeout(() => { removeEventListener("message", on); ok({ err: "no answer" }); }, timeout);
});
const rm = () => document.getElementById("f")?.remove();
const SANDBOX = "allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-modals allow-downloads";
const r = {};
"""

let STEP1 = HELPERS + """
r["ua"] = navigator.userAgent;
r["today's headers frame?"] = await addFrame(`\(APP)/?framing=none`); rm();
r["frame-ancestors control, sandboxed frame"] = await addFrame(`\(APP)/?framing=control`, { sandbox: SANDBOX });
await inFrame(`fetch("/set-cookie?name=framed_set").then(r=>r.status)`);
r["frame's cookie comes back to it"] = await inFrame(`fetch("/echo-cookies").then(r=>r.json())`);
await inFrame(`localStorage.setItem("from_frame","1"); true`);
r["frame reads parent.document"] = await inFrame(`parent.document.title`);
r["frame sets document.domain, reads parent"] = await inFrame(`document.domain="example.com"; parent.document.title`);
r["sandboxed frame moves the top page"] = await inFrame(`(()=>{ try { top.location = "https://stranger.example.org/"; return "called"; } catch (e) { return e.name } })()`);
await new Promise((s) => setTimeout(s, 700));
r["  top page after"] = location.href;
rm();
await addFrame(`\(APP)/?framing=control`);
r["unsandboxed frame moves the top page, no click"] = await inFrame(`(()=>{ try { top.location = "https://stranger.example.org/#moved"; return "called"; } catch (e) { return e.name } })()`);
await new Promise((s) => setTimeout(s, 700));
r["  top page after (2)"] = location.href;
rm();
r["hop: hub redirects under 'none'"] = await addFrame(`\(APP)/hop-to-control?session=1`); rm();
r["hop: hub's own page under 'none'"] = await addFrame(`\(APP)/hop-to-control`); rm();
r["GitHub-like stranger in a frame"] = await addFrame(`https://stranger.example.org/`); rm();
await addFrame(`\(APP)/?framing=control`, { sandbox: SANDBOX });
document.getElementById("f").style.display = "none";
await new Promise((s) => setTimeout(s, 300));
r["display:none frame"] = await inFrame(`new Promise((ok)=>{ let t=0, raf=0; const i=setInterval(()=>t++,100); const loop=()=>{raf++;requestAnimationFrame(loop)}; requestAnimationFrame(loop); setTimeout(()=>{clearInterval(i); ok({visibility:document.visibilityState, width:innerWidth, timersIn2s:t, rafIn2s:raf})},2000)})`, 6000);
document.getElementById("f").style.display = "";
const before = history.length;
await inFrame(`history.pushState({},"","/p/one"); history.pushState({},"","/p/two"); location.pathname`);
r["history"] = { before, after: history.length };
r["navigator.credentials in the top page"] = typeof navigator.credentials;
r["navigator.credentials in the frame"] = await inFrame(`typeof navigator.credentials`);
r["passkey get in the frame, no allow="] = await inFrame(`typeof navigator.credentials === "undefined" ? "no API" : navigator.credentials.get({publicKey:{challenge:new Uint8Array(32),rpId:"control.example.com",userVerification:"required",timeout:2000}, signal:AbortSignal.timeout(2500)}).then(()=>"got one")`, 7000);
rm();
return JSON.stringify(r);
"""

let STEP2 = """
const r = {};
r["top-level app sees the frame's cookie"] = await fetch("/echo-cookies").then((x) => x.json());
r["top-level app sees the frame's localStorage"] = localStorage.getItem("from_frame");
await fetch("/set-cookie?name=top_set");
return JSON.stringify(r);
"""

let STEP3 = HELPERS + """
r["new framed document carries the cookies"] = await addFrame(`\(APP)/?framing=control`, { sandbox: SANDBOX });
return JSON.stringify(r);
"""

@MainActor func run() async {
  let store = WKWebsiteDataStore.nonPersistent()
  store.proxyConfigurations = [ProxyConfiguration(httpCONNECTProxy: NWEndpoint.hostPort(host: "127.0.0.1", port: 8474))]
  let cfg = WKWebViewConfiguration()
  cfg.websiteDataStore = store
  let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800), configuration: cfg)
  let nav = Nav()
  web.navigationDelegate = nav
  let window = NSWindow(contentRect: NSRect(x: -5000, y: -5000, width: 1200, height: 800), styleMask: [.borderless], backing: .buffered, defer: false)
  window.contentView = web
  window.orderBack(nil)

  func load(_ url: String) async throws {
    try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
      nav.loaded = { e in if let e { c.resume(throwing: e) } else { c.resume() } }
      web.load(URLRequest(url: URL(string: url)!))
    }
  }
  func step(_ js: String) async throws {
    let v = try await web.callAsyncJavaScript(js, contentWorld: .page)
    if let s = v as? String, let d = s.data(using: .utf8), let o = try JSONSerialization.jsonObject(with: d) as? [String: Any] {
      results.merge(o) { a, _ in a }
    }
  }
  do {
    try await load("\(CONTROL)/")
    try await step(STEP1)
    try await load("\(APP)/?framing=control")
    try await step(STEP2)
    try await load("\(CONTROL)/")
    try await step(STEP3)
  } catch { results["failed"] = String(describing: error) }
  let data = try! JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
  try! data.write(to: URL(fileURLWithPath: out))
  exit(0)
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
DispatchQueue.main.asyncAfter(deadline: .now() + 90) { results["failed"] = "watchdog"; let d = try! JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted]); try! d.write(to: URL(fileURLWithPath: out)); exit(3) }
Task { @MainActor in await run() }
app.run()
