#!/usr/bin/env node
// arugula vendor: arugula-salad/planter's prod-scaffold.mjs at 90d6e772213c78301b92e89c01377197973b54fc
// arugula vendor: (last changed in 7512bf468f750a69450592bf60ff7446c89dfb37), byte for byte but for these lines. Arugula's planter and
// arugula vendor: apps-platform deploy drivers render with it on a deploy's box (M233,
// arugula vendor: crates/e2e/src/deploy_planter.rs). Don't edit it here: change planter, then run
// arugula vendor: scripts/vendor-planter, which copies it again and captures planter's own output
// arugula vendor: as the fixtures its test renders identically (upstream sha256 c1371a5c872e239e85682b7e0fba92c0dd32294b0375f4eadb4196e8da43ef61).
// planter owns this scaffold. It began as a copy of studio's kit/scripts/prod-scaffold.mjs (534a3ec, branch
// planter-prod), which studio#447 deleted; this is now the only one, and a change to what a prod app gets is
// made here and in ./platform-app. readAppFacts takes the app member that `chant workspace status --json`
// prints for a full checkout, with its `fields` (ws-093: which scripts start, test and migrate it, the
// variables it reads its port and data directory from, its health path), read by appByChant here or
// planter's workspace.mjs (planter#8, planter#26); nothing is taken from delivery/chant.config.ts, and PORT
// and APP_DATA are the names the fields give.
// Prod on home-cloud for a studio-kit app: the files that put a repo made from template/ (or planted
// from an existing one) on the home Kubernetes cluster, in the shared `planter-apps` namespace, as
// <name>.apps.arugula.io. Merging to the repo's main is the deploy: .github/workflows/build.yml runs
// the app's tests, builds the app member into ghcr.io/arugula-salad/<name> for amd64 and arm64,
// then commits `deploy(<name>): pin image sha-<7>` to k8s/kustomization.yaml, and Flux applies k8s/.
//
// The shape is fleet's templates/app-repo (arugula-salad/fleet), with the app member as the build
// context and the planter-apps namespace, whose Namespace, `ghcr` pull secret and
// `planter-apps-secrets` ServiceAccount the platform owns. The files are the repo's once written,
// except k8s/platform/: that is rendered from app.yaml by platform-app (./platform-app/render.mjs) on
// every build of main, so an app changes it through app.yaml and patches in k8s/kustomization.yaml.
//
// prodScaffold() is pure and this module has no dependencies, so planter can copy it and call it
// on a repo it reads through GitHub's API; readAppFacts() finds its options from the repo's files
// through a `read(path)` you give it. From a checkout:
//
//   node prod-scaffold.mjs <repo-dir> <name> [--out <dir>] [--secrets] [--postgres] [--volume <size>|--no-volume] [--metrics <path>] [--force]
//
// writes the files into <repo-dir> (or <out>), refusing to overwrite one unless --force, and prints
// the paths it wrote. <name> is the GitHub repo's name in arugula-salad: the image, the host and
// every resource's prefix.

import { DOMAIN, NAME, PORT, REGISTRY, manifestText, render, topKustomization, validate } from './platform-app/render.mjs';

// The platform's constants and the dashboard are the renderer's; re-exported for what imported them from here.
export { DOMAIN, NAME, NAMESPACE, OTLP, PORT, REGISTRY, dashboard } from './platform-app/render.mjs';

/** The members of a template repo that are not the app: a change only there does not rebuild the image. */
const NOT_APP = ['box', 'delivery', 'design', 'design-client', 'decisions', 'work', 'answers'];

const SIZE = /^[1-9][0-9]*(?:Mi|Gi|Ti)$/;
const PATH = /^\/[A-Za-z0-9._~\/-]*$/;
const DIR = /^(?!\/)(?!.*\.\.)[A-Za-z0-9._\/-]+$/;
const ENV = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** A command as Dockerfile exec form when it is plain words, else run by sh with exec. */
function cmd(start, migrate) {
  const plain = /^[A-Za-z0-9@%+=:,.\/_-]+(?: [A-Za-z0-9@%+=:,.\/_-]+)*$/;
  if (!migrate && plain.test(start)) return JSON.stringify(start.split(' '));
  return JSON.stringify(['sh', '-c', migrate ? `${migrate} && exec ${start}` : `exec ${start}`]);
}

