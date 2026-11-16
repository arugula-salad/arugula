#!/usr/bin/env node
// The released daemons `just older-daemons` runs the page at HEAD against
// (Arugula #808): by default the oldest and the newest daemon release of the
// supported window, any released in the last 30 days (#807), or the
// versions named (`0.66.10`, `v0.70.23`). Each is downloaded from
// arugula-salad/arugula's GitHub releases for this machine, checked against
// the release's SHA256SUMS, and kept in ~/.cache/arugula/older-daemons/.
// Prints a JSON list of { version, bin } for web/playwright.older.config.ts.
//
//   node scripts/older-daemons.mjs [--days N] [VERSION...]
//
// GH_TOKEN or GITHUB_TOKEN, when set, lifts GitHub's limit on anonymous
// calls (60 an hour; a run makes a few).

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const REPO = "arugula-salad/arugula";
const args = process.argv.slice(2);
let days = 30;
const named = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--days") days = Number(args[++i]);
  else named.push(args[i].replace(/^v/, ""));
}
if (!(days > 0)) fail("--days takes a number of days");

const TARGETS = {
  "linux x64": "x86_64-unknown-linux-musl",
  "linux arm64": "aarch64-unknown-linux-musl",
  "darwin arm64": "aarch64-apple-darwin",
  "darwin x64": "x86_64-apple-darwin",
};
const target = TARGETS[`${process.platform} ${process.arch}`];
if (!target) fail(`no daemon release for ${process.platform} ${process.arch}`);

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
const headers = { accept: "application/vnd.github+json", "user-agent": "arugula-older-daemons", ...(token ? { authorization: `Bearer ${token}` } : {}) };

function fail(why) {
  console.error(`older-daemons: ${why}`);
  process.exit(1);
}

async function get(url) {
  const res = await fetch(url, { headers: url.startsWith("https://api.github.com/") ? headers : { "user-agent": headers["user-agent"] } });
  if (!res.ok) fail(`${url}: HTTP ${res.status}${res.status === 403 ? " (set GH_TOKEN for GitHub's higher limit)" : ""}`);
  return res;
}

/** Every published daemon release (tags `vX.Y.Z`; the app's are `app-v`), newest first. */
async function releases() {
  const all = [];
  for (let page = 1; ; page++) {
    const batch = await (await get(`https://api.github.com/repos/${REPO}/releases?per_page=100&page=${page}`)).json();
    all.push(...batch.filter((r) => !r.draft && !r.prerelease && /^v\d+\.\d+\.\d+$/.test(r.tag_name)));
    // Releases come newest first: once a page reaches past the window, stop.
    const last = batch.at(-1);
    if (batch.length < 100 || (!named.length && last && Date.parse(last.published_at) < Date.now() - days * 86_400_000)) break;
  }
  return all;
}

const list = await releases();
let picked;
if (named.length) {
  picked = named.map((v) => list.find((r) => r.tag_name === `v${v}`) ?? fail(`no release v${v}`));
} else {
  const since = Date.now() - days * 86_400_000;
  const window = list.filter((r) => Date.parse(r.published_at) >= since);
  if (!window.length) fail(`no daemon release in the last ${days} days`);
  // The newest and the oldest: one, if the window has only one.
  picked = [...new Set([window[0], window.at(-1)])].reverse();
}

const cache = join(process.env.XDG_CACHE_HOME || join(homedir(), ".cache"), "arugula", "older-daemons");
const out = [];
for (const r of picked) {
  const version = r.tag_name.slice(1);
  const dir = join(cache, `${version}-${target}`);
  const bin = join(dir, "arugulad");
  if (!existsSync(bin)) {
    const name = `arugula-${version}-${target}.tar.gz`;
    const asset = r.assets.find((a) => a.name === name) ?? fail(`v${version} has no ${name}`);
    const sums = r.assets.find((a) => a.name === "SHA256SUMS") ?? fail(`v${version} has no SHA256SUMS`);
    const want = (await (await get(sums.browser_download_url)).text())
      .split("\n")
      .map((l) => l.trim().split(/\s+\*?/))
      .find(([, f]) => f === name)?.[0];
    if (!want) fail(`v${version}'s SHA256SUMS doesn't list ${name}`);
    console.error(`older-daemons: downloading ${name}`);
    const data = Buffer.from(await (await get(asset.browser_download_url)).arrayBuffer());
    const got = createHash("sha256").update(data).digest("hex");
    if (got !== want) fail(`${name}: sha256 ${got}, SHA256SUMS says ${want}`);
    const tmp = `${dir}.tmp`;
    rmSync(tmp, { recursive: true, force: true });
    mkdirSync(tmp, { recursive: true });
    writeFileSync(join(tmp, name), data);
    execFileSync("tar", ["xzf", name, "--strip-components=1"], { cwd: tmp });
    rmSync(join(tmp, name));
    rmSync(dir, { recursive: true, force: true });
    renameSync(tmp, dir);
  }
  if (!existsSync(bin)) fail(`${bin} isn't there`);
  out.push({ version, published: r.published_at, bin });
}
console.log(JSON.stringify(out));
