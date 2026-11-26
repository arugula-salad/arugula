#!/usr/bin/env node
// Quarantines a test that ejects merge groups (flaky-quarantine.yml, Arugula
// #1423). When the same test fails the merge queue's verdict in two groups
// within 24 hours, for two different pull requests that both merged later
// (so the test passed for them on another try), it files a native `flaky`
// issue through the project's CI alerts (a `ci.flaky` event, signed as
// chant's are) and opens a pull request that adds the test to
// chant.workspace.json's ci.green.quarantine (docs/testing.md,
// "Quarantine") naming that issue. A person merges it: nothing here queues it.
//
//   scripts/flaky-quarantine.mjs                 look at the last 48 hours, act
//   scripts/flaky-quarantine.mjs --dry-run       say what it would do
//   scripts/flaky-quarantine.mjs --replay FROM..TO
//                                             each hour from FROM to TO (ISO),
//                                             what it would have quarantined
//
// A test is a Playwright spec's (the `N failed` block of its report: the
// file from web/e2e and the test's own title, phase e2e) or a nextest test's
// (the FAIL lines after its Summary: the name without the binary id, phase
// test), read from the failed jobs' logs of the queue's failed runs. A test
// stays out of it when it's quarantined already, its branch
// (quarantine-<hash>) exists (its pull request is open, or a person closed
// it), or it's in a serial spec, which docs/testing.md says not to
// quarantine. At most three a run.
//
// Reads GitHub with gh (GH_TOKEN, the job's token). Opens the pull request
// with ARUGULA_QUARANTINE_TOKEN when set (CI_GREEN_TOKEN: a pull request
// opened with the job's own token runs no workflows). Files the issue at
// ARUGULA_CI_NOTIFY_URL, signed with ARUGULA_CI_NOTIFY_KEY. The repository
// variable ARUGULA_QUARANTINE=off turns it off. ARUGULA_QUARANTINE_NOW (ISO)
// is for tests.
import { execFileSync } from "node:child_process";
import { createHash, createHmac } from "node:crypto";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const HOUR = 3600e3;
const DAY = 24 * HOUR;
// How far back a run counts: a pair within a day, the later one's pull
// request merging up to a day after.
const LOOK_BACK = 2 * DAY;
const PER_RUN = 3;
// How long an entry stays out: chant's maxDays.
const MAX_DAYS = 14;
const WORKSPACE = "chant.workspace.json";

// ------------------------------------------------------------ what failed

/** The tests a job's log says failed (not flaky), each `{ phase, file?, test }`. */
export function failures(log) {
  const lines = log.split("\n").map((l) => l.replace(/^﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ?/, "").replace(/\r$/, ""));
  const out = new Map();
  const add = (f) => out.set(name(f), f);
  for (let i = 0; i < lines.length; i++) {
    // Playwright's list: `  1 failed`, then a line a test, until the next count.
    if (/^\s+\d+ failed$/.test(lines[i])) {
      for (let j = i + 1; j < lines.length; j++) {
        const m = lines[j].match(/^\s+\[[^\]]+\] › (?:e2e\/)?(\S+?\.spec\.ts):\d+:\d+ › (.+?)\s*$/);
        if (!m) break;
        const title = m[2].split(" › ");
        // The report pads a test's line out with a rule.
        add({ phase: "e2e", file: m[1], test: title[title.length - 1].replace(/[\s─]+$/, "") });
      }
    }
    // nextest's summary: the tests that failed every try, by binary and name.
    if (/^\s+Summary \[/.test(lines[i])) {
      for (let j = i + 1; j < lines.length; j++) {
        const m = lines[j].match(
          /^\s+(?:TRY \d+ )?(?:FAIL|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|SIGTERM|ABORT|LEAK-FAIL)\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)\s*$/,
        );
        if (m) add({ phase: "test", test: m[2] });
        else if (!/^\s+(?:TRY \d+ )?(?:FLAKY|SLOW|LEAK|PASS|SKIP)\b/.test(lines[j])) break;
      }
    }
  }
  return [...out.values()];
}

