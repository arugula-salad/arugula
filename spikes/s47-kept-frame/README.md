# S47: the shell keeps the platform page in a frame

Native issue Arugula #655, round 2 of the cohesion review (Doc 97609ee5b7ecb6a0).

**Answer: go, behind a labs flag, on computers only.** Turning it on for everyone waits on the real
Safari and Mac app checks below, and on accepting one new risk: `control.arugula.io` in the address
bar over a page drawn by `app.arugula.io`.

The prototype is [prototype.patch](prototype.patch), 203 added lines, and none of it lands from here.

## How it works

Control's page stays the top page. Projects and Chat on the rail show an iframe of `app.arugula.io`
beside the rail, and Panes hides it. The frame's own rail is hidden. The shell draws its own bell and
Commands over the frame, so **no machine data enters the platform page**. The frame sends the shell
two messages: where it is, and "open Commands". The shell sends it one: go to this path.

## Measured

Chromium 153 on loopback, demo fleet (4 machines), same build with and without the frame.
Raw numbers are in [results/](results/).

| | Today, one window | Kept frame |
|---|---|---|
| Back to a terminal showing text | 387–452 ms | 30–32 ms, the same terminal |
| To the project list, already visited | 50–74 ms | 31–32 ms |
| To the project list, first time | same as above | 80–107 ms |
| First time, with the sign-in hop | not measured | 171 ms |
| New websockets across a round trip | all of them | 0 |

- 30 ms is two animation frames, the lowest this method can read.
- A real fleet costs more today: 626–946 ms for the visible pane on geek (58 panes, 2026-11-01).
- **Memory:** about 15–55 MB more with the frame's own process, against a noise of about 30 MB between
  runs. These were small pages: a project list and a near-empty chat.
- **Idle CPU:** no measurable change: about 34–36% with the frame hidden and without it, in headless
  software rendering.
- Hiding by `visibility` and `display` measured the same. Use `visibility`: `display: none` loses the
  frame's size, so a page could relayout on every return.

## What has to loosen

| Header | Today | Needed | What it gives away |
|---|---|---|---|
| Platform page `frame-ancestors` | `'none'` | control's sites only | Another site is still refused (Chromium, Firefox, WebKit) |
| Platform page `X-Frame-Options` | `DENY` | removed | It can't name an origin; `frame-ancestors` does the job |
| Platform page COOP | `same-origin` | unchanged | A frame ignores it |
| Platform page `Origin-Agent-Cluster` | none | `?1` | Nothing: it adds separation |
| Control's pages | `frame-ancestors 'none'` | unchanged | The keys' page is never framed |
| Control's `frame-src` | `'self' https:` | unchanged in production | The rig needed its http origin added |

## Sign-in and passkeys

- **Cookies and storage:** with hostnames on the public suffix list, a framed page sends and sets its
  cookies as the top-level page does (all engines tested). A made-up TLD gave a false failure in
  WebKit, so the harness uses real suffixes.
- **Hop with a hub session:** works silently, because `frame-ancestors` isn't applied to a redirect
  (all engines tested, and in Chromium on the real shell).
- **Hop with no hub session, and GitHub:** refused in a frame. These have to happen at top level, so
  the frame is only offered once the shell is signed in. Signing out from the frame still needs design.