/** The node major an engines range asks for at least (">=22.13" -> 22), else 22. */
export function nodeMajor(engines) {
  const m = /(\d+)/.exec(String(engines ?? ''));
  return m && Number(m[1]) >= 18 ? Number(m[1]) : 22;
}

/**
 * The files that deploy a studio-kit app to prod on home-cloud, as { path: content }, paths relative
 * to the repo's root:
 *
 *   Dockerfile, Dockerfile.dockerignore, .github/workflows/build.yml, app.yaml,
 *   k8s/kustomization.yaml (the image pin, the repo's own), and k8s/platform/: what render() makes of
 *   app.yaml (the Deployment, Service, IngressRoute and Certificate, a Grafana dashboard, and with
 *   `secrets`, `volume`, `metrics` and `postgres` an InfisicalSecret, a PVC, a ServiceMonitor and a
 *   database on apps-pg).
 *
 * @param {object} o
 * @param {string} o.name        the GitHub repo's name in arugula-salad (lowercase, digits, dashes)
 * @param {string} [o.appDir]    the app member's directory, the image's build context ("app")
 * @param {string} [o.start]     the app's start command, run in appDir ("node server.js")
 * @param {string} [o.health]    the app's health path ("/health")
 * @param {number} [o.node]      the node major of the base image (22)
 * @param {boolean} [o.lockfile] whether appDir has a package-lock.json, so npm ci works (true)
 * @param {string} [o.test]      the app's tests, run in appDir before a build ("npm test"); "" for none
 * @param {string} [o.migrate]   a command that applies pending migrations, run before start (none)
 * @param {boolean} [o.secrets]  apply the InfisicalSecret and load <name>-secrets as env (false)
 * @param {string|null} [o.volume] a Longhorn volume at /data of this size ("1Gi"), one replica (none)
 * @param {string|null} [o.metrics] the app's Prometheus metrics path, scraped by a ServiceMonitor (none)
 * @param {boolean} [o.postgres] a database of its own on the shared apps-pg, as DATABASE_URL (false)
 * @param {string} [o.portEnv]   the variable the app reads its port from ("PORT")
 * @param {string} [o.dataEnv]   the variable the app reads its data directory from ("APP_DATA")
 * @returns {Record<string, string>}
 */