/** A test as chant names an entry: `file › test`, or the test. */
export const name = (f) => (f.file !== undefined ? `${f.file} › ${f.test}` : f.test);

// ------------------------------------------------------------ deciding

/**
 * The tests to quarantine at `now` from `groups`, each failed group
 * `{ run, url, at, pr, merged, sha, tests }` (ms; `merged` null when its pull
 * request hasn't): two failures within a day, for different pull requests
 * that each merged after its group failed and by `now`, in groups
 * `apart(a, b)` says aren't one stacked on the other. A group behind a
 * broken one in the queue holds its change too, so it fails with it, and
 * its own pull request merges once the broken one is out: that's one
 * break, not two. Each answer is the test with its failures `hits` (no two
 * stacked, oldest first) and when it could first tell (`decided`).
 */
export function decide(groups, now, apart = () => true) {
  const by = new Map();
  for (const g of groups) {
    if (g.at > now || g.at < now - LOOK_BACK) continue;
    if (!(g.merged && g.merged > g.at && g.merged <= now)) continue;
    for (const t of g.tests) {
      const k = name(t);
      if (!by.has(k)) by.set(k, { ...t, hits: [] });
      by.get(k).hits.push(g);
    }
  }
  const out = [];
  for (const c of by.values()) {
    const hits = c.hits.sort((a, b) => a.at - b.at);
    let pair = null;
    for (let i = 0; i < hits.length && !pair; i++)
      for (let j = i + 1; j < hits.length && !pair; j++)
        if (hits[j].pr !== hits[i].pr && hits[j].at - hits[i].at <= DAY && apart(hits[i], hits[j])) pair = [hits[i], hits[j]];
    if (!pair) continue;
    const within = [];
    for (const h of hits)
      if (h.at >= pair[0].at && h.at - pair[0].at <= DAY && within.every((w) => w.pr !== h.pr && apart(w, h))) within.push(h);
    out.push({ ...c, hits: within, decided: Math.max(pair[1].at, pair[0].merged, pair[1].merged) });
  }
  return out.sort((a, b) => a.decided - b.decided);
}

// ------------------------------------------------------------ the entry

/** Where the value at `path` (keys) is in JSON `text`: [start, end). */
export function span(text, path) {
  let i = 0;
  const ws = () => {
    while (/\s/.test(text[i])) i++;
  };
  const str = () => {
    const s = i++;
    while (text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
    i++;
    return JSON.parse(text.slice(s, i));
  };
  const skip = () => {
    ws();
    if (text[i] === '"') return void str();
    if (text[i] === "{" || text[i] === "[") {
      const close = text[i] === "{" ? "}" : "]";
      i++;
      ws();
      while (text[i] !== close) {
        if (close === "}") {
          str();
          ws();
          i++; // :
        }
        skip();
        ws();
        if (text[i] === ",") i++;
        ws();
      }
      i++;
      return;
    }
    while (i < text.length && !/[\s,}\]]/.test(text[i])) i++;
  };
  const find = (keys) => {
    ws();
    const start = i;
    if (!keys.length) {
      skip();
      return [start, i];
    }
    if (text[i] !== "{") return null;
    i++;
    ws();
    while (text[i] !== "}") {
      const k = str();
      ws();
      i++; // :
      if (k === keys[0]) return find(keys.slice(1));
      skip();
      ws();
      if (text[i] === ",") i++;
      ws();
    }
    return null;
  };
  return find(path);
}

const oneLine = (e) => `{ ${Object.entries(e).map(([k, v]) => `${JSON.stringify(k)}: ${JSON.stringify(v)}`).join(", ")} }`;

