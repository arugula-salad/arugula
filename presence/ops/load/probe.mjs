#!/usr/bin/env node
// The relay's load probe (#318's P12): N rooms × M members as real
// WebSocket clients speaking the protocol (../../PROTOCOL.md), each
// sending rig-sized frames at the real rate (the client's 12 a second; the
// relay's floor is one per 70 ms), text, waves
// and reactions, while it measures fan-out latency (sent → heard by
// another member), frames missing, and errors. Node 24, no packages.
//
//   node probe.mjs --url http://localhost:4214 --rooms 10 --members 8
//
// From two machines at once, into the same rooms: give both the same
// --prefix and --start-at, a different --id, and split the members
// (--members 4 on each, for rooms of 8). Latency between clients on one
// machine is exact (one clock); between machines it's by wall clock, as
// good as the machines' NTP. See README.md.

import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
import { availableParallelism, hostname } from "node:os";
import { createHmac } from "node:crypto";
import { fileURLToPath } from "node:url";

// Latency histogram: 0.1 ms buckets up to 10 s, then one overflow bucket.
const RES = 0.1;
const BUCKETS = 100_000;
const MAGIC = [0x4c, 0x50]; // "LP": a probe's frame

if (isMainThread) await main();
else await worker(workerData);

// ---------------------------------------------------------------- main

function usage() {
  return `usage: node probe.mjs [options]
  --url URL           the relay (http[s]://host[:port]; default http://localhost:4214)
  --rooms N           rooms (default 10)
  --members M         members per room from this machine (default 8)
  --duration S        seconds measured once everyone's in (default 60)
  --ramp S            seconds over which everyone connects (default 10)
  --warmup S          seconds after the ramp before measuring (default 5)
  --gap MS            ms between a member's frames (default 83: the client's 12 fps
                      rig; 70 is the relay's floor, where network jitter makes it
                      drop some as too soon)
  --frame-bytes B     bytes a frame (default 32: a rig frame is 22–41)
  --say-every S       a text message per member about this often (default 20; 0: none)
  --react-every S     a reaction per member about this often (default 10; 0: none)
  --wave-every S      a wave per member about this often (default 30; 0: none)
  --prefix P          room names are P-0 … P-(N-1) (default load)
  --id ID             this machine's name in the rooms (default the hostname)
  --token MODE        demo (POST /api/demo/token; default) or app:<id>, signing
                      with the secret in PROBE_APP_SECRET (a PRESENCE_APPS app)
  --workers W         threads (default: one per 100 clients, at most the CPUs)
  --start-at T        Unix ms (or an ISO time) to start connecting at, for runs
                      from several machines at once
  --metrics URL       the relay's /metrics, read before and after (local runs)
  --json              print the summary as JSON too`;
}

function args(argv) {
  const o = {
    url: "http://localhost:4214",
    rooms: 10,
    members: 8,
    duration: 60,
    ramp: 10,
    warmup: 5,
    gap: 83,
    frameBytes: 32,
    sayEvery: 20,
    reactEvery: 10,
    waveEvery: 30,
    prefix: "load",
    id: hostname().split(".")[0],
    token: "demo",
    workers: 0,
    startAt: 0,
    metrics: "",
    json: false,
  };
  const num = new Set(["rooms", "members", "duration", "ramp", "warmup", "gap", "frameBytes", "sayEvery", "reactEvery", "waveEvery", "workers"]);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") {
      console.log(usage());
      process.exit(0);
    }
    if (a === "--json") {
      o.json = true;
      continue;
    }
    const key = a.replace(/^--/, "").replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    if (!a.startsWith("--") || !(key in o)) throw new Error(`unknown option ${a}\n${usage()}`);
    const v = argv[++i];
    if (v === undefined) throw new Error(`${a} needs a value`);
    if (key === "startAt") o.startAt = /^\d+$/.test(v) ? Number(v) : Date.parse(v);
    else o[key] = num.has(key) ? Number(v) : v;
  }
  if (o.frameBytes < 22) throw new Error("--frame-bytes is at least 22 (the probe's own header)");
  if (o.token !== "demo" && !(o.token.startsWith("app:") && process.env.PROBE_APP_SECRET))
    throw new Error("--token app:<id> needs PROBE_APP_SECRET");
  return o;
}