export function prodScaffold({
  name,
  appDir = 'app',
  start = 'node server.js',
  health = '/health',
  node = 22,
  lockfile = true,
  test = 'npm test',
  migrate = '',
  secrets = false,
  volume = null,
  metrics = null,
  postgres = false,
  portEnv = 'PORT',
  dataEnv = 'APP_DATA',
} = {}) {
  for (const [what, value] of [['portEnv', portEnv], ['dataEnv', dataEnv]]) if (!ENV.test(value ?? '')) throw new Error(`${what} ${JSON.stringify(value)} must be an environment variable's name`);
  if (!NAME.test(name ?? '')) throw new Error(`name ${JSON.stringify(name)} must be the repo's name: lowercase letters, digits and dashes, at most 50`);
  if (!DIR.test(appDir)) throw new Error(`appDir ${JSON.stringify(appDir)} must be a relative directory`);
  if (!PATH.test(health)) throw new Error(`health ${JSON.stringify(health)} must be a path such as /health`);
  if (!Number.isInteger(node) || node < 18) throw new Error(`node ${JSON.stringify(node)} must be a node major, 18 or later`);
  if (metrics != null && !PATH.test(metrics)) throw new Error(`metrics ${JSON.stringify(metrics)} must be a path such as /metrics`);
  if (volume != null && !SIZE.test(volume)) throw new Error(`volume ${JSON.stringify(volume)} must be a size such as 1Gi`);
  if (!String(start).trim()) throw new Error('start must be the command that runs the app');
  const dir = appDir.replace(/\/+$/, '');
  const host = `${name}.${DOMAIN}`;
  const image = `${REGISTRY}/${name}`;
  const pvc = volume != null;

  const files = {};

  files['Dockerfile'] = `# syntax=docker/dockerfile:1
# ${name}'s production image: the app member (${dir}/) is the build context. Built for amd64 and arm64
# by .github/workflows/build.yml and run from k8s/ as https://${host}. Written by planter
# (arugula-salad/planter prod-scaffold.mjs); edit it freely.
FROM node:${node}-slim
WORKDIR /srv/app
ENV NODE_ENV=production ${portEnv}=${PORT} HOST=0.0.0.0
COPY package*.json ./
RUN ${lockfile ? 'npm ci --omit=dev' : 'npm install --omit=dev'} --no-audit --no-fund && npm cache clean --force
COPY . .
# The box runs the app behind hud's proxy, so the template's server listens on 127.0.0.1. In a pod it
# must listen on every address: this preload rebinds a loopback listen to HOST. It also stops the app
# on SIGTERM when the app does not handle it (node as PID 1 ignores it), closing its servers first.
# Once the app listens on process.env.HOST itself and handles SIGTERM, drop it and NODE_OPTIONS.
COPY <<'EOF' /opt/studio/pod.cjs
'use strict';
const net = require('node:net');
const host = process.env.HOST;
const loopback = /^(127\\.0\\.0\\.1|localhost|::1)$/;
const servers = new Set();
const listen = net.Server.prototype.listen;
net.Server.prototype.listen = function (...args) {
  if (host && typeof args[1] === 'string' && loopback.test(args[1])) args[1] = host;
  else if (host && args[0] && typeof args[0] === 'object' && loopback.test(String(args[0].host))) args[0] = { ...args[0], host };
  servers.add(this);
  return listen.apply(this, args);
};
process.on('SIGTERM', () => {
  if (process.listenerCount('SIGTERM') > 1) return;
  for (const server of servers) { server.close(); server.closeIdleConnections?.(); }
  setTimeout(() => process.exit(0), 10000).unref();
});
EOF
ENV NODE_OPTIONS=--require=/opt/studio/pod.cjs
ARG APP_REVISION=dev
ENV APP_REVISION=$APP_REVISION
# The image's node user, by number: the pod's runAsNonRoot needs a numeric user. The root filesystem
# is read-only in the pod; the app writes only /tmp${pvc ? ` and its volume at /data (${dataEnv})` : ''}.
USER 1000:1000
EXPOSE ${PORT}
CMD ${cmd(start, migrate)}
`;

  files['Dockerfile.dockerignore'] = `# What the image leaves out of the build context, ${dir}/ (BuildKit reads <Dockerfile>.dockerignore).
node_modules
test
.data
.env*
*.md
`;

  const paths = [`${dir}/**`, `!${dir}/**/*.md`, 'Dockerfile', 'Dockerfile.dockerignore', '.github/workflows/build.yml', 'app.yaml'];
  files['.github/workflows/build.yml'] = `# Runs the app's tests, builds the app member (${dir}/) into ${image} for amd64 and arm64, then pins
# that exact build into k8s/kustomization.yaml, with k8s/platform/ rendered again from app.yaml by
# the platform (arugula-salad/planter's platform-app, at main: a platform change reaches the app on
# its next build). That commit is the deploy: Flux applies k8s/ to the planter-apps namespace on
# home-cloud, serving https://${host}. A pull request that touches app.yaml or k8s/ is checked: its
# k8s/platform/ must be what its app.yaml renders.
#
# Only a change to the app, its image or app.yaml builds: the other members (${NOT_APP.filter((m) => m !== dir).join(', ')}),
# Markdown and k8s/ do not, so the pin commit below does not trigger another build (GitHub also
# starts no workflow for a push made with GITHUB_TOKEN). workflow_dispatch builds on demand.
# Written by planter (arugula-salad/planter prod-scaffold.mjs); edit it freely.
name: build

on:
  workflow_dispatch:
  push:
    branches: [main]
    paths:
${paths.map((p) => `      - "${p}"`).join('\n')}
  pull_request:
    paths:
      - "app.yaml"
      - "k8s/**"

concurrency:
  group: build-\${{ github.ref }}
  cancel-in-progress: false

env:
  IMAGE: ghcr.io/\${{ github.repository_owner }}/${name}

jobs:
  platform:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - name: k8s/platform/ is what app.yaml renders
        uses: arugula-salad/planter/platform-app@main
        with:
          mode: check

  build:
    if: github.event_name != 'pull_request'
    runs-on: ubuntu-latest
    permissions:
      contents: write
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: ${node}
${test ? `      # The app's own tests, with the whole repo checked out (they read the design member's screen specs).
      - name: App tests
        working-directory: ${dir}
        run: |
          ${lockfile ? 'npm ci' : 'npm install'} --no-audit --no-fund
          ${test}
` : ''}      - uses: docker/setup-qemu-action@v3
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: \${{ github.actor }}
          password: \${{ secrets.GITHUB_TOKEN }}
      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          context: ${dir}
          file: Dockerfile
          platforms: linux/amd64,linux/arm64
          push: true
          build-args: |
            APP_REVISION=\${{ github.sha }}
          tags: |
            \${{ env.IMAGE }}:latest
            \${{ env.IMAGE }}:sha-\${{ github.sha }}
          labels: |
            org.opencontainers.image.source=https://github.com/\${{ github.repository }}
            org.opencontainers.image.revision=\${{ github.sha }}
      - name: Render k8s/platform/ from app.yaml
        uses: arugula-salad/planter/platform-app@main
      - name: Pin the image
        run: |
          sed -i -E "s#^(    newTag: ).*#\\1sha-\${GITHUB_SHA}#" k8s/kustomization.yaml
          git add k8s/
          git diff --cached --quiet && { echo "already pinned and rendered"; exit 0; }
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git commit -m "deploy(${name}): pin image sha-\${GITHUB_SHA::7}"
          git pull --rebase
          git push
`;

  // app.yaml and what the platform renders from it (platform-app/render.mjs): k8s/platform/ is
  // rendered again on every build of main; k8s/kustomization.yaml (the image pin) is the repo's.
  files['app.yaml'] = manifestText({ name, health, volume, portEnv, dataEnv, secrets, metrics, postgres });
  const spec = validate({ name, health, runtime: 'k8s', ...(volume != null ? { volume } : {}), ...(portEnv !== 'PORT' || dataEnv !== 'APP_DATA' ? { env: { port: portEnv, data: dataEnv } } : {}), needs: { domain: [host], secrets, telemetry: metrics ? { metrics } : true, ...(postgres ? { postgres: {} } : {}) } });
  files['k8s/kustomization.yaml'] = topKustomization(spec);
  Object.assign(files, render(spec));

  return files;
}


