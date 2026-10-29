# S43: outline presence, live camera outlines of who's around (Arugula #310)

Run on 2026-10-28 on geek, from `main` at d69da7bc, on branch `s43-outline-presence`. Inspired by
[lowbandwidth.video](https://lowbandwidth.video/): each browser traces its camera into line outlines
and sends only the lines. The follow-up for real low-res video on the platform is filed as its own
issue (see #310's closing comment).

**Result:** it works end to end and is cheap. A page traces its camera into outlines (2–8 ms a frame),
posts them to control, control streams them to the people who share a team, and a **People** block
draws a tile per teammate. With *colour by what it is* on, MediaPipe in the page labels each frame,
so you, your face, your chair and your screen draw in different colours. Jake tried it from
jake-air ("this is okay"); the defaults went to 4 fps and more detail after that. **Nothing here is
a product decision**: no Labs flag, the relay works on one platform machine, and the shells' camera
access is untested.

![The People block with the fake camera: you, and sam and riley echoing you](https://share.files.inevitable.fyi/2026-10-29/s43-people-block.png)

## What's on the branch

> Since #315 the presence code is in packages: `presence/core` (encoder, rig, renderers, characters) and
> `presence/client` (the room client over a transport); the web client keeps the People block, chat
> avatars and a thin `web/src/shared/outline/store.ts`. Paths below are updated.

| Piece | Where |
|---|---|
| Encoder: blur, Sobel, thinning, hysteresis (thresholds from the frame, steadied by the last frame), chains, Douglas–Peucker, a varint frame (v2: a class byte per line), deflate-raw when smaller | `presence/core/src/outline/encode.ts`, `frame.ts`, `render.ts`, `camera.ts` |
| Depth: Depth Anything V2 Small (4-bit, Apache-2.0) through transformers.js on WebGPU, input cut to 266 px, run beside the frames about twice a second. Depth jumps count as edges, far lines draw fainter and thinner (frame v3: a nearness byte per line), and a label well behind you is dropped (a poster isn't you, a chair across the room isn't yours). No WebGPU, no depth: the block says so | `presence/core/src/outline/depth.ts`, `encode.ts` (`byDepth`) |
| Live avatars: while someone's camera is on, their outline replaces their picture wherever chat shows it (messages, DMs, members), yours mirrored | `web/src/shared/chat/avatar.tsx` (`LiveFace`) |
| Labelling: DeepLab v3 (21 Pascal VOC classes, a class per grid cell; a line takes the class it runs along and splits where it changes) and the face landmarker (eyes, brows, irises, lips) | `presence/core/src/outline/labels.ts`, `classes.ts`; assets by `presence/core/scripts/assets.sh` |
| People block (`BlockType::People`, config `{}`, drawn only while shown); *⋯ → Open beside → People* | `web/src/shell/blocks/people.tsx`, `crates/daemon/src/people_block.rs` |
| Relay: `POST /api/people/outline` (≤32 KiB, ≥70 ms apart), `POST …/outline/off`, `GET /api/people/outlines` (SSE); latest frame per person in memory only, visibility as presence's, behind chat's gate | `crates/platform/src/chat/outlines.rs`, `crates/control/routes.toml` |
| Trying it alone: `DEV_CONTROL_OUTLINES=1` (sam and riley echo your frames, and beat presence); `dev-browser --camera [FILE]` | `web/dev-control.ts`, `web/dev-browser.ts` |
| Load probe | `web/scripts/outline-load.ts` |

Control's page CSP gains `script-src 'self' 'wasm-unsafe-eval'` for MediaPipe's and ONNX Runtime's
Wasm, and control serves `.mjs` as JavaScript and `.wasm` as `application/wasm`.
`@mediapipe/tasks-vision` 1.2.0 and `@huggingface/transformers` 4.3.2 (both Apache-2.0) load only when
labelling or depth starts; their Wasm and models (~65 MB in all, fetched by
`web/scripts/outline-assets.sh`) are served by control. transformers.js runs with its Wasm cache
off: the cache makes a `blob:` script, which the CSP refuses.

![Live avatars in #general: the people whose cameras are on show as outlines](https://share.files.inevitable.fyi/2026-10-29/s43-live-avatars.png)

## Numbers

Encoding: 2–8 ms a frame on the main thread (192×144); 1.7 ms for a busy synthetic room in node.
Labelling: 171 ms a frame in headless Chrome without a GPU (throttled to a third of the frame
interval, reusing its last answer between); a real GPU wasn't measured. Depth needs WebGPU, which
headless Chrome on geek doesn't have, so its speed is unmeasured here too.

Load probe, local debug-build control, jake posting incompressible frames to 8 streams:

| Frame | fps | In (kbps) | Out (kbps) | Delivered | p50 / p95 | Control CPU |
|---|---|---|---|---|---|---|
| 1 KB | 4 | 33 | 350 | 100% | 2 / 4 ms | 1% |
| 4 KB | 10 | 328 | 3,495 | 100% | 2 / 3 ms | 3% |
| 16 KB | 10 | 1,311 | 13,981 | 100% | 2 / 4 ms | 5% |
| 31 KB | 14 | 3,584 | 38,229 | 99% (the 70 ms gap) | 3 / 3 ms | 10% |

The relay isn't the limit at team scale. In practice the limits come in this order: egress cost
(out = in × 4/3 × viewers; 10 people at 2 KB × 4 fps ≈ 7.7 Mbit/s, ~$12/month on Fly at 8 h/day),
viewers' downlink, the one platform-wide broadcast channel (every stream wakes for every frame:
per-team topics fix it), a POST per frame (headers ≈ the frame at 1–2 KB: a WebSocket fixes it),
and frames held on one machine. Sweet spot: ≤ 4 KB a frame at ≤ 10 fps.

## Path to shipping, if it's wanted

1. The relay on the Elixir server: a channel per team, PubSub, Presence; a WebSocket upstream.
2. A Labs flag, the encoder and labelling in a worker, a privacy-page line (off by default, only
   lines leave the page, control keeps nothing).
3. Outlines as avatars wherever the presence dot shows (done in chat on the branch; the stream
   then runs whenever chat is open, which is the cost to watch).
4. The shells: the macOS camera entitlement and usage string, a WebKitGTK `getUserMedia` check, the
   phone.

## Reproduce

In the worktree, with `CARGO_TARGET_DIR=$PWD/target`: `web/scripts/outline-assets.sh`, then
`DEV_CONTROL_CHAT=1 DEV_CONTROL_LOGINS=sam,riley DEV_CONTROL_OUTLINES=1 just dev-control`
(add `DEV_CONTROL_HOST=<tailnet address>` and open it from another machine with the Chrome command
dev-control prints, for a real camera). Give each instance a state dir that isn't a prefix of
another's: dev-control's `pkill -f <dir>` takes both down.
