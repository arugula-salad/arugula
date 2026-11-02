#!/usr/bin/env node
// Measurement only (#443): medians per case from run-443.sh's PERF lines. Usage: summarize-443.mjs FILE...
import { readFileSync } from "node:fs";
const med = (a) => { const s = a.filter((x) => x != null).sort((x, y) => x - y); return s.length ? s[Math.floor((s.length - 1) / 2)] : null; };
const rows = new Map();
for (const f of process.argv.slice(2))
  for (const l of readFileSync(f, "utf8").split("\n")) {
    if (!l.startsWith("PERF ")) continue;
    const j = JSON.parse(l.slice(5));
    const [ver, cse] = j.label.split("/");
    const k = `${ver} ${cse} ${j.kind} busy=${j.busy}`;
    const r = rows.get(k) ?? { runs: 0, p50: [], first: [], msgs: [], bytes: [], to: 0, all: [] };
    r.runs++; r.p50.push(j.p50); r.first.push(j.first_p50); r.msgs.push(j.ws_msgs_first500ms_med); r.bytes.push(j.ws_bytes_med); r.to += j.timeouts; r.all.push(...j.all);
    rows.set(k, r);
  }
for (const [k, r] of [...rows].sort()) console.log(k.padEnd(40), `runs=${r.runs} all_shown_ms=${med(r.p50)} first_ms=${med(r.first)} msgs=${med(r.msgs)} bytes=${med(r.bytes)} timeouts=${r.to} min..max=${Math.min(...r.all)}..${Math.max(...r.all)}`);