/** ci.green.quarantine as chant.workspace.json writes it: an entry a line. */
function render(q, ind) {
  const [i1, i2, i3] = [ind + "  ", ind + "    ", ind + "      "];
  const parts = Object.entries(q).map(([k, v]) => {
    if (k !== "phases") return `${i1}${JSON.stringify(k)}: ${JSON.stringify(v)}`;
    const ps = Object.entries(v).map(([p, es]) => `${i2}${JSON.stringify(p)}: [\n${es.map((e) => i3 + oneLine(e)).join(",\n")}\n${i2}]`);
    return `${i1}"phases": {\n${ps.join(",\n")}\n${i1}}`;
  });
  return `{\n${parts.join(",\n")}\n${ind}}`;
}

/** `text` (chant.workspace.json) with `entry` quarantined under `phase`. */
export function addEntry(text, phase, entry) {
  const doc = JSON.parse(text);
  const green = doc.ci.green;
  const q = structuredClone(green.quarantine ?? { maxDays: MAX_DAYS, phases: {} });
  (q.phases[phase] ??= []).push(entry);
  let out;
  const at = span(text, ["ci", "green", "quarantine"]);
  if (at) {
    const ind = text.slice(text.lastIndexOf("\n", at[0]) + 1).match(/^\s*/)[0];
    out = text.slice(0, at[0]) + render(q, ind) + text.slice(at[1]);
  } else {
    const [, end] = span(text, ["ci", "green"]);
    const close = end - 1;
    const ind = text.slice(text.lastIndexOf("\n", close) + 1).match(/^\s*/)[0] + "  ";
    const last = text.slice(0, close).trimEnd().length;
    out = `${text.slice(0, last)},\n${ind}"quarantine": ${render(q, ind)}${text.slice(last)}`;
  }
  const want = structuredClone(doc);
  want.ci.green.quarantine = q;
  if (JSON.stringify(JSON.parse(out)) !== JSON.stringify(want)) throw new Error("the quarantine didn't go in as meant");
  return out;
}

/** The tests quarantined in chant.workspace.json's `text`, by name. */
export function quarantined(text) {
  const phases = JSON.parse(text).ci?.green?.quarantine?.phases ?? {};
  return new Set(Object.values(phases).flat().map(name));
}

// ------------------------------------------------------------ GitHub