- **`localStorage` differs in a real WKWebView:** see [Where it was tested](#where-it-was-tested).
- **Passkeys stay out of the frame.** Don't delegate `publickey-credentials-get`. With it, Chrome runs
  the ceremony and reports `crossOrigin: true`, which `crates/control/src/passkey.rs` refuses; S46
  decided that for good reason. Without it the call fails at once as `NotAllowedError`, which
  `refusedHow` already treats as "use Control's dialog". Read and probed, **not clicked through** in
  the prototype. The "Approve on Control" link needs the bridge, since control refuses to load in a frame.

## Keys across the frame

What a compromised `app.arugula.io` could do from the frame:

| Attempt | Result |
|---|---|
| Read the parent's DOM or storage, or use `document.domain` to get there | Refused, all engines including WKWebView |
| Call control's API | Refused by the page's own `connect-src` |
| Send the shell a bad path (`//evil.test`, `javascript:`) | Ignored in the prototype |
| Move the top page, no click | Refused, all engines including WKWebView |
| Move the top page, **after a click, no sandbox** | **Worked** in Chromium, on the real shell |
| The same, with `sandbox` and no `allow-top-navigation*` | Refused |
| Share a process with the keys | **Yes by default** in Chrome; own process with `Origin-Agent-Cluster: ?1` |
| Draw a fake prompt under control's address | **Possible. This is the new risk** |

- **The sandbox is required.** Leaving the app then goes through the shell.
- **Process separation is only shown for Chrome.** Firefox and WebKit report `originAgentCluster: false`
  and the process split wasn't measured there. The shell's CSP already lets browser blocks frame any
  https site, so a shared process with strangers' pages may not be new there.
- **The spoofing risk can be reduced, not closed.** Today the address bar tells you which page you're
  on. Mitigation: the rail and bell are always the shell's, and a frame can't draw over them, and
  anything asking for trust stays in the shell's chrome. A fake prompt still gets nothing by itself:
  passkeys use the browser's own sheet.

## URLs

Tested on the real shell in Chromium.

- The address bar says `control.arugula.io/?machine=…&at=/boards`, and follows the frame.
- Reload comes back to the same page. Back and Forward work across the shell and the frame.
- A link to `app.arugula.io/...` from outside still opens at top level, with panes unloaded. Sending
  those through `?at=` is a decision for the build.

## Where it was tested

| Surface | Browser facts | Real shell |
|---|---|---|
| Chromium 153 | Yes, with a virtual authenticator | Yes |
| Chrome 153 | Process split only | No |
| Firefox 155 | Yes, no authenticator | **No:** stuck at "Connecting to Arugula control…" on this rig |
| Playwright WebKit (Linux) | Yes, no WebAuthn in this build | **No:** browser closed during sign-in |
| WKWebView, macOS 15.5 | Yes, off-screen, throwaway store | No |

**WKWebView differs in one way:** the framed page's `localStorage` wasn't visible to the top-level page.
Playwright's WebKit build shared it. Cookies were shared, so the session is fine, but remembered things
(last page, recent items) would be separate from a top-level `app.arugula.io` there. The passkey call
there said "The document is not focused", which tells us nothing.

**Not tested:** Safari itself, the real Mac app, WebKitGTK, WebView2, any phone, real passkey providers,
a live call in a hidden frame (camera and mic would stay on while you're in Panes), heavy pages, and
Firefox and Safari on the real shell.

## Build tickets, if it's a go

1. **Headers:** `frame-ancestors` naming control's sites, no `X-Frame-Options`, `Origin-Agent-Cluster`.
   Update the tests that assert `'none'`. No visible change, so it can ship first.
2. **The frame, behind a labs flag:** computers only, only when the shell is on control's origin and
   signed in. `sandbox`, `?at=`, the rail, Back and Forward, `visibility` hiding.
3. **The bridge:** a closed set of messages, origins pinned on both sides (the prototype's frame falls
   back to `*`), every field checked.
4. **Links out of the frame:** about 20 uses of `controlHref`/`controlOrigin()`, and three full-page
   trips (GitHub install, GitHub authorize, Stripe checkout) go through the bridge or a new tab.
5. **Sign-in and sign-out at the edges:** signed out in the frame falls back to top level, and
   sign-out clears both.
6. **The bell and Commands over the frame:** forward the other shell keys, and fix the layout at narrow widths.
7. **The merged Inbox:** the shell's drawer with the frame's Inbox. Not prototyped.
8. **Calls in a hidden frame:** decide, then make it visible that a call is live.
9. **The real-surface sheet (the gate for turning it on):** Safari, the Mac app, Windows, real passkey
   providers, on a staging control with TLS.
10. **Amend aru-079:** origins stay separate, and control may now hold the page in a frame.

## Running it

- `harness/probe.mjs`: browser facts in Chromium, Firefox and WebKit, against `harness/serve.mjs`.
- `harness/process.mjs [chrome]`, `harness/stranger.mjs`: process split and framing by another site.
- `harness/wk-probe.swift`: the same facts in a WKWebView. Run it as an app bundle.
- `harness/measure.mjs`: the real shell, with `prototype.patch` applied and the demo fleet on
  `arugula.localhost` and `app.arugula.localhost`, plus `*.localhost` allowed as a private host.
