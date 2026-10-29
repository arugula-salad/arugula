// S43 load probe: how big and how often an outline frame can go through
// control before it shows. jake posts frames of SIZE bytes at FPS; sam
// and riley each hold STREAMS streams; each frame carries its send time,
// so every stream says how late it came and how many arrived.
//
//   node --experimental-strip-types scripts/outline-load.ts STATE_DIR [STREAMS]
//
// STATE_DIR is a dev-control's (DEV_CONTROL_CHAT=1, DEV_CONTROL_LOGINS=
// sam,riley). Not a test: numbers for the S43 Doc.
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { Device } from "../fixtures/device.ts";

const [dir, streamsArg] = process.argv.slice(2);
const per = Number(streamsArg ?? 4);
const jake = await Device.load(join(dir, "device.json"));
await jake.session("jake");
const viewers: Device[] = [];
for (const l of ["sam", "riley"]) {
  const d = await Device.load(join(dir, `device-${l}.json`));
  await d.session(l);
  viewers.push(d);
}

const MAGIC = 0xee;
let got: { stream: number; seq: number; late: number }[] = [];
const ctl = new AbortController();
let open = 0;
for (const v of viewers) {
  for (let s = 0; s < per; s++) {
    const id = open++;
    void v
      .events(
        "/api/people/outlines",
        (event, data) => {
          if (event !== "frame") return;
          const m = JSON.parse(data) as { account: string; data: string };
          const b = Buffer.from(m.data, "base64");
          if (b[0] !== MAGIC) return;
          got.push({ stream: id, seq: b.readUInt32LE(1), late: Date.now() - b.readDoubleLE(5) });
        },
        ctl.signal,
      )
      .catch(() => {});
  }
}
await new Promise((r) => setTimeout(r, 1500));

const pid = (() => {
  try {
    return execFileSync("pgrep", ["-f", `arugula-control.*${dir}`]).toString().trim().split("\n")[0];
  } catch {
    return "";
  }
})();
const cpu = () => {
  if (!pid) return 0;
  const f = execFileSync("cat", [`/proc/${pid}/stat`]).toString().split(") ")[1].split(" ");
  return (Number(f[11]) + Number(f[12])) / 100; // utime + stime, in s
};

const pct = (xs: number[], p: number) => (xs.length ? xs.sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(xs.length * p))] : NaN);
console.log(`streams: ${open} (${per} each for sam and riley); control pid ${pid || "?"}`);
console.log("size_kb fps kbps_in kbps_out sent posted_ms_p50 got% late_p50 late_p95 late_max control_cpu%");
for (const size of [1024, 4096, 16384, 32000]) {
  for (const fps of [4, 10, 14]) {
    got = [];
    const sent: number[] = [];
    const posted: number[] = [];
    const secs = 5;
    const c0 = cpu();
    const t0 = Date.now();
    let seq = 0;
    while (Date.now() - t0 < secs * 1000) {
      const b = Buffer.alloc(size);
      for (let i = 13; i < size; i++) b[i] = (Math.random() * 256) | 0; // incompressible, as deflated frames are
      b[0] = MAGIC;
      b.writeUInt32LE(seq, 1);
      const at = Date.now();
      b.writeDoubleLE(at, 5);
      sent.push(seq++);
      await jake.postBytes("/api/people/outline", new Uint8Array(b)).catch(() => {});
      posted.push(Date.now() - at);
      const next = t0 + (seq * 1000) / fps;
      await new Promise((r) => setTimeout(r, Math.max(0, next - Date.now())));
    }
    await new Promise((r) => setTimeout(r, 1500));
    const cpuPct = ((cpu() - c0) / (secs + 1.5)) * 100;
    const lates = got.map((g) => g.late);
    const kbpsIn = (size * 8 * fps) / 1000;
    console.log(
      [
        (size / 1024).toFixed(1),
        fps,
        kbpsIn.toFixed(0),
        ((kbpsIn * 4) / 3 * open).toFixed(0),
        sent.length,
        pct(posted, 0.5),
        ((got.length / (sent.length * open)) * 100).toFixed(0),
        pct(lates, 0.5),
        pct(lates, 0.95),
        Math.max(...lates, 0),
        cpuPct.toFixed(0),
      ].join(" "),
    );
  }
}
ctl.abort();
process.exit(0);