const repo = process.env.GITHUB_REPOSITORY || "arugula-salad/arugula";
const say = (s) => console.log(`flaky-quarantine: ${s}`);
const summary = (s) => process.env.GITHUB_STEP_SUMMARY && appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${s}\n`);

function gh(args, env = {}) {
  return execFileSync("gh", args, { encoding: "utf8", maxBuffer: 256 << 20, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
}
const api = (path) => JSON.parse(gh(["api", path]));
// A job's log. Newer gh refuses one with terminal escapes (the specs' colours)
// without --allow-escape-sequences, which older gh doesn't know.
function jobLog(id) {
  const path = `repos/${repo}/actions/jobs/${id}/logs`;
  try {
    return gh(["api", "--allow-escape-sequences", path]);
  } catch {
    return gh(["api", path]);
  }
}
const iso = (ms) => new Date(ms).toISOString().replace(/\.\d+Z$/, "Z");

/** The queue's failed groups created in [from, to], with their failed tests. */
function groups(from, to) {
  const runs = [];
  for (let page = 1; ; page++) {
    const r = api(`repos/${repo}/actions/runs?event=merge_group&status=failure&created=${iso(from)}..${iso(to)}&per_page=100&page=${page}`);
    runs.push(...r.workflow_runs);
    if (r.workflow_runs.length < 100) break;
  }
  const merged = new Map();
  const mergedAt = (pr) => {
    if (!merged.has(pr)) {
      const p = api(`repos/${repo}/pulls/${pr}`);
      merged.set(pr, p.merged_at ? Date.parse(p.merged_at) : null);
    }
    return merged.get(pr);
  };
  const out = [];
  for (const r of runs) {
    // The verdict's run says only that the others failed.
    if (r.name === "chant-merge-queue") continue;
    const pr = Number(r.head_branch?.match(/^gh-readonly-queue\/[^/]+\/pr-(\d+)-/)?.[1]);
    if (!pr) continue;
    const jobs = api(`repos/${repo}/actions/runs/${r.id}/jobs?filter=latest&per_page=100`).jobs.filter((j) => j.conclusion === "failure");
    const tests = new Map();
    for (const j of jobs) {
      let log;
      try {
        log = jobLog(j.id);
      } catch {
        say(`no log for job ${j.id} (${j.name}) of run ${r.id}`);
        continue;
      }
      for (const t of failures(log)) tests.set(name(t), t);
    }
    if (tests.size) out.push({ run: r.id, url: r.html_url, at: Date.parse(r.created_at), sha: r.head_sha, pr, merged: mergedAt(pr), tests: [...tests.values()] });
  }
  return out;
}

/** Whether neither group's commit is below the other's (GitHub's compare). */
const compared = new Map();
function apart(a, b) {
  const k = `${a.sha}...${b.sha}`;
  if (!compared.has(k)) {
    let status;
    try {
      status = api(`repos/${repo}/compare/${a.sha}...${b.sha}`).status;
    } catch {
      // A commit GitHub no longer has: count them apart.
      status = "diverged";
    }
    compared.set(k, status === "diverged");
  }
  return compared.get(k);
}

// ------------------------------------------------------------ acting

const hashOf = (k) => createHash("sha256").update(k).digest("hex").slice(0, 12);
const day = (ms) => iso(ms).slice(0, 10);

async function fileIssue(c, now) {
  const url = process.env.ARUGULA_CI_NOTIFY_URL;
  const key = process.env.ARUGULA_CI_NOTIFY_KEY;
  if (!url || !key) throw new Error("no ARUGULA_CI_NOTIFY_URL or ARUGULA_CI_NOTIFY_KEY to file the issue with");
  const last = c.hits[c.hits.length - 1];
  const body = JSON.stringify({
    schema: "chant.notify/v1",
    id: `flaky-${hashOf(name(c))}-${now}`,
    event: "ci.flaky",
    at: new Date(now).toISOString(),
    source: "flaky-quarantine",
    project: repo,
    summary: `${name(c)} failed ${c.hits.length} merge groups within a day`,
    targets: [],
    ci: { branch: "main", commit: last.sha, phase: c.phase, test: name(c), groups: c.hits.map((h) => ({ run: h.url, pr: h.pr })) },
    run: { forge: "github", repository: repo, commit: last.sha },
  });
  const ts = String(Math.floor(Date.now() / 1000));
  const sig = createHmac("sha256", key).update(`${ts}.${body}`).digest("hex");
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", "x-chant-timestamp": ts, "x-chant-signature": `sha256=${sig}`, "x-chant-event": "ci.flaky" },
    body,
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok || !v.number) throw new Error(`CI alerts answered ${r.status}: ${JSON.stringify(v)}`);
  return v.number;
}

function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

async function quarantine(c, now) {
  const k = name(c);
  const branch = `quarantine-${hashOf(k)}`;
  try {
    git("ls-remote", "--exit-code", "--heads", "origin", branch);
    return say(`${k}: ${branch} exists already (its pull request is open, or a person closed it)`);
  } catch {}
  if (c.file && /mode:\s*["']serial["']/.test(readFileSync(`web/e2e/${c.file}`, "utf8"))) {
    console.log(`::warning::${k} ejected ${c.hits.length} merge groups, but its spec is serial, which a quarantine would break: fix it by hand`);
    return summary(`- ${k}: in a serial spec, not quarantined`);
  }
  const issue = await fileIssue(c, now);
  const until = day(now + MAX_DAYS * DAY);
  const runs = c.hits.map((h) => `run ${h.run} for #${h.pr}`).join(", ");
  const why = `Failed the merge queue's verdict in ${c.hits.length} groups within a day (${runs}); each pull request merged later.`;
  const entry = c.file ? { file: c.file, test: c.test, issue: String(issue), until, why } : { test: c.test, issue: String(issue), until, why };
  const base = git("rev-parse", "HEAD");
  git("switch", "-q", "-c", branch);
  try {
    writeFileSync(WORKSPACE, addEntry(readFileSync(WORKSPACE, "utf8"), c.phase, entry));
    const title = `CI: quarantine ${k.length > 90 ? `${k.slice(0, 89)}…` : k}, which ejected ${c.hits.length} merge groups`;
    git("commit", "-q", "-am", `${title}\n\n${why}\nThe fix is tracked in native issue ${issue}.`);
    git("push", "-q", "origin", branch);
    const list = c.hits.map((h) => `- [run ${h.run}](${h.url}) for #${h.pr}, merged later`).join("\n");
    const body = `part of Arugula #${issue}

\`${k}\` failed the merge queue's verdict in these groups within a day, and each group's pull request merged later, so the test is flaky, not broken:

${list}

Merging this leaves it out of CI's runs until ${until} (\`ci.green.quarantine\`, docs/testing.md "Quarantine"). quarantine-nightly.yml still runs it every night. Fix the test, then delete the entry. Close this pull request to keep it in: this branch then keeps \`scripts/flaky-quarantine.mjs\` from opening another for it.`;
    const tok = process.env.ARUGULA_QUARANTINE_TOKEN;
    let pr;
    try {
      pr = gh(["pr", "create", "-R", repo, "--head", branch, "--title", title, "--body", body], tok ? { GH_TOKEN: tok } : {}).trim();
    } catch (e) {
      // Not left behind: a run that finds the branch stays out, so nothing would try again.
      git("push", "-q", "origin", "--delete", branch);
      throw e;
    }
    say(`${k}: opened ${pr}, with issue ${issue}`);
    summary(`- ${k}: ${pr}, native issue ${issue}`);
  } finally {
    git("switch", "-q", "--detach", base);
  }
}