async function main() {
  let o;
  try {
    o = args(process.argv.slice(2));
  } catch (e) {
    console.error(e.message);
    process.exit(2);
  }
  const clients = o.rooms * o.members;
  const workers = o.workers || Math.max(1, Math.min(availableParallelism(), Math.ceil(clients / 100)));
  // A small number per machine for the frame header (other machines' frames are told apart by it).
  const tag = [...o.id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) & 0xffff, 7);

  const metricsBefore = o.metrics ? await scrape(o.metrics) : null;
  if (o.startAt) {
    const wait = o.startAt - Date.now();
    if (wait > 0) {
      log(`waiting ${(wait / 1000).toFixed(1)} s for --start-at`);
      await sleep(wait);
    }
  }

  log(`${o.rooms} rooms × ${o.members} members = ${clients} clients from ${o.id}, ${workers} threads → ${o.url}`);
  const t0 = Date.now();
  const rampMs = o.ramp * 1000;
  const parts = Array.from({ length: workers }, () => []);
  for (let i = 0; i < clients; i++) parts[i % workers].push(i);

  const threads = parts.map(
    (idx) =>
      new Worker(fileURLToPath(import.meta.url), {
        workerData: { o, tag, idx, t0, rampMs, clients },
      }),
  );
  const joined = new Array(workers).fill(0);
  const done = threads.map(
    (w, i) =>
      new Promise((resolve, reject) => {
        w.on("message", (m) => {
          if (m.type === "joined") joined[i] = m.n;
          if (m.type === "stats") resolve(m.stats);
        });
        w.on("error", reject);
        w.on("exit", (code) => code !== 0 && reject(new Error(`thread exited ${code}`)));
      }),
  );

  const progress = setInterval(() => log(`joined ${joined.reduce((a, b) => a + b, 0)}/${clients}`), 5000);
  await sleep(rampMs + o.warmup * 1000);
  clearInterval(progress);
  const joinedAtStart = joined.reduce((a, b) => a + b, 0);
  log(`measuring for ${o.duration} s with ${joinedAtStart}/${clients} joined`);
  for (const w of threads) w.postMessage({ type: "measure" });
  await sleep(o.duration * 1000);
  for (const w of threads) w.postMessage({ type: "stop" });
  const all = await Promise.all(done);
  for (const w of threads) w.terminate();

  const s = merge(all);
  const metricsAfter = o.metrics ? await scrape(o.metrics) : null;
  const summary = summarize(o, s, clients, joinedAtStart, metricsBefore, metricsAfter);
  print(summary);
  if (o.json) console.log(JSON.stringify(summary));
}

function merge(all) {
  const s = blank();
  for (const x of all) {
    for (const k of Object.keys(s)) {
      if (k === "connectMs") s[k].push(...x[k]);
      else if (s[k] instanceof Uint32Array) for (let i = 0; i < x[k].length; i++) s[k][i] += x[k][i];
      else s[k] += x[k];
    }
  }
  return s;
}

function pct(h, p) {
  let total = 0;
  for (const c of h) total += c;
  if (!total) return null;
  const want = Math.ceil(total * p);
  let n = 0;
  for (let i = 0; i < h.length; i++) {
    n += h[i];
    if (n >= want) return i === BUCKETS ? Infinity : (i + 1) * RES;
  }
  return null;
}

function maxOf(h) {
  for (let i = h.length - 1; i >= 0; i--) if (h[i]) return i === BUCKETS ? Infinity : (i + 1) * RES;
  return null;
}

function lat(h) {
  const n = h.reduce((a, b) => a + b, 0);
  return { n, p50: pct(h, 0.5), p95: pct(h, 0.95), p99: pct(h, 0.99), max: maxOf(h) };
}

