#!/usr/bin/env node
// Measurement only (#443): a byte-stream TCP proxy that simulates a slow link.
// Each direction is a serial link: a chunk waits for the bytes before it
// (size / rate), then travels `delay` ms. The source is paused while more than
// `buf` bytes are queued, as a full TCP send buffer would.
// Usage: link-proxy.mjs LISTEN_PORT TARGET_PORT [DELAY_MS_ONE_WAY=0] [MBIT=0 (unlimited)]
// Env: LINK_STATS=file writes {up,down} byte totals at exit.
import net from "node:net";
import { writeFileSync } from "node:fs";

const [listen, target, delayArg = "0", mbitArg = "0"] = process.argv.slice(2);
const delay = Number(delayArg);
const bps = (Number(mbitArg) * 1e6) / 8; // bytes per second; 0 = unlimited
const BUF = 256 * 1024;
const totals = { up: 0, down: 0 };

// The daemon refuses a Host it doesn't know: say its own port in what the browser sends up.
const hostFrom = Buffer.from(`127.0.0.1:${listen}`), hostTo = Buffer.from(`127.0.0.1:${target}`);
const fix = (c) => (hostFrom.length === hostTo.length && c.includes(hostFrom) ? Buffer.from(c.toString("latin1").replaceAll(hostFrom.toString(), hostTo.toString()), "latin1") : c);

function pipe(from, to, key) {
  if (!delay && !bps) {
    from.on("data", (c) => { if (key === "up") c = fix(c); totals[key] += c.length; if (!to.write(c)) { from.pause(); to.once("drain", () => from.resume()); } });
    from.on("end", () => to.end());
    return;
  }
  let free = 0; // when the link has sent everything queued
  let queued = 0;
  from.on("data", (c) => {
    if (key === "up") c = fix(c);
    totals[key] += c.length;
    const now = performance.now();
    const start = Math.max(now, free);
    free = bps ? start + (c.length / bps) * 1000 : now;
    const at = free + delay;
    queued += c.length;
    if (queued > BUF) from.pause();
    setTimeout(() => {
      queued -= c.length;
      if (!to.destroyed) to.write(c);
      if (queued <= BUF / 2) from.resume();
    }, Math.max(0, at - performance.now()));
  });
  from.on("end", () => setTimeout(() => to.end(), Math.max(0, free + delay - performance.now())));
}

const server = net.createServer((a) => {
  const b = net.connect(Number(target), "127.0.0.1");
  a.setNoDelay(true); b.setNoDelay(true);
  pipe(a, b, "up");
  pipe(b, a, "down");
  const kill = () => { a.destroy(); b.destroy(); };
  a.on("error", kill); b.on("error", kill); a.on("close", kill); b.on("close", kill);
});
server.listen(Number(listen), "127.0.0.1");
const stop = () => {
  if (process.env.LINK_STATS) writeFileSync(process.env.LINK_STATS, JSON.stringify(totals));
  process.exit(0);
};
process.on("SIGTERM", stop); process.on("SIGINT", stop);