// ------------------------------------------------------------ main

async function main() {
  const argv = process.argv.slice(2);
  const replay = argv[argv.indexOf("--replay") + 1];
  if (argv.includes("--replay")) {
    const [from, to] = (replay ?? "").split("..").map(Date.parse);
    if (!from || !to) throw new Error("--replay FROM..TO, two ISO times");
    const all = groups(from - LOOK_BACK, to);
    const seen = new Set();
    for (let t = from; t <= to; t += HOUR)
      for (const c of decide(all, t, apart)) {
        if (seen.has(name(c))) continue;
        seen.add(name(c));
        console.log(`${iso(t)}  would quarantine ${name(c)} (${c.phase}), decided at ${iso(c.decided)}:`);
        for (const h of c.hits) console.log(`    run ${h.run} at ${iso(h.at)} for #${h.pr}, merged ${iso(h.merged)}`);
      }
    if (!seen.size) console.log("nothing would have been quarantined");
    return;
  }
  if (process.env.ARUGULA_QUARANTINE === "off") return say("ARUGULA_QUARANTINE is off");
  const now = process.env.ARUGULA_QUARANTINE_NOW ? Date.parse(process.env.ARUGULA_QUARANTINE_NOW) : Date.now();
  const out = quarantined(readFileSync(WORKSPACE, "utf8"));
  const todo = decide(groups(now - LOOK_BACK, now), now, apart).filter((c) => !out.has(name(c)));
  if (!todo.length) return say("no test ejected two merge groups");
  let failed = false;
  for (const c of todo.slice(0, PER_RUN)) {
    if (argv.includes("--dry-run")) {
      say(`would quarantine ${name(c)} (${c.phase}): ${c.hits.map((h) => `run ${h.run} for #${h.pr}`).join(", ")}`);
      continue;
    }
    try {
      await quarantine(c, now);
    } catch (e) {
      failed = true;
      console.log(`::error::couldn't quarantine ${name(c)}: ${e.message}`);
    }
  }
  if (todo.length > PER_RUN) say(`${todo.length - PER_RUN} more wait for the next run`);
  if (failed) process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((e) => {
    console.error(`flaky-quarantine: ${e.message}`);
    process.exit(1);
  });
}