function summarize(o, s, clients, joinedAtStart, before, after) {
  const secs = o.duration;
  const sorted = s.connectMs.sort((a, b) => a - b);
  const q = (p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))] : null);
  const relay =
    before && after
      ? Object.fromEntries(
          ['presence_frames_total{result="relayed"}', 'presence_frames_total{result="too_soon"}', 'presence_frames_total{result="too_big"}', "presence_joins_total", "presence_rate_limited_total"].map(
            (k) => [k, (after[k] ?? 0) - (before[k] ?? 0)],
          ),
        )
      : null;
  return {
    id: o.id,
    url: o.url,
    rooms: o.rooms,
    members: o.members,
    clients,
    seconds: secs,
    connected: s.connected,
    joinedAtStart,
    connectMs: { p50: q(0.5), p95: q(0.95), max: sorted.at(-1) ?? null },
    errors: { token: s.tokenErrors, token429: s.token429, socket: s.socketErrors, join: s.joinErrors, closed: s.closed, channel: s.channelErrors },
    frames: {
      sent: s.framesSent,
      sentPerS: Math.round(s.framesSent / secs),
      received: s.framesReceived,
      receivedPerS: Math.round(s.framesReceived / secs),
      missing: s.framesMissing,
      missingPct: s.framesReceived ? (100 * s.framesMissing) / (s.framesReceived + s.framesMissing) : 0,
      latency: lat(s.latLocal),
      latencyOtherMachines: lat(s.latRemote),
    },
    say: { sent: s.saySent, ok: s.sayOk, refused: s.sayRefused, heard: s.sayHeard, latency: lat(s.latSay) },
    react: { sent: s.reactSent, heard: s.reactHeard },
    wave: { sent: s.waveSent, heard: s.waveHeard },
    relay,
  };
}

function ms(v) {
  return v === null ? "–" : v === Infinity ? ">10s" : `${v.toFixed(1)} ms`;
}

function print(r) {
  const l = (x) => `p50 ${ms(x.p50)}  p95 ${ms(x.p95)}  p99 ${ms(x.p99)}  max ${ms(x.max)}  (n=${x.n})`;
  console.log(`
arugula.stream load probe: ${r.id} → ${r.url}
  ${r.rooms} rooms × ${r.members} members = ${r.clients} clients; measured ${r.seconds} s
  connected ${r.connected}/${r.clients} (${r.joinedAtStart} in when measuring began); connect+join p50 ${ms(r.connectMs.p50)} p95 ${ms(r.connectMs.p95)} max ${ms(r.connectMs.max)}
  errors: token ${r.errors.token} (429s retried ${r.errors.token429}), socket ${r.errors.socket}, join ${r.errors.join}, closed early ${r.errors.closed}, channel ${r.errors.channel}
  frames: sent ${r.frames.sent} (${r.frames.sentPerS}/s), heard ${r.frames.received} (${r.frames.receivedPerS}/s), missing ${r.frames.missing} (${r.frames.missingPct.toFixed(3)}%)
    fan-out, same machine:   ${l(r.frames.latency)}
    fan-out, other machines: ${l(r.frames.latencyOtherMachines)}
  say: sent ${r.say.sent}, ok ${r.say.ok}, refused ${r.say.refused}, heard ${r.say.heard}; ${l(r.say.latency)}
  react: sent ${r.react.sent}, heard ${r.react.heard}; wave: sent ${r.wave.sent}, heard ${r.wave.heard}`);
  if (r.relay) console.log(`  relay /metrics during the run: ${Object.entries(r.relay).map(([k, v]) => `${k} ${v}`).join(", ")}`);
}

async function scrape(url) {
  try {
    const text = await (await fetch(url)).text();
    const out = {};
    for (const line of text.split("\n")) {
      if (!line || line.startsWith("#")) continue;
      const i = line.lastIndexOf(" ");
      out[line.slice(0, i)] = Number(line.slice(i + 1));
    }
    return out;
  } catch (e) {
    log(`couldn't read ${url}: ${e.message}`);
    return null;
  }
}

// ---------------------------------------------------------------- worker

function blank() {
  return {
    connected: 0,
    connectMs: [],
    tokenErrors: 0,
    token429: 0,
    socketErrors: 0,
    joinErrors: 0,
    closed: 0,
    channelErrors: 0,
    framesSent: 0,
    framesReceived: 0,
    framesMissing: 0,
    saySent: 0,
    sayOk: 0,
    sayRefused: 0,
    sayHeard: 0,
    reactSent: 0,
    reactHeard: 0,
    waveSent: 0,
    waveHeard: 0,
    latLocal: new Uint32Array(BUCKETS + 1),
    latRemote: new Uint32Array(BUCKETS + 1),
    latSay: new Uint32Array(BUCKETS + 1),
  };
}

