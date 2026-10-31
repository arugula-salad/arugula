#!/usr/bin/env python3
"""S46 (#382): does WebKitGTK (webkit2gtk-4.1, what Tauri's Linux window
runs on) offer WebAuthn? Loads the harness at http://localhost (a secure
context) and reports what the page sees, plus WebKit's own feature flags
that mention WebAuthn, and tries turning those on.

    node serve.mjs --rp localhost --app x.invalid --stranger y.invalid --http 8473 &
    xvfb-run -a python3 probe-webkitgtk.py http://localhost:8473/
"""
import json
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

URL = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8473/"

PROBE = """
const out = { ua: navigator.userAgent, secure: isSecureContext,
  credentials: typeof navigator.credentials, pkc: typeof window.PublicKeyCredential };
if (window.PublicKeyCredential) {
  try { out.uvpaa = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable(); } catch (e) { out.uvpaa = String(e); }
  try { out.caps = PublicKeyCredential.getClientCapabilities ? await PublicKeyCredential.getClientCapabilities() : 'none'; } catch (e) { out.caps = String(e); }
}
if (navigator.credentials && navigator.credentials.get) {
  const ac = new AbortController(); setTimeout(() => ac.abort(), 3000);
  const t = performance.now();
  try { await navigator.credentials.get({ publicKey: { challenge: new Uint8Array(32), userVerification: 'required' }, signal: ac.signal }); out.get = 'a passkey?'; }
  catch (e) { out.get = e.name + ' after ' + Math.round(performance.now() - t) + ' ms: ' + e.message; }
}
return JSON.stringify(out);
"""

results = []


def features(settings):
    """WebKit's feature flags whose name mentions WebAuthn (2.42+ API)."""
    found = []
    try:
        fl = WebKit2.Settings.get_all_features()
    except AttributeError:
        return found
    for i in range(fl.get_length()):
        f = fl.get(i)
        text = f"{f.get_identifier()} {f.get_name() or ''}"
        if "webauthn" in text.lower() or "credential" in text.lower():
            found.append((f, f.get_identifier(), f.get_status().value_nick, settings.get_feature_enabled(f)))
    return found


def run(enable):
    settings = WebKit2.Settings()
    flags = features(settings)
    if enable:
        for f, *_ in flags:
            settings.set_feature_enabled(f, True)
    view = WebKit2.WebView.new_with_settings(settings)
    win = Gtk.Window()
    win.add(view)
    win.show_all()

    def done(v, res, _):
        try:
            r = v.call_async_javascript_function_finish(res)
            results.append((enable, [(i, s, e) for _, i, s, e in flags], json.loads(r.to_string())))
        except GLib.Error as e:
            results.append((enable, [], {"error": str(e)}))
        win.destroy()
        if enable:
            Gtk.main_quit()
        else:
            run(True)

    def loaded(v, ev):
        if ev == WebKit2.LoadEvent.FINISHED:
            v.call_async_javascript_function(PROBE, -1, None, None, None, None, done, None)

    view.connect("load-changed", loaded)
    view.load_uri(URL)


print(f"WebKitGTK {WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}")
run(False)
GLib.timeout_add_seconds(30, Gtk.main_quit)
Gtk.main()
for enable, flags, r in results:
    print(f"\n## {'with every WebAuthn flag turned on' if enable else 'as shipped'}")
    for ident, status, on in flags:
        print(f"- feature {ident}: status {status}, enabled {on}")
    for k, v in r.items():
        print(f"- {k}: {v}")