/**
 * prodScaffold's options for a repo, but its name: the app member as chant reads it (`app`, `{ dir,
 * fields }`, from `chant workspace status --json` on a full checkout: planter's workspace.mjs, or
 * appByChant below), and the repo's files through `read(path)`, which returns a file's text or
 * null/undefined when it is absent (sync or async; paths are relative to the repo's root). The fields
 * say which package.json scripts start, test and migrate the app, the variables it reads its port and
 * data directory from, and its health path (the app kind's defaults when its kind declares none). The
 * files give what chant doesn't: <dir>/package.json (engines.node, and the start script's own command,
 * which the image runs as its main process) and whether <dir>/package-lock.json exists.
 */
export async function readAppFacts(read, { app } = {}) {
  if (!app?.dir) throw new Error('prod needs the app member as chant reads it: declare one member of kind app (pin kinds/app)');
  const json = async (path) => {
    const text = await read(path);
    if (text == null) return undefined;
    try { return JSON.parse(text); } catch { throw new Error(`${path} is not JSON`); }
  };
  const appDir = String(app.dir).replace(/^\.\/|\/+$/g, '');
  const fields = app.fields ?? {};
  const names = { start: 'start', test: 'test', migrate: 'migrate', ...fields.scripts };
  const env = { port: 'PORT', data: 'APP_DATA', ...fields.env };
  const pkg = await json(`${appDir}/package.json`);
  if (!pkg) throw new Error(`${appDir}/package.json is missing: prod needs the app member to be a Node app`);
  const scripts = pkg.scripts ?? {};
  // A script the fields name that package.json doesn't have means the app has none (ws-093).
  const has = (part) => typeof scripts[names[part]] === 'string' && scripts[names[part]].trim() !== '';
  if (!has('start')) throw new Error(`${appDir}/package.json has no "${names.start}" script, which starts the app`);
  return {
    appDir,
    start: scripts[names.start],
    health: fields.health ?? '/health',
    node: nodeMajor(pkg.engines?.node),
    lockfile: (await read(`${appDir}/package-lock.json`)) != null,
    test: has('test') ? (names.test === 'test' ? 'npm test' : `npm run ${names.test}`) : '',
    migrate: has('migrate') ? `npm run ${names.migrate}` : '',
    // An app that migrates keeps its data (SQLite in its data directory, the template's convention): give it a volume.
    volume: has('migrate') ? '1Gi' : null,
    // The app kind's field `postgres: true` asks for a database of its own on the shared apps-pg.
    postgres: fields.postgres === true,
    portEnv: env.port,
    dataEnv: env.data,
  };
}

