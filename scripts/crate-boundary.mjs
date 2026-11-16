#!/usr/bin/env node
// The line between the cloud, the daemon and the native clients (Arugula
// #809): each is a Cargo workspace of its own, and what crosses between them
// is the crates in SHARED, nothing else. Reads `cargo metadata --no-deps`
// (no network, no build) of each workspace and fails, naming the crate and
// the edge, when:
//
// - a crate's path dependency leaves this repository;
// - a crate depends on a crate of another workspace here (native/core or
//   the desktop app on the daemon's, or the daemon on theirs) that isn't
//   shared;
// - a shared crate depends, to build (not for its tests), on one that isn't
//   shared: the cloud and the clients would build it too;
// - a path dependency points at a directory that's no workspace's member,
//   or a Cargo workspace here isn't in WORKSPACES, or a shared crate isn't
//   in the root workspace.
//
// With --cloud DIR it checks the cloud's repository instead (arugula-salad/
// cloud, which has this repo at open/ and runs this script from there): each
// of its crates may depend on its own and on open/'s shared crates only. It
// checks this repo's shared crates as above too, at the cloud's pin.
//
//   node scripts/crate-boundary.mjs [--cloud DIR]
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

// What the cloud and the clients build from this repo: the wire between
// daemons and control, the end-to-end crypto, Claude's list prices, the
// GitHub reader, and the protocol crates under them. The cloud depends on
// control-wire, core, e2e, forge-github and prices; native/core on e2e,
// control-wire and proto; the desktop app on proto. proto is in the cloud's
// build through forge-github. A crate added here is one the daemon can no
// longer change alone: say why in the pull request.
export const SHARED = [
  "arugula-control-wire",
  "arugula-core",
  "arugula-e2e",
  "arugula-forge-github",
  "arugula-prices",
  "arugula-proto",
];

// This repo's Cargo workspaces, by their root's directory.
export const WORKSPACES = {
  ".": "daemon",
  "native/core": "native",
  "crates/desktop": "desktop",
};

const open = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = process.env.CARGO || "cargo";
const problems = [];

// Cargo refuses some crossings itself (a crate in one workspace on the
// root of another is "multiple workspace roots"): those fail here as its
// error.
const metadata = (dir) => {
  const manifest = join(dir, "Cargo.toml");
  try {
    return JSON.parse(
      execFileSync(cargo, ["metadata", "--no-deps", "--format-version", "1", "--offline", "--manifest-path", manifest], {
        encoding: "utf8",
        maxBuffer: 256 << 20,
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
  } catch (e) {
    console.error(`crate-boundary: cargo metadata on ${manifest} failed:\n${String(e.stderr ?? e.message).trim()}`);
    process.exit(1);
  }
};

const inside = (dir, root) => dir === root || dir.startsWith(root + sep);
const kind = (dep) => (dep.kind === "dev" ? "dev-dependency" : dep.kind === "build" ? "build-dependency" : "dependency");

// The directory of every Cargo.toml with a [workspace] table in the repo:
// of git's files, tracked or not but never ignored (spikes' work
// directories, build output); out of git (the test's fake repo), of every
// one under root but in build output, packages and vendored code.
const workspaceRoots = (root) => {
  let manifests = [];
  try {
    manifests = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "Cargo.toml", "*/Cargo.toml"], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split("\0")
      .filter(Boolean);
  } catch {
    const walk = (dir) => {
      for (const e of readdirSync(join(root, dir), { withFileTypes: true })) {
        if (!e.isDirectory() || e.name.startsWith(".") || ["target", "node_modules", "vendor", "dist"].includes(e.name)) continue;
        const sub = join(dir, e.name);
        manifests.push(join(sub, "Cargo.toml"));
        walk(sub);
      }
    };
    walk("");
  }
  return manifests
    .filter((m) => existsSync(join(root, m)) && /^\[workspace\]/m.test(readFileSync(join(root, m), "utf8")))
    .map((m) => dirname(m));
};

// The members of this repo's workspaces: directory -> {name, ws}.
const openMembers = () => {
  const members = new Map();
  const packages = [];
  for (const [dir, ws] of Object.entries(WORKSPACES)) {
    for (const p of metadata(join(open, dir)).packages) {
      const d = dirname(p.manifest_path);
      members.set(d, { name: p.name, ws });
      packages.push({ ...p, ws });
    }
  }
  return { members, packages };
};

const checkOpen = () => {
  for (const dir of workspaceRoots(open)) {
    if (!(dir in WORKSPACES)) problems.push(`${dir}/Cargo.toml is a Cargo workspace that scripts/crate-boundary.mjs doesn't know: add it to WORKSPACES`);
  }
  const { members, packages } = openMembers();
  for (const name of SHARED) {
    const p = packages.find((p) => p.name === name);
    if (!p) problems.push(`${name} is in SHARED but no workspace here has it`);
    else if (p.ws !== "daemon") problems.push(`${name} is in SHARED but it's in the ${p.ws} workspace, not the root one`);
  }
  for (const p of packages) {
    for (const dep of p.dependencies) {
      if (!dep.path) continue;
      const edge = `${p.name} (${p.ws}) -> ${dep.name}, a ${kind(dep)} at ${relative(open, dep.path) || "."}`;
      if (!inside(dep.path, open)) {
        problems.push(`${edge}: it's outside this repository`);
        continue;
      }
      const to = members.get(dep.path);
      if (!to) {
        problems.push(`${edge}: that's no workspace's member here`);
        continue;
      }
      const shared = SHARED.includes(to.name);
      if (to.ws !== p.ws && !shared) {
        problems.push(`${edge}: ${to.name} is the ${to.ws} workspace's, and not shared`);
      } else if (SHARED.includes(p.name) && !shared && dep.kind !== "dev") {
        problems.push(`${edge}: ${p.name} is shared, so the cloud and the clients would build ${to.name}, which isn't`);
      }
    }
  }
  return members;
};

const checkCloud = (cloud) => {
  const members = checkOpen();
  const { packages } = metadata(cloud);
  const own = new Set(packages.map((p) => dirname(p.manifest_path)));
  for (const p of packages) {
    for (const dep of p.dependencies) {
      if (!dep.path || own.has(dep.path)) continue;
      const edge = `${p.name} (cloud) -> ${dep.name}, a ${kind(dep)} at ${relative(cloud, dep.path)}`;
      const to = inside(dep.path, open) ? members.get(dep.path) : undefined;
      if (!inside(dep.path, open)) problems.push(`${edge}: it's neither the cloud's crate nor the open repo's`);
      else if (!to) problems.push(`${edge}: that's no workspace's member in the open repo`);
      else if (!SHARED.includes(to.name)) problems.push(`${edge}: ${to.name} is the open repo's ${to.ws} workspace's, and not shared`);
    }
  }
};

const at = process.argv.indexOf("--cloud");
if (at > 0) {
  const cloud = resolve(process.argv[at + 1] ?? ".");
  if (!inside(open, cloud)) {
    console.error(`crate-boundary: run the open repo's copy at ${cloud}/open/scripts`);
    process.exit(2);
  }
  checkCloud(cloud);
} else {
  checkOpen();
}

if (problems.length) {
  for (const p of problems) console.error(`crate-boundary: ${p}`);
  console.error(`crate-boundary: the workspaces share only ${SHARED.join(", ")} (scripts/crate-boundary.mjs, Arugula #809)`);
  process.exit(1);
}
console.log(`crate-boundary: ${at > 0 ? "the cloud and " : ""}${Object.values(WORKSPACES).join(", ")} share only ${SHARED.join(", ")}`);
