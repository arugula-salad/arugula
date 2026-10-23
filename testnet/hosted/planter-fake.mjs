// What planter's Ship reaches on home-cloud and GitHub, faked on the hosted
// box for testnet's `planter-deploy` claim (M233), run as `runner`:
//
// - GitHub's API and the app, on 127.0.0.1:9100: `GET /health` answers 200
//   once the app's image is pinned; `DELETE /installation/token` is logged
//   (`revoke <Authorization>`, to LOG).
// - shipped's build: every half second, a ship commit on the bare
//   `arugula-salad/shipped` (under ROOT) with no pin after it gets one, as
//   the repository's workflow pins it (`deploy(<app>): pin image ...`, the
//   image by digest in `<app>/k8s/kustomization.yaml`).
//
//   node planter-fake.mjs ROOT LOG
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [root, log] = process.argv.slice(2);
const bare = join(root, "arugula-salad/shipped.git");
const env = { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1", GIT_AUTHOR_NAME: "github-actions[bot]",
  GIT_AUTHOR_EMAIL: "github-actions[bot]@users.noreply.github.com", GIT_COMMITTER_NAME: "github-actions[bot]",
  GIT_COMMITTER_EMAIL: "github-actions[bot]@users.noreply.github.com" };
const git = (cwd, ...args) => execFileSync("git", args, { cwd, env, encoding: "utf8" }).trim();
let pinned = false;

createServer((req, res) => {
  if (req.method === "DELETE" && req.url === "/installation/token") {
    appendFileSync(log, `revoke ${req.headers.authorization ?? ""}\n`);
    return res.writeHead(204).end();
  }
  if (req.method === "GET" && req.url === "/health") return res.writeHead(pinned ? 200 : 503).end(pinned ? "ok" : "");
  res.writeHead(404).end();
}).listen(9100, "127.0.0.1");

setInterval(() => {
  try {
    const msg = git(bare, "log", "-1", "--format=%B", "main");
    const app = /^([a-z0-9-]+): ship to loam/.exec(msg)?.[1];
    if (!app) return;
    const w = mkdtempSync(join(tmpdir(), "build-"));
    git(w, "clone", "--quiet", bare, "c");
    const k = join(w, "c", app, "k8s/kustomization.yaml");
    writeFileSync(k, readFileSync(k, "utf8").replace(/newTag: unpinned/, `digest: sha256:${"7".repeat(64)}`));
    git(join(w, "c"), "commit", "--quiet", "-am", `deploy(${app}): pin image 777777777777`);
    git(join(w, "c"), "push", "--quiet", "origin", "HEAD:main");
    rmSync(w, { recursive: true, force: true });
    pinned = true;
    appendFileSync(log, `pinned ${app}\n`);
  } catch (e) {
    appendFileSync(log, `build: ${String(e.message).split("\n")[0]}\n`);
  }
}, 500);