async function worker({ o, tag, idx, t0, rampMs, clients }) {
  const s = blank();
  let measuring = false;
  let stopping = false;
  const live = [];
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  const now = () => performance.timeOrigin + performance.now();
  const record = (h, v) => (h[v < 0 ? 0 : Math.min(BUCKETS, Math.floor(v / RES))] += 1);

  parentPort.on("message", (m) => {
    if (m.type === "measure") {
      Object.assign(s, { ...blank(), connected: s.connected, connectMs: s.connectMs, tokenErrors: s.tokenErrors, token429: s.token429, socketErrors: s.socketErrors, joinErrors: s.joinErrors });
      for (const c of live) c.seen.clear();
      measuring = true;
    }
    if (m.type === "stop") {
      measuring = false;
      stopping = true;
      for (const c of live) c.stop();
      parentPort.postMessage({ type: "stats", stats: s });
    }
  });

  const base = o.url.replace(/\/$/, "");
  const wsBase = base.replace(/^http/, "ws") + "/socket/websocket?vsn=2.0.0&token=";

  async function token(room, name, sub) {
    if (o.token.startsWith("app:")) {
      const b64 = (x) => Buffer.from(JSON.stringify(x)).toString("base64url");
      const h = b64({ alg: "HS256", typ: "JWT" });
      const p = b64({ app: o.token.slice(4), room, sub, name, exp: Math.floor(Date.now() / 1000) + 6 * 3600 });
      return `${h}.${p}.${createHmac("sha256", process.env.PROBE_APP_SECRET).update(`${h}.${p}`).digest("base64url")}`;
    }
    for (let tries = 0; tries < 20; tries++) {
      const r = await fetch(`${base}/api/demo/token`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ room, name }),
      });
      if (r.status === 429) {
        s.token429++;
        await sleep(1000 * Number(r.headers.get("retry-after") || 5));
        continue;
      }
      if (!r.ok) throw new Error(`demo token: ${r.status}`);
      return (await r.json()).token;
    }
    throw new Error("demo token: still rate limited");
  }

  function client(i) {
    const room = `${o.prefix}-${Math.floor(i / o.members)}`;
    const topic = `room:${room}`;
    const name = `${o.id}-${i}`;
    const c = { seen: new Map(), stop: () => {} };
    let ws;
    let ref = 1;
    let me = null;
    let others = new Set();
    let seq = 0;
    let lastSend = 0;
    const timers = [];
    const topicB = enc.encode(topic);
    const frameEv = enc.encode("frame");
    const joinRef = "1";
    const joinRefB = enc.encode(joinRef);
    const pending = new Map(); // ref => kind

    c.stop = () => {
      for (const t of timers) clearTimeout(t), clearInterval(t);
      try {
        ws?.close(1000);
      } catch {}
    };

    const sendJson = (event, payload, kind) => {
      const r = String(++ref);
      if (kind) pending.set(r, kind);
      ws.send(JSON.stringify([joinRef, r, topic, event, payload]));
    };

    const sendFrame = () => {
      const t = now();
      if (t - lastSend < o.gap * 0.9) return;
      lastSend = t;
      const data = new Uint8Array(o.frameBytes);
      const dv = new DataView(data.buffer);
      data[0] = 0; // raw
      data[1] = 0x40; // rig
      data[2] = MAGIC[0];
      data[3] = MAGIC[1];
      dv.setUint16(4, tag);
      dv.setUint32(6, i);
      dv.setUint32(10, ++seq);
      dv.setFloat64(14, t);
      // Phoenix V2 binary push: kind, sizes, join ref, ref (none), topic, event, data.
      const head = 5 + joinRefB.length + topicB.length + frameEv.length;
      const msg = new Uint8Array(head + data.length);
      msg.set([0, joinRefB.length, 0, topicB.length, frameEv.length]);
      let p = 5;
      msg.set(joinRefB, p), (p += joinRefB.length);
      msg.set(topicB, p), (p += topicB.length);
      msg.set(frameEv, p), (p += frameEv.length);
      msg.set(data, p);
      ws.send(msg);
      if (measuring) s.framesSent++;
    };

    const onFrame = (buf) => {
      // kind 0 push: join ref, topic and event sizes, then the relay's
      // `len:u8 sub` and the frame.
      const b = new Uint8Array(buf);
      if (b[0] !== 0) return;
      const off = 4 + b[1] + b[2] + b[3];
      const subLen = b[off];
      const f = off + 1 + subLen;
      if (b.length < f + 22 || b[f + 2] !== MAGIC[0] || b[f + 3] !== MAGIC[1]) return;
      const dv = new DataView(b.buffer, b.byteOffset + f);
      const fromTag = dv.getUint16(4);
      const from = dv.getUint32(6);
      const n = dv.getUint32(10);
      const sent = dv.getFloat64(14);
      if (!measuring) return;
      s.framesReceived++;
      record(fromTag === tag ? s.latLocal : s.latRemote, now() - sent);
      const key = fromTag * 1e7 + from;
      const last = c.seen.get(key);
      if (last !== undefined && n > last + 1) s.framesMissing += n - last - 1;
      if (last === undefined || n > last) c.seen.set(key, n);
    };

    const onText = (text) => {
      const [, r, , event, payload] = JSON.parse(text);
      switch (event) {
        case "phx_reply": {
          const kind = pending.get(r);
          pending.delete(r);
          if (kind === "join") {
            if (payload.status === "ok") {
              me = payload.response.you;
              joined();
            } else {
              s.joinErrors++;
              c.stop();
            }
          } else if (kind === "say" && measuring) {
            if (payload.status === "ok") s.sayOk++;
            else s.sayRefused++;
          }
          break;
        }
        case "presence_state":
          others = new Set(Object.keys(payload).filter((k) => k !== me));
          break;
        case "presence_diff":
          for (const k of Object.keys(payload.joins || {})) if (k !== me) others.add(k);
          for (const k of Object.keys(payload.leaves || {})) others.delete(k);
          break;
        case "said": {
          if (!measuring) break;
          s.sayHeard++;
          const m = /^probe (\d+) ([\d.]+)/.exec(payload.text || "");
          if (m && Number(m[1]) === tag) record(s.latSay, now() - Number(m[2]));
          break;
        }
        case "react":
          if (measuring) s.reactHeard++;
          break;
        case "wave":
          if (measuring) s.waveHeard++;
          break;
        case "phx_error":
        case "phx_close":
          if (!stopping) s.channelErrors++;
          break;
      }
    };

    // Each member's chatter, at random times about this often.
    const every = (secs, fn) => {
      if (!secs) return;
      const next = () => timers.push(setTimeout(() => (fn(), next()), secs * 1000 * (0.5 + Math.random())));
      next();
    };

    const joined = () => {
      s.connected++;
      s.connectMs.push(now() - startedAt);
      parentPort.postMessage({ type: "joined", n: s.connected });
      timers.push(setInterval(sendFrame, o.gap));
      timers.push(setInterval(() => ws.send(JSON.stringify([null, String(++ref), "phoenix", "heartbeat", {}])), 30_000));
      every(o.sayEvery, () => {
        if (measuring) s.saySent++;
        sendJson("say", { text: `probe ${tag} ${now()} hello from ${name}` }, "say");
      });
      every(o.reactEvery, () => {
        if (measuring) s.reactSent++;
        sendJson("react", { emoji: "🎉" });
      });
      every(o.waveEvery, () => {
        const to = [...others][Math.floor(Math.random() * others.size)];
        if (!to) return;
        if (measuring) s.waveSent++;
        sendJson("wave", { to });
      });
    };

    let startedAt = 0;
    const start = async () => {
      startedAt = now();
      let tok;
      try {
        tok = await token(room, name, `${o.id}-${i}`.replace(/[^A-Za-z0-9_.:@-]/g, "_"));
      } catch (e) {
        s.tokenErrors++;
        return;
      }
      ws = new WebSocket(wsBase + encodeURIComponent(tok));
      ws.binaryType = "arraybuffer";
      ws.onopen = () => sendJson("phx_join", {}, "join");
      ws.onmessage = (e) => (typeof e.data === "string" ? onText(e.data) : onFrame(e.data));
      ws.onerror = () => {
        if (!stopping) s.socketErrors++;
      };
      ws.onclose = () => {
        for (const t of timers) clearTimeout(t), clearInterval(t);
        if (!stopping) s.closed++;
      };
    };

    live.push(c);
    // Spread over the ramp by the client's place among all of them.
    const at = t0 + (rampMs * i) / clients;
    setTimeout(start, Math.max(0, at - Date.now()));
  }

  for (const i of idx) client(i);
  await new Promise(() => {});
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function log(m) {
  console.error(`[probe ${new Date().toISOString().slice(11, 19)}] ${m}`);
}