/**
 * The app member of the checkout at `root`, as chant reads it (planter's workspace.mjs, appOfCheckout:
 * `chant workspace status --json`, CHANT or chant on PATH); undefined when there is no declaration.
 */
async function appByChant(root) {
  const { existsSync } = await import('node:fs');
  const { join } = await import('node:path');
  if (!existsSync(join(root, 'chant.workspace.json')) && !existsSync(join(root, 'chant.workspace.jsonc'))) return undefined;
  const { appOfCheckout } = await import('./workspace.mjs');
  return appOfCheckout(root, { chant: process.env.CHANT || 'chant' });
}

const main = typeof process !== 'undefined' && process.argv?.[1]
  && (await import('node:url')).pathToFileURL((await import('node:fs')).realpathSync(process.argv[1])).href === import.meta.url;

if (main) {
  const { existsSync, mkdirSync, readFileSync, writeFileSync } = await import('node:fs');
  const { dirname, join, resolve } = await import('node:path');
  const args = process.argv.slice(2);
  const flag = (f) => { const i = args.indexOf(f); if (i < 0) return false; args.splice(i, 1); return true; };
  const opt = (f) => { const i = args.indexOf(f); if (i < 0) return undefined; return args.splice(i, 2)[1]; };
  const out = opt('--out');
  const size = opt('--volume');
  const noVolume = flag('--no-volume');
  const secrets = flag('--secrets');
  const postgres = flag('--postgres');
  const metrics = opt('--metrics');
  const force = flag('--force');
  const [repo, name] = args;
  if (!repo || !name || args.length > 2) {
    process.stderr.write('usage: prod-scaffold.mjs <repo-dir> <name> [--out <dir>] [--secrets] [--postgres] [--volume <size>|--no-volume] [--metrics <path>] [--force]\n');
    process.exit(2);
  }
  try {
    const root = resolve(repo);
    const facts = await readAppFacts((p) => (existsSync(join(root, p)) ? readFileSync(join(root, p), 'utf8') : null), { app: await appByChant(root) });
    const files = prodScaffold({ ...facts, name, secrets, postgres: postgres || facts.postgres, metrics: metrics ?? null, volume: noVolume ? null : size ?? facts.volume });
    const dest = resolve(out ?? root);
    const clash = Object.keys(files).filter((p) => existsSync(join(dest, p)));
    if (clash.length && !force) throw new Error(`${clash.join(', ')} already in ${dest}; --force overwrites`);
    for (const [path, content] of Object.entries(files)) {
      mkdirSync(dirname(join(dest, path)), { recursive: true });
      writeFileSync(join(dest, path), content);
      process.stdout.write(`${join(dest, path)}\n`);
    }
  } catch (error) {
    process.stderr.write(`prod-scaffold: ${error.message}\n`);
    process.exit(1);
  }
}
