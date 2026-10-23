// arugula vendor: arugula-salad/planter's platform-app/render.mjs at 90d6e772213c78301b92e89c01377197973b54fc
// arugula vendor: (last changed in 795d81b6704e8771ed7ecfd6eb5fb3cada4aee78), byte for byte but for these lines. Arugula's planter and
// arugula vendor: apps-platform deploy drivers render with it on a deploy's box (M233,
// arugula vendor: crates/e2e/src/deploy_planter.rs). Don't edit it here: change planter, then run
// arugula vendor: scripts/vendor-planter, which copies it again and captures planter's own output
// arugula vendor: as the fixtures its test renders identically (upstream sha256 6001ef11ffeda1882e7928fb58eecd5e0399dbd0010a944842c5238f82c49eff).
// PlatformApp: an app's app.yaml, rendered into the Kubernetes objects that run it in the shared
// `planter-apps` namespace on home-cloud as <name>.apps.arugula.io (the PaaS design, phase 2).
//
// app.yaml says what the app needs from the platform; this module says how the platform gives it.
// The output is k8s/platform/, which the app's build re-renders on every build of main through the
// action next to this file (platform-app/action.yml), so a change here reaches every app on its next
// build. k8s/platform/ is generated: an app changes what it gets through app.yaml, and anything
// app.yaml can't say through patches in its own k8s/kustomization.yaml, which nothing regenerates.
//
// validate() and render() are pure and this module has no dependencies: planter's prod flow calls them
// to write a new app's first k8s/, and cli.mjs (which parses the YAML) calls them in an app's CI.
//
// app.yaml, v0. Unknown keys are errors, so a typo never quietly does nothing:
//
//   name: fern                   # the repo's name in arugula-salad: the image, the host, every object's prefix
//   port: 8080                   # the container port; the app reads it from env.port (default 8080)
//   health: /health              # readiness and liveness path (default /health)
//   runtime: k8s                 # k8s (default), or loam: see "On loam" below
//   volume: 1Gi                  # a Longhorn volume at /data, one replica replaced on deploy (default none)
//   env: { port: PORT, data: APP_DATA }   # the variables the app reads its port and data directory from
//   needs:
//     domain: [fern.apps.arugula.io]      # the default. Also fern.apps.inevitable.fyi (the platform's second
//                                         # wildcard), and custom domains: see "Custom domains" below
//     secrets: [STRIPE_KEY]               # Infisical planter-apps-md-qs / prod / /<name>, loaded as env;
//                                         # true loads the folder without naming what's in it
//     telemetry: true                     # OTel env and a Grafana dashboard (default true);
//                                         # { metrics: /metrics } also has Prometheus scrape the app
//     postgres: {}                        # a database and role of its own on the shared apps-pg,
//                                         # as DATABASE_URL and the PG* variables
//     bucket: uploads                     # a Garage bucket of its own, pa-<name>-uploads (5 GiB at most),
//                                         # as S3_* and AWS_* variables. Lowercase letters and digits, no dashes
//     conversations:                      # agents the app talks to: see "Conversations" below
//       agents:
//         support:
//           runtime: claude               # a harness: claude or codex (RUNTIMES)
//           model: anthropic/claude-sonnet-5   # optional: the harness's default otherwise
//           instructions: agents/support.md    # a file in the repo, read in at render
//           repos: [https://github.com/acme/kb.git]   # optional: public repos cloned onto the machine
//           secrets: [ZENDESK_TOKEN]      # optional: names from needs.secrets, given to the agent
//           sandbox: { persistent: false }     # optional, and only false: every conversation has a machine of
//                                              # its own, which keeps its files between turns (managoat/fountain ADR 0073)
//       budget_usd_month: 10              # accepted and ignored: the service has no credits (managoat/fountain#2609)
//       concurrency: 2                    # accepted and ignored: the service has no per-account cap (managoat/fountain#2638)
//     analytics: true                     # product analytics: see "Analytics" below
//
// On loam (runtime: loam, phase 5): the app runs on loam, home-cloud's Firecracker sandboxes, as one
// sandbox of loam project LOAM_PROJECT named after it, at https://<name>.LOAM_DOMAIN. It sleeps when
// nobody uses it and wakes on the next request. It is where agent-written apps run until a person
// promotes one to k8s by merging a pull request that changes runtime. loam can't reach the cluster, so
// an app there may declare only its domain (its own name under LOAM_DOMAIN), secrets (given to it as
// env when its sandbox is made) and telemetry: logs (its service's log, kept by loam), and no volume:
// its disk is its sandbox's, which every deploy replaces. render() makes the Deployment with no
// replicas: nothing runs in the cluster, but Flux applies it and the image pin rewrites it, so it is
// the record home-cloud's loam provisioner (apps/apps-platform) makes the sandbox from. The image must
// be pinned by digest (k8s/kustomization.yaml's images: digest), which loam needs.
//
// Conversations: each agent under needs.conversations.agents is made, by home-cloud's conversations
// provisioner (apps/apps-platform), in an account of the app's own on the platform's conversation
// service, running on loam. The app gets CONVERSATIONS_URL, CONVERSATIONS_TOKEN (its account's key) and
// CONVERSATIONS_AGENT_<NAME> per agent (the agent's id) as env, from Infisical /<name>/conversations,
// which the provisioner writes. Not yet: a schedule, and conversations for an app on loam.
//
// Analytics: home-cloud's analytics provisioner (apps/apps-platform) makes the app a project of its
// own on the platform's OpenPanel, whose CORS origins are the app's hosts (browsers may send events
// only from them), and a client for it. The app gets ANALYTICS_CLIENT_ID, ANALYTICS_CLIENT_SECRET,
// ANALYTICS_URL (the public ingest, for OpenPanel's browser SDK) and ANALYTICS_API_URL (the api's
// Service, for its server SDKs) as env, from Infisical /<name>/analytics, which the provisioner writes.
// Not yet for an app on loam.
//
// Custom domains: a host in needs.domain that isn't one of the app's two names is routed, and certified
// over HTTP-01, once home-cloud grants it to the app. The app's registration asks for it (the annotation
// platform.arugula.io/domains on its Kustomization in arugula-salad/planter-apps, which planter writes
// from this list at Go live); once its A record (or CNAME) points at ADDRESS, home-cloud's domain
// provisioner (apps/apps-platform) opens a pull request that adds it to the `# custom-hosts` line of
// platform/planter-apps-policy/hosts.yaml, and a person merges it. Until then home-cloud refuses the
// app's routes for the host, which fails its deploy. Hosts under the platform's own zones
// (RESERVED_ZONES) aren't an app's to have, except for Jake's own apps (JAKES_REGISTRIES), whose zones they are.

/** A name that is a DNS label, a GitHub repo name and an image name at once, short enough for `<name>-secrets`. */
export const NAME = /^[a-z0-9](?:[a-z0-9-]{0,48}[a-z0-9])?$/;

export const NAMESPACE = 'planter-apps';
export const DOMAIN = 'apps.arugula.io';
/** The platform's second wildcard: <name>.apps.inevitable.fyi, on one certificate the fleet keeps for every app. */
export const SECOND_DOMAIN = 'apps.inevitable.fyi';
/** The fleet's certificate for *.apps.inevitable.fyi, in NAMESPACE (arugula-salad/fleet, planter-apps.yaml). */
export const SECOND_DOMAIN_TLS = 'planter-apps-wildcard-inevitable';
/** Where WAN 80 and 443 reach the cluster: a custom domain points its A record here. */
export const ADDRESS = '38.42.123.76';
/** Zones whose names the platform gives out; an app gets only its own two names under them (home-cloud's apps-platform). */
export const RESERVED_ZONES = ['inevitable.fyi', 'arugula.io', 'managoat.com', 'jakegaylor.com', 'widgets.wtf', 'skipto.tv'];
/** Garage's S3 API in the cluster (home-cloud's apps/garage), path-style. */
export const S3_ENDPOINT = 'http://s3.garage.svc.cluster.local:3900';
export const REGISTRY = 'ghcr.io/arugula-salad';
/** The registry Jake's private apps push to: the private Forgejo org, whose packages need a login. */
export const PRIVATE_REGISTRY = 'git.inevitable.fyi/jhgaylor-private';
/**
 * Where an app's image may come from, by registry, and the pull secret in NAMESPACE that reaches it
 * (null: the registry serves the image anonymously). An arugula-salad app builds to ghcr, through the
 * fleet's `ghcr` secret; one of Jake's, registered from Forgejo jhgaylor/apps (home-cloud's
 * platform/jhgaylor-apps), builds to Forgejo's registry under jhgaylor, whose images are public, or,
 * when its code is private, under jhgaylor-private, pulled with a Secret of the app's own,
 * <name>-registry (OWN_PULL_SECRET). Jake's managoat demos (GitHub managoat/demos, also in his list)
 * keep building to ghcr under managoat, whose images are public. The app's CI says which
 * (cli.mjs --registry); app.yaml can't.
 */
const OWN_PULL_SECRET = Symbol('<name>-registry');
export const MANAGOAT_REGISTRY = 'ghcr.io/managoat';
export const REGISTRIES = { [REGISTRY]: 'ghcr', 'git.inevitable.fyi/jhgaylor': null, [PRIVATE_REGISTRY]: OWN_PULL_SECRET, [MANAGOAT_REGISTRY]: null };
/**
 * The registries of Jake's own apps (home-cloud's jhgaylor/apps list). Those apps may ask for hosts
 * under RESERVED_ZONES, which are his: a site that already lives at shop.managoat.com keeps its name
 * when it moves onto the platform. Asking isn't having: home-cloud grants every custom host by a
 * reviewed pull request (platform/planter-apps-policy/hosts.yaml), and refuses one under these zones
 * to an app of any other list.
 */
export const JAKES_REGISTRIES = ['git.inevitable.fyi/jhgaylor', PRIVATE_REGISTRY, MANAGOAT_REGISTRY];
/** The port the image serves on unless app.yaml says otherwise; the app reads it from env.port (PORT). */
export const PORT = 8080;
/** Tempo's OTLP/HTTP receiver: an app with an OpenTelemetry SDK sends its traces here, unconfigured. */
export const OTLP = 'http://tempo.tempo.svc:4318';
/** The Infisical project every app in NAMESPACE reads its secrets from, a folder each. */
export const SECRETS_PROJECT = 'planter-apps-md-qs';
/** The shared Postgres (home-cloud's apps/apps-pg), read-write Service: an app's database is named after it. */
export const POSTGRES_HOST = 'apps-pg-rw.apps-pg.svc.cluster.local';
/** Where runtime: loam apps are: <name>.LOAM_DOMAIN, the URL domain of loam's project LOAM_PROJECT. */
export const LOAM_DOMAIN = 'run.inevitable.fyi';
export const LOAM_PROJECT = 'apps';
/** loam's longest sandbox name (a DNS label of 32, with its project in 54). */
const LOAM_NAME_MAX = 32;
/** Where render() writes, relative to the repo's root. Everything in it is generated. */
export const OUT = 'k8s/platform';

const SIZE = /^[1-9][0-9]*(?:Mi|Gi|Ti)$/;
const PATH = /^\/[A-Za-z0-9._~\/-]*$/;
const ENV = /^[A-Za-z_][A-Za-z0-9_]*$/;
const HOST = /^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
// No dashes, so no app's pa-<app>-<bucket> can spell another app's.
const BUCKET = /^[a-z0-9]{1,20}$/;

const TOP = ['name', 'image', 'port', 'health', 'runtime', 'volume', 'env', 'needs'];
const NEEDS = ['domain', 'secrets', 'telemetry', 'postgres', 'bucket', 'conversations', 'analytics'];
// The harnesses an agent can run, each to its name on the conversation service. Add one here (and to
// the service) to offer it; nothing else in app.yaml names a harness.
export const RUNTIMES = { claude: 'claude', codex: 'codex' };
const AGENT = /^[a-z](?:[a-z0-9-]{0,30}[a-z0-9])?$/;
const FILE = /^(?!\/)(?!.*(?:^|\/)\.\.(?:\/|$))[A-Za-z0-9._\/-]+$/;
const REPO_URL = /^https:\/\/[A-Za-z0-9.-]+\/[A-Za-z0-9._\/-]+$/;
/** Where an app in the cluster reaches the conversation service: its Service, not the public host. */
export const CONVERSATIONS_URL = 'http://fountain.fountain.svc.cluster.local';
/** The env var an agent's id arrives in: CONVERSATIONS_AGENT_SUPPORT for support. */
/** Where browsers send an app's events (OpenPanel's ingest), and where its server does (the api's Service). */
export const ANALYTICS_URL = 'https://t.inevitable.fyi';
export const ANALYTICS_API_URL = 'http://openpanel-api.openpanel.svc.cluster.local:3000';
export const agentEnv = (agent) => `CONVERSATIONS_AGENT_${agent.toUpperCase().replace(/-/g, '_')}`;

/** An error in app.yaml: its message names the key and what it should be. */
export class ManifestError extends Error {}

const fail = (message) => { throw new ManifestError(message); };
const isObject = (v) => v != null && typeof v === 'object' && !Array.isArray(v);
const unknown = (obj, allowed, where) => {
  const extra = Object.keys(obj).filter((k) => !allowed.includes(k));
  if (extra.length) fail(`${where}${extra.map((k) => JSON.stringify(k)).join(', ')}: not ${extra.length > 1 ? 'keys' : 'a key'} app.yaml knows (${allowed.join(', ')})`);
};

/**
 * app.yaml, parsed, checked and with its defaults filled in: the spec render() takes. Throws a
 * ManifestError naming the first thing wrong. `repo`, when given, is the repo's name, which `name`
 * must be (the host and image are named after it, and home-cloud's admission policy holds the app to them).
 */
export function validate(manifest, { repo, registry = REGISTRY, readFile = null } = {}) {
  if (!Object.hasOwn(REGISTRIES, registry)) fail(`registry ${JSON.stringify(registry)} must be one of ${Object.keys(REGISTRIES).join(', ')}`);
  if (!isObject(manifest)) fail('app.yaml must be a mapping of keys (name, port, health, needs, ...)');
  unknown(manifest, TOP, '');
  const { name } = manifest;
  if (!NAME.test(name ?? '')) fail(`name ${JSON.stringify(name)} must be the repo's name: lowercase letters, digits and dashes, at most 50`);
  if (repo != null && name !== repo) fail(`name ${JSON.stringify(name)} must be the repo's name, ${JSON.stringify(repo)}`);
  const pullSecret = REGISTRIES[registry] === OWN_PULL_SECRET ? `${name}-registry` : REGISTRIES[registry];
  const image = `${registry}/${name}`;
  if (manifest.image != null && manifest.image !== image) fail(`image ${JSON.stringify(manifest.image)} must be ${image}, which the build pushes (or leave it out)`);
  const port = manifest.port ?? PORT;
  if (!Number.isInteger(port) || port < 1024 || port > 65535) fail(`port ${JSON.stringify(port)} must be a port number from 1024 to 65535 (the image runs unprivileged)`);
  const health = manifest.health ?? '/health';
  if (!PATH.test(health)) fail(`health ${JSON.stringify(health)} must be a path such as /health`);
  const runtime = manifest.runtime ?? 'k8s';
  if (runtime !== 'k8s' && runtime !== 'loam') fail(`runtime ${JSON.stringify(runtime)} must be k8s or loam`);
  if (runtime === 'loam' && registry !== REGISTRY) fail(`runtime: loam takes its image from ${REGISTRY} only (loam's allowed registries), not ${registry}`);
  if (runtime === 'loam') {
    if (isObject(manifest.needs) && manifest.needs.conversations !== undefined) fail('needs.conversations: not yet for an app on loam; promote it to runtime k8s for that');
    if (isObject(manifest.needs) && manifest.needs.analytics !== undefined) fail('needs.analytics: not yet for an app on loam; promote it to runtime k8s for that');
    return { ...validateLoam(manifest, { name, image, port, health }), pullSecret };
  }
  const volume = manifest.volume ?? null;
  if (volume != null && !SIZE.test(volume)) fail(`volume ${JSON.stringify(volume)} must be a size such as 1Gi`);
  const env = manifest.env ?? {};
  if (!isObject(env)) fail('env must be a mapping: { port: PORT, data: APP_DATA }');
  unknown(env, ['port', 'data'], 'env: ');
  const portEnv = env.port ?? 'PORT';
  const dataEnv = env.data ?? 'APP_DATA';
  for (const [what, value] of [['env.port', portEnv], ['env.data', dataEnv]]) if (!ENV.test(value)) fail(`${what} ${JSON.stringify(value)} must be an environment variable's name`);

  const needs = manifest.needs ?? {};
  if (!isObject(needs)) fail('needs must be a mapping (domain, secrets, telemetry, postgres, bucket)');
  unknown(needs, NEEDS, 'needs: ');
  const analytics = needs.analytics ?? false;
  if (typeof analytics !== 'boolean') fail('needs.analytics must be true or false');
  const postgres = needs.postgres !== undefined;
  if (postgres) {
    if (!isObject(needs.postgres)) fail('needs.postgres must be {} (a database on the shared cluster)');
    unknown(needs.postgres, ['dedicated'], 'needs.postgres: ');
    if (needs.postgres.dedicated === true) fail('needs.postgres.dedicated: not yet (an app gets a database on the shared cluster apps-pg; a cluster of its own comes later)');
    if (needs.postgres.dedicated !== undefined && needs.postgres.dedicated !== false) fail('needs.postgres.dedicated must be true or false');
  }
  let bucket = null;
  if (needs.bucket !== undefined) {
    if (typeof needs.bucket !== 'string' || !BUCKET.test(needs.bucket)) fail(`needs.bucket ${JSON.stringify(needs.bucket)} must be a bucket name: lowercase letters and digits, no dashes, at most 20, such as uploads`);
    if (name.length + needs.bucket.length + 4 > 63) fail(`needs.bucket ${needs.bucket}: the bucket is named pa-${name}-${needs.bucket}, which is longer than Garage's 63; use a shorter name`);
    bucket = needs.bucket;
  }
  const own = `${name}.${DOMAIN}`;
  const second = `${name}.${SECOND_DOMAIN}`;
  const domains = needs.domain ?? [own];
  if (!Array.isArray(domains) || domains.length === 0 || domains.some((d) => typeof d !== 'string')) fail(`needs.domain must be a list of hosts, such as [${own}]`);
  if (new Set(domains).size !== domains.length) fail('needs.domain lists a host more than once');
  const custom = [];
  for (const d of domains) {
    if (d === own || d === second) continue;
    if (!HOST.test(d)) fail(`needs.domain ${JSON.stringify(d)} must be a lowercase host name, such as shop.example.org (no wildcards)`);
    if (d.endsWith(`.${DOMAIN}`) || d.endsWith(`.${SECOND_DOMAIN}`)) fail(`needs.domain ${d}: under the platform's wildcards an app is ${own} and ${second}, by its name`);
    const zone = RESERVED_ZONES.find((z) => d === z || d.endsWith(`.${z}`));
    if (zone && !JAKES_REGISTRIES.includes(registry)) fail(`needs.domain ${d}: names under ${zone} are the platform's to give; an app gets ${own} and ${second}`);
    custom.push(d);
  }
  // The address the app is known by: its own name if it has it, else the first host it lists.
  const host = domains.includes(own) ? own : domains.includes(second) ? second : custom[0];
  const declared = needs.secrets ?? false;
  if (declared !== true && declared !== false && (!Array.isArray(declared) || declared.some((s) => typeof s !== 'string' || !ENV.test(s)))) fail('needs.secrets must be a list of environment variable names, such as [STRIPE_KEY], or true');
  const secrets = Array.isArray(declared) ? declared : [];
  if (new Set(secrets).size !== secrets.length) fail('needs.secrets names a secret more than once');
  const secretsOn = declared === true || secrets.length > 0;
  let telemetry = needs.telemetry ?? true;
  let metrics = null;
  if (isObject(telemetry)) {
    unknown(telemetry, ['metrics'], 'needs.telemetry: ');
    metrics = telemetry.metrics ?? null;
    if (metrics == null || !PATH.test(metrics)) fail(`needs.telemetry.metrics ${JSON.stringify(metrics)} must be the app's Prometheus metrics path, such as /metrics`);
    telemetry = true;
  } else if (typeof telemetry !== 'boolean') fail('needs.telemetry must be true, false, or { metrics: /metrics }');

  const conversations = needs.conversations === undefined ? null : validateConversations(needs.conversations, { secrets, readFile });

  return { name, image, port, health, runtime, volume, portEnv, dataEnv, host, ownHost: domains.includes(own), secondHost: domains.includes(second), custom, secrets, secretsOn, telemetry, metrics, postgres, bucket, ...(conversations ? { conversations } : {}), ...(analytics ? { analytics } : {}), pullSecret };
}

// needs.conversations: the agents, each with its instructions read in from the repo (readFile is the
// caller's: cli.mjs reads from the checkout). budget_usd_month and concurrency are accepted and
// ignored for now, so apps that still say them keep rendering: the conversation service dropped
// credits (managoat/fountain#2609) and its per-account sandbox cap (#2638), so neither reaches the
// claim. Each agent's model runs on a key the app gives it among its secrets (the service has no
// platform keys since its ADR 0075), and that key's limit at the provider is the app's spend
// ceiling. Take both out of app.yaml; a later release refuses them as unknown keys.
function validateConversations(conv, { secrets, readFile }) {
  if (!isObject(conv)) fail('needs.conversations must be a mapping: { agents: { <name>: {...} } }');
  unknown(conv, ['agents', 'budget_usd_month', 'concurrency'], 'needs.conversations: ');
  if (!isObject(conv.agents) || Object.keys(conv.agents).length === 0) fail('needs.conversations.agents must name at least one agent, such as { support: { runtime: claude, instructions: agents/support.md } }');
  const agents = [];
  for (const [agent, def] of Object.entries(conv.agents)) {
    const where = `needs.conversations.agents.${agent}`;
    if (!AGENT.test(agent)) fail(`${where}: an agent's name is lowercase letters, digits and dashes, starting with a letter, at most 32`);
    if (!isObject(def)) fail(`${where} must be a mapping (runtime, model, instructions, repos, secrets, sandbox)`);
    unknown(def, ['runtime', 'model', 'instructions', 'repos', 'secrets', 'sandbox', 'schedule'], `${where}: `);
    if (def.schedule !== undefined) fail(`${where}.schedule: not yet (agents start when the app sends a prompt)`);
    if (!Object.hasOwn(RUNTIMES, def.runtime)) fail(`${where}.runtime ${JSON.stringify(def.runtime)} must be one of ${Object.keys(RUNTIMES).join(', ')}`);
    const model = def.model ?? null;
    if (model !== null && (typeof model !== 'string' || !/^[A-Za-z0-9._\/:-]{1,100}$/.test(model))) fail(`${where}.model ${JSON.stringify(model)} must be a model name, such as anthropic/claude-sonnet-5`);
    if (typeof def.instructions !== 'string' || !FILE.test(def.instructions)) fail(`${where}.instructions must be a file in the repo, such as agents/${agent}.md`);
    if (!readFile) fail(`${where}.instructions: rendering an app with conversations needs its checkout, to read ${def.instructions}`);
    let instructions;
    try { instructions = readFile(def.instructions); } catch { fail(`${where}.instructions: ${def.instructions} isn't a file in the repo`); }
    if (typeof instructions !== 'string' || instructions.trim() === '') fail(`${where}.instructions: ${def.instructions} is empty`);
    if (Buffer.byteLength(instructions) > 32 * 1024) fail(`${where}.instructions: ${def.instructions} is over 32 KiB`);
    const repos = def.repos ?? [];
    if (!Array.isArray(repos) || repos.some((r) => typeof r !== 'string' || !REPO_URL.test(r))) fail(`${where}.repos must be a list of https git URLs of public repos`);
    const agentSecrets = def.secrets ?? [];
    if (!Array.isArray(agentSecrets) || agentSecrets.some((x) => typeof x !== 'string')) fail(`${where}.secrets must be a list of names from needs.secrets`);
    for (const x of agentSecrets) if (!secrets.includes(x)) fail(`${where}.secrets: ${x} isn't in needs.secrets, which is where the app's secrets are named`);
    const sandbox = def.sandbox ?? {};
    if (!isObject(sandbox)) fail(`${where}.sandbox must be a mapping: { persistent: false }`);
    unknown(sandbox, ['persistent'], `${where}.sandbox: `);
    const persistent = sandbox.persistent ?? false;
    if (typeof persistent !== 'boolean') fail(`${where}.sandbox.persistent must be true or false`);
    // Fountain removed persistent machines (managoat/fountain ADR 0073) and refuses an agent that asks for one,
    // so it is refused here, at the app's build, rather than by the provisioner later. A conversation keeps its
    // own machine and its files between turns: to keep files, keep prompting the same conversation.
    if (persistent) fail(`${where}.sandbox.persistent: true is gone: every conversation has a machine of its own, which keeps its files between turns. Remove it, and keep prompting one conversation to keep its files`);
    agents.push({ name: agent, runtime: RUNTIMES[def.runtime], model, instructions, repos, secrets: agentSecrets });
  }
  return { agents };
}

// app.yaml for runtime: loam: what an app on loam can have, which is less than one in the cluster.
function validateLoam(manifest, { name, image, port, health }) {
  const where = 'runtime loam';
  if (name.length > LOAM_NAME_MAX) fail(`name ${name}: an app on loam is a sandbox named after it, at most ${LOAM_NAME_MAX} long`);
  if (manifest.volume != null) fail(`volume: an app on loam has no volume (its disk is its sandbox's, which every deploy replaces); ${where} keeps nothing across deploys`);
  if (manifest.env != null) {
    if (!isObject(manifest.env)) fail('env must be a mapping: { port: PORT }');
    unknown(manifest.env, ['port'], 'env: ');
  }
  const portEnv = manifest.env?.port ?? 'PORT';
  if (!ENV.test(portEnv)) fail(`env.port ${JSON.stringify(portEnv)} must be an environment variable's name`);
  const needs = manifest.needs ?? {};
  if (!isObject(needs)) fail('needs must be a mapping (domain, secrets, telemetry)');
  unknown(needs, NEEDS, 'needs: ');
  for (const k of ['postgres', 'bucket']) if (needs[k] !== undefined) fail(`needs.${k}: an app on loam can't reach the cluster, where ${k === 'postgres' ? 'apps-pg' : 'Garage'} is; promote it to runtime k8s for that`);
  const host = `${name}.${LOAM_DOMAIN}`;
  const domains = needs.domain ?? [host];
  if (!Array.isArray(domains) || domains.length !== 1 || domains[0] !== host) fail(`needs.domain: an app on loam is ${host}, and only that (leave it out, or [${host}])`);
  const declared = needs.secrets ?? false;
  if (declared !== true && declared !== false && (!Array.isArray(declared) || declared.some((s) => typeof s !== 'string' || !ENV.test(s)))) fail('needs.secrets must be a list of environment variable names, such as [STRIPE_KEY], or true');
  const secrets = Array.isArray(declared) ? declared : [];
  if (new Set(secrets).size !== secrets.length) fail('needs.secrets names a secret more than once');
  const telemetry = needs.telemetry ?? 'logs';
  if (telemetry !== 'logs' && telemetry !== false) fail(`needs.telemetry: an app on loam has logs (its service's, kept by loam) or nothing: logs or false, not ${JSON.stringify(telemetry)}`);
  return { name, image, port, health, runtime: 'loam', volume: null, portEnv, dataEnv: null, host, ownHost: false, secondHost: false, custom: [], secrets, secretsOn: declared === true || secrets.length > 0, telemetry: false, logs: telemetry === 'logs', metrics: null, postgres: false, bucket: null, allSecrets: declared === true };
}

// k8s/platform/ for an app on loam: the Deployment with no replicas that says what its sandbox is.
function renderLoam({ name, image, port, health, portEnv, host, secrets, secretsOn, allSecrets, logs, pullSecret = 'ghcr' }) {
  const files = {};
  files[`${OUT}/deployment.yaml`] = HEADER('the app, on loam') + `# ${name} runs on loam, at https://${host}, not here: this Deployment has no replicas. It is the
# record home-cloud's loam provisioner (apps/apps-platform) reads: the pinned image, the port, the
# health path and the secrets, from which it makes the app's sandbox in loam project ${LOAM_PROJECT} and runs
# the image's program in it as a service, replacing the sandbox whenever the image changes.
# Promoting the app to the cluster is changing app.yaml's runtime to k8s, in a pull request a person merges.
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ${name}
  labels:
    app.kubernetes.io/name: ${name}
    platform.arugula.io/runtime: loam
  annotations:
    platform.arugula.io/loam-port: "${port}"
    platform.arugula.io/loam-port-env: ${portEnv}
    platform.arugula.io/loam-health: ${health}
${secretsOn ? `    # Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}\`, given to the sandbox as env when it is made.
    platform.arugula.io/loam-secrets: "${allSecrets ? '*' : secrets.join(',')}"
` : ''}spec:
  replicas: 0
  selector:
    matchLabels:
      app.kubernetes.io/name: ${name}
  template:
    metadata:
      labels:
        app.kubernetes.io/name: ${name}
    spec:
${pullSecret ? `      imagePullSecrets:
        - name: ${pullSecret}
` : ''}      automountServiceAccountToken: false
      securityContext:
        runAsNonRoot: true
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: ${name}
          image: ${image}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: [ALL]
`;
  if (logs) files[`${OUT}/dashboard.yaml`] = HEADER('needs.telemetry') + `# A Grafana dashboard for ${name}, "${name} (${NAMESPACE})": its service's log, which home-cloud's loam log
# shipper (apps/apps-platform) sends to Loki as {namespace="${NAMESPACE}", app="${name}", runtime="loam"}, the
# labels the cluster gives its pods' logs, so the panel keeps its history if the app is promoted.
# Grafana loads every ConfigMap labelled grafana_dashboard: "1".
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-dashboard
  labels:
    app.kubernetes.io/name: ${name}
    grafana_dashboard: "1"
data:
  ${name}.json: |
${JSON.stringify(dashboard(name, { runtime: 'loam', url: `https://${host}` }), null, 2).replace(/^/gm, '    ')}
`;
  files[`${OUT}/kustomization.yaml`] = HEADER('the list of what it renders') + `apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - deployment.yaml
${logs ? '  - dashboard.yaml\n' : ''}`;
  return files;
}

const HEADER = (what) => `# Generated from app.yaml by arugula-salad/planter's platform-app (${what}). Don't edit: the next
# build of main renders it again. Change app.yaml, or patch it from k8s/kustomization.yaml.
`;

/**
 * The files of k8s/platform/ for a spec from validate(), as { path: content }, paths relative to the
 * repo's root. k8s/platform/kustomization.yaml lists the rest; the app's own k8s/kustomization.yaml
 * includes the directory and pins the image (topKustomization() writes its first version).
 */
export function render(spec) {
  if (spec.runtime === 'loam') return renderLoam(spec);
  const { name, image, port, health, volume, portEnv, dataEnv, host, ownHost = true, secondHost = false, custom = [], secrets, secretsOn, telemetry, metrics, postgres, bucket = null, conversations = null, analytics = false, pullSecret = 'ghcr' } = spec;
  const ownPullSecret = pullSecret === `${name}-registry`;
  const named = secrets.length ? ` (${secrets.join(', ')})` : '';
  const pvc = volume != null;
  const labels = (indent) => `${indent}app.kubernetes.io/name: ${name}`;
  const files = {};
  const resources = [];
  const add = (file, what, body) => { files[`${OUT}/${file}`] = HEADER(what) + body; resources.push(file); };

  const envLines = [
    `            - { name: ${portEnv}, value: "${port}" }`,
    '            - { name: HOST, value: 0.0.0.0 }',
    ...(telemetry ? [
      '            # Where an OpenTelemetry SDK sends traces (Tempo), and what it calls the app. An app',
      '            # without one ignores them.',
      `            - { name: OTEL_EXPORTER_OTLP_ENDPOINT, value: "${OTLP}" }`,
      '            - { name: OTEL_EXPORTER_OTLP_PROTOCOL, value: http/protobuf }',
      `            - { name: OTEL_SERVICE_NAME, value: ${name} }`,
      `            - { name: OTEL_RESOURCE_ATTRIBUTES, value: "service.namespace=${NAMESPACE},deployment.environment=prod" }`,
    ] : []),
    ...(pvc ? ['            # Where the app keeps its data (SQLite, uploads): the volume.', `            - { name: ${dataEnv}, value: /data }`] : []),
  ];

  add('deployment.yaml', 'the app', `apiVersion: apps/v1
kind: Deployment
metadata:
  name: ${name}
  labels:
${labels('    ')}
${secretsOn || postgres || bucket || conversations || analytics ? `  annotations:
    # Restart the pods when an Infisical value changes (the app reads env at startup).
    secrets.infisical.com/auto-reload: "true"
` : ''}spec:
${pvc ? `  # One replica, replaced rather than rolled: the volume is ReadWriteOnce.
  replicas: 1
  strategy:
    type: Recreate` : `  # Stateless: two replicas, rolled.
  replicas: 2`}
  selector:
    matchLabels:
${labels('      ')}
  template:
    metadata:
      labels:
${labels('        ')}
    spec:
${pullSecret ? `      imagePullSecrets:
        - name: ${pullSecret}
` : ''}      automountServiceAccountToken: false
      # The image runs as the numeric user 1000 (the Dockerfile's USER 1000:1000); a root or named
      # user fails runAsNonRoot with CreateContainerConfigError.
      securityContext:
        runAsNonRoot: true
${pvc ? `        # Makes the volume group-writable by gid 1000.
        fsGroup: 1000
` : ''}        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: ${name}
          image: ${image}
          ports:
            - name: http
              containerPort: ${port}
          env:
${envLines.join('\n')}
${secretsOn ? `          # Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}\`${named}, as the Secret ${name}-secrets.
` : ''}${secretsOn || postgres || bucket || conversations || analytics ? `          envFrom:
` : ''}${secretsOn ? `            - secretRef:
                name: ${name}-secrets
` : ''}${postgres ? `            # The app's database on apps-pg: DATABASE_URL, PGHOST, PGPORT, PGDATABASE, PGUSER, PGPASSWORD, PGSSLMODE.
            - secretRef:
                name: ${name}-postgres
` : ''}${bucket ? `            # The app's bucket on Garage: S3_ENDPOINT, S3_BUCKET, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, ...
            - secretRef:
                name: ${name}-bucket
` : ''}${conversations ? `            # The app's agents: CONVERSATIONS_URL, CONVERSATIONS_TOKEN, CONVERSATIONS_AGENT_<NAME>.
            - secretRef:
                name: ${name}-conversations
` : ''}${analytics ? `            # The app's analytics: ANALYTICS_CLIENT_ID, ANALYTICS_CLIENT_SECRET, ANALYTICS_URL, ANALYTICS_API_URL.
            - secretRef:
                name: ${name}-analytics
` : ''}          readinessProbe:
            httpGet: { path: ${health}, port: http }
          livenessProbe:
            httpGet: { path: ${health}, port: http }
            initialDelaySeconds: 10
          resources:
            requests: { cpu: 10m, memory: 32Mi }
            limits: { memory: 256Mi }
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: [ALL]
          volumeMounts:
            - name: tmp
              mountPath: /tmp
${pvc ? `            - name: data
              mountPath: /data
` : ''}      volumes:
        - name: tmp
          emptyDir:
            sizeLimit: 256Mi
${pvc ? `        - name: data
          persistentVolumeClaim:
            claimName: ${name}-data
` : ''}`);

  add('service.yaml', 'the app\'s Service', `apiVersion: v1
kind: Service
metadata:
  name: ${name}
  labels:
${labels('    ')}
spec:
  selector:
${labels('    ')}
  ports:
    - name: http
      port: 80
      targetPort: http
`);

  const own = `${name}.${DOMAIN}`;
  const second = `${name}.${SECOND_DOMAIN}`;
  const redirect = (route, hosts) => `apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: ${route}
  labels:
${labels('    ')}
spec:
  entryPoints:
    - web
  routes:
${hosts.map((h) => `    - match: Host(\`${h}\`)
      kind: Rule
      middlewares:
        # home-cloud's shared redirect middleware.
        - name: redirect-https
          namespace: default
      services:
        - name: ${name}
          port: 80`).join('\n')}
`;
  const route = (routeName, hosts, secret) => `apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: ${routeName}
  labels:
${labels('    ')}
spec:
  entryPoints:
    - websecure
  routes:
${hosts.map((h) => `    - match: Host(\`${h}\`)
      kind: Rule
      services:
        - name: ${name}
          port: 80`).join('\n')}
  tls:
    secretName: ${secret}
`;
  const routes = [];
  if (ownHost) routes.push(`# https://${own}, with port 80 redirected. *.${DOMAIN} resolves to the cluster, so there is no
# DNS step; cert-manager issues the certificate once the route exists.
apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: ${name}
  labels:
${labels('    ')}
spec:
  entryPoints:
    - websecure
  routes:
    - match: Host(\`${own}\`)
      kind: Rule
      services:
        - name: ${name}
          port: 80
  tls:
    secretName: ${name}-tls
---
apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: ${name}-http
  labels:
${labels('    ')}
spec:
  entryPoints:
    - web
  routes:
    - match: Host(\`${own}\`)
      kind: Rule
      middlewares:
        # home-cloud's shared redirect middleware.
        - name: redirect-https
          namespace: default
      services:
        - name: ${name}
          port: 80
---
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: ${name}-tls
  labels:
${labels('    ')}
spec:
  secretName: ${name}-tls
  issuerRef:
    name: letsencrypt-production
    kind: ClusterIssuer
  dnsNames:
    - ${own}
`);
  if (secondHost) routes.push(`# https://${second}, with port 80 redirected, on the fleet's certificate for *.${SECOND_DOMAIN}
# (${SECOND_DOMAIN_TLS}). *.inevitable.fyi resolves to the cluster, so there is no DNS step either.
${route(`${name}-inevitable`, [second], SECOND_DOMAIN_TLS)}---
${redirect(`${name}-inevitable-http`, [second])}`);
  if (custom.length) routes.push(`# ${custom.map((h) => `https://${h}`).join(', ')}: custom domains, with port 80 redirected. home-cloud admits
# these routes only once it has granted each host to ${name}: the app's registration in
# arugula-salad/planter-apps asks for it (planter writes the annotation platform.arugula.io/domains at Go
# live), its A record (or CNAME) points at ${ADDRESS}, and the pull request home-cloud's domain
# provisioner opens for it is merged. Then cert-manager proves it over HTTP-01.
${route(`${name}-custom`, custom, `${name}-custom-tls`)}---
${redirect(`${name}-custom-http`, custom)}---
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: ${name}-custom-tls
  labels:
${labels('    ')}
spec:
  secretName: ${name}-custom-tls
  issuerRef:
    name: letsencrypt-http01
    kind: ClusterIssuer
  dnsNames:
${custom.map((h) => `    - ${h}`).join('\n')}
`);
  add('ingressroute.yaml', `https://${host}`, routes.join('---\n'));

  if (secretsOn) add('infisicalsecret.yaml', 'needs.secrets', `# The app's runtime secrets${named}: Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}\`,
# materialized as the Secret ${name}-secrets. Every secret in the folder is loaded, listed in app.yaml
# or not. Every app in ${NAMESPACE} can read every folder of the project: anything that must not be
# shared gets a project of its own.
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-secrets
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}"
        recursive: false
  managedSecretReference:
    secretName: ${name}-secrets
    secretNamespace: ${NAMESPACE}
    creationPolicy: Orphan
`);

  if (ownPullSecret) add('registry.yaml', `its private image, ${image}`, `# The login that pulls ${image}: a docker config of a read:package Forgejo token, kept in Infisical
# \`${SECRETS_PROJECT}\` / prod / \`/${name}/registry\` (key FORGEJO_DOCKERCONFIGJSON, a copy of project
# forgejo's /cluster-pull, written there when the app is onboarded) and materialized as the Secret
# ${name}-registry. home-cloud's admission policy lets only this app's pods name it. Its own folder,
# so a change to it doesn't restart the app through ${name}-secrets.
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-registry
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}/registry"
        recursive: false
  managedKubeSecretReferences:
    - secretName: ${name}-registry
      secretNamespace: ${NAMESPACE}
      secretType: kubernetes.io/dockerconfigjson
      creationPolicy: Orphan
      template:
        includeAllSecrets: false
        data:
          .dockerconfigjson: "{{ .FORGEJO_DOCKERCONFIGJSON.Value }}"
`);

  if (postgres) add('postgres.yaml', 'needs.postgres', `# A database and a role of its own, both called ${name}, on home-cloud's shared Postgres apps-pg.
# The ConfigMap asks for them: home-cloud's provisioner sees it, makes the role, the database and a
# password (Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}/postgres\`, which nobody reads but this
# and apps-pg), and the InfisicalSecret turns the password into ${name}-postgres. A first deploy
# waits for that, a minute or two, with its pods in CreateContainerConfigError. The role reaches only
# its own database, over TLS. A new pod's first few seconds of connections can be refused while the
# network policy learns its address, so connect with a retry (or let the pod restart). Removing the
# app keeps the database.
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-postgres-claim
  labels:
${labels('    ')}
    platform.arugula.io/postgres: "true"
data:
  database: ${name}
---
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-postgres
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}/postgres"
        recursive: false
  managedSecretReference:
    secretName: ${name}-postgres
    secretNamespace: ${NAMESPACE}
    creationPolicy: Orphan
    template:
      includeAllSecrets: false
      data:
        DATABASE_URL: "postgresql://${name}:{{ .PASSWORD.Value }}@${POSTGRES_HOST}:5432/${name}?sslmode=require"
        PGHOST: ${POSTGRES_HOST}
        PGPORT: "5432"
        PGDATABASE: ${name}
        PGUSER: ${name}
        PGPASSWORD: "{{ .PASSWORD.Value }}"
        PGSSLMODE: require
`);

  if (bucket) add('bucket.yaml', 'needs.bucket', `# A Garage bucket of its own, pa-${name}-${bucket} (5 GiB at most), and a key that reaches only it.
# The ConfigMap asks for them: home-cloud's bucket provisioner (apps/apps-platform) sees it, makes
# both, and writes the key to Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}/bucket\`, which the
# InfisicalSecret turns into ${name}-bucket. A first deploy waits for that, a minute or two, with its
# pods in CreateContainerConfigError. Use path-style requests (S3_FORCE_PATH_STYLE). Removing the app
# keeps the bucket and what's in it.
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-bucket-claim
  labels:
${labels('    ')}
    platform.arugula.io/bucket: "true"
data:
  bucket: ${bucket}
---
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-bucket
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}/bucket"
        recursive: false
  managedSecretReference:
    secretName: ${name}-bucket
    secretNamespace: ${NAMESPACE}
    creationPolicy: Orphan
    template:
      includeAllSecrets: false
      data:
        AWS_ACCESS_KEY_ID: "{{ .AWS_ACCESS_KEY_ID.Value }}"
        AWS_SECRET_ACCESS_KEY: "{{ .AWS_SECRET_ACCESS_KEY.Value }}"
        AWS_REGION: home-cloud
        AWS_ENDPOINT_URL_S3: ${S3_ENDPOINT}
        S3_ENDPOINT: ${S3_ENDPOINT}
        S3_BUCKET: pa-${name}-${bucket}
        S3_FORCE_PATH_STYLE: "true"
`);

  if (conversations) {
    const claim = JSON.stringify({ agents: conversations.agents }, null, 2);
    add('conversations.yaml', 'needs.conversations', `# The app's agents (${conversations.agents.map((a) => a.name).join(', ')}), in an account of its own on the platform's
# conversation service. The ConfigMap asks for them: home-cloud's conversations provisioner
# (apps/apps-platform) sees it, opens the account, makes each agent on loam, and writes its key and the agents' ids to Infisical \`${SECRETS_PROJECT}\` / prod /
# \`/${name}/conversations\`, which the InfisicalSecret turns into ${name}-conversations. A first deploy
# waits for that, a minute or two, with its pods in CreateContainerConfigError. Start each
# conversation with "sandbox_api_access": "none" unless its agent must call the service itself.
# Removing the app deletes the agents made for it and keeps the account and its conversations.
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-conversations-claim
  labels:
${labels('    ')}
    platform.arugula.io/conversations: "true"
data:
  claim.json: |
${claim.split('\n').map((l) => `    ${l}`).join('\n')}
---
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-conversations
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}/conversations"
        recursive: false
  managedSecretReference:
    secretName: ${name}-conversations
    secretNamespace: ${NAMESPACE}
    creationPolicy: Orphan
    template:
      includeAllSecrets: false
      data:
        CONVERSATIONS_URL: ${CONVERSATIONS_URL}
        CONVERSATIONS_TOKEN: "{{ .TOKEN.Value }}"
${conversations.agents.map((a) => `        ${agentEnv(a.name)}: "{{ .AGENT_${a.name.toUpperCase().replace(/-/g, '_')}.Value }}"`).join('\n')}
`);
  }

  if (analytics) {
    const second = `${name}.${SECOND_DOMAIN}`;
    const origins = [...(ownHost ? [`${name}.${DOMAIN}`] : []), ...(secondHost ? [second] : []), ...custom].map((h) => `https://${h}`);
    add('analytics.yaml', 'needs.analytics', `# Product analytics: a project of the app's own on the platform's OpenPanel, and a client for it.
# The ConfigMap asks for them: home-cloud's analytics provisioner (apps/apps-platform) sees it, makes
# the project (browsers may send events only from the origins below, the app's hosts), and writes the
# client to Infisical \`${SECRETS_PROJECT}\` / prod / \`/${name}/analytics\`, which the InfisicalSecret
# turns into ${name}-analytics. A first deploy waits for that, a minute or two, with its pods in
# CreateContainerConfigError. Send events with OpenPanel's SDKs: from the browser with
# ANALYTICS_CLIENT_ID to ANALYTICS_URL, from the server with the secret as well to ANALYTICS_API_URL.
# Removing the app keeps the project and its events.
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-analytics-claim
  labels:
${labels('    ')}
    platform.arugula.io/analytics: "true"
data:
  origins: '${JSON.stringify(origins)}'
---
apiVersion: secrets.infisical.com/v1alpha1
kind: InfisicalSecret
metadata:
  name: ${name}-analytics
  labels:
${labels('    ')}
spec:
  hostAPI: http://infisical.infisical.svc.cluster.local:8080
  resyncInterval: 60
  authentication:
    kubernetesAuth:
      identityId: 51ff5eda-4369-4107-80bc-d11d342763fd
      autoCreateServiceAccountToken: true
      serviceAccountRef:
        name: planter-apps-secrets
        namespace: ${NAMESPACE}
      secretsScope:
        projectSlug: ${SECRETS_PROJECT}
        envSlug: prod
        secretsPath: "/${name}/analytics"
        recursive: false
  managedSecretReference:
    secretName: ${name}-analytics
    secretNamespace: ${NAMESPACE}
    creationPolicy: Orphan
    template:
      includeAllSecrets: false
      data:
        ANALYTICS_CLIENT_ID: "{{ .CLIENT_ID.Value }}"
        ANALYTICS_CLIENT_SECRET: "{{ .CLIENT_SECRET.Value }}"
        ANALYTICS_URL: ${ANALYTICS_URL}
        ANALYTICS_API_URL: ${ANALYTICS_API_URL}
`);
  }

  if (pvc) add('pvc.yaml', 'volume', `# A Longhorn volume for the app's data, mounted at /data (${dataEnv}), replicated across nodes.
# Removing the app from the cluster deletes it and its data; there is no backup unless you set one up.
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: ${name}-data
  labels:
${labels('    ')}
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: longhorn
  resources:
    requests:
      storage: ${volume}
`);

  if (metrics) add('servicemonitor.yaml', 'needs.telemetry.metrics', `# Prometheus scrapes the app's ${metrics} on its Service's http port every 30s. Only Services in
# ${NAMESPACE}, and no honorLabels: home-cloud's policy refuses anything else.
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: ${name}
  labels:
${labels('    ')}
spec:
  selector:
    matchLabels:
${labels('      ')}
  endpoints:
    - port: http
      path: ${metrics}
      interval: 30s
`);

  if (telemetry) add('dashboard.yaml', 'needs.telemetry', `# A Grafana dashboard for ${name}, "${name} (${NAMESPACE})": requests, errors and latency as Traefik
# sees them, the pods' memory and CPU, their logs, and the app's traces (if it sends any; see the
# OTEL_ variables in deployment.yaml). Grafana loads every ConfigMap labelled grafana_dashboard: "1".
apiVersion: v1
kind: ConfigMap
metadata:
  name: ${name}-dashboard
  labels:
${labels('    ')}
    grafana_dashboard: "1"
data:
  ${name}.json: |
${JSON.stringify(dashboard(name), null, 2).replace(/^/gm, '    ')}
`);

  files[`${OUT}/kustomization.yaml`] = HEADER('the list of what it renders') + `apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
${resources.map((r) => `  - ${r}`).join('\n')}
`;
  return files;
}

/**
 * The app's own k8s/kustomization.yaml, as its first version: the namespace, k8s/platform/, and the
 * image pin the build rewrites. Nothing regenerates it: patches to what platform/ renders go here.
 */
export function topKustomization({ name, image, host }, tag = 'unpinned') {
  return `# https://${host}: ${name} on home-cloud, applied by Flux from this directory. platform/ is rendered
# from app.yaml on every build of main (don't edit it); this file is yours: add kustomize patches here
# for anything app.yaml can't say. Every app shares the \`${NAMESPACE}\` namespace, which the platform
# owns along with the \`ghcr\` pull secret and the \`planter-apps-secrets\` ServiceAccount.
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: ${NAMESPACE}
resources:
  - platform
images:
  - name: ${image}
    # Pinned by .github/workflows/build.yml on every build of main. \`unpinned\` is a placeholder
    # that does not exist: the app does not run until CI has replaced it.
    newTag: ${tag}
`;
}

/** app.yaml's text for a spec's choices: what planter writes for a new app. */
export function manifestText({ name, runtime = 'k8s', domains = null, port = PORT, health = '/health', volume = null, portEnv = 'PORT', dataEnv = 'APP_DATA', secrets = false, metrics = null, postgres = false }) {
  const lines = [
    `# What ${name} needs from the platform. Its build renders k8s/platform/ from this on every build of`,
    '# main (arugula-salad/planter, platform-app/render.mjs, says what each key gives).',
    `name: ${name}`,
  ];
  if (port !== PORT) lines.push(`port: ${port}`);
  lines.push(`health: ${health}`);
  if (runtime === 'loam') {
    lines.push(
      `# On loam, at https://${name}.${LOAM_DOMAIN}: asleep until a request wakes it. Moving it into the`,
      '# cluster is changing this to k8s (with its domain), in a pull request a person merges.',
      'runtime: loam',
    );
    if (portEnv !== 'PORT') lines.push(`env: { port: ${portEnv} }`);
    lines.push('needs:', `  domain: [${name}.${LOAM_DOMAIN}]`);
    if (secrets === true) lines.push(`  secrets: true            # Infisical ${SECRETS_PROJECT} / prod / /${name}; or list the names kept there`);
    else if (secrets.length) lines.push(`  secrets: [${secrets.join(', ')}]`);
    lines.push('  telemetry: logs');
    return `${lines.join('\n')}\n`;
  }
  lines.push('runtime: k8s');
  if (volume) lines.push(`volume: ${volume}`);
  if (portEnv !== 'PORT' || dataEnv !== 'APP_DATA') lines.push(`env: { port: ${portEnv}, data: ${dataEnv} }`);
  lines.push('needs:', `  domain: [${(domains ?? [`${name}.${DOMAIN}`]).join(', ')}]`);
  if (secrets === true) lines.push(`  secrets: true            # Infisical ${SECRETS_PROJECT} / prod / /${name}; or list the names kept there`);
  else if (secrets.length) lines.push(`  secrets: [${secrets.join(', ')}]`);
  lines.push(`  telemetry: ${metrics ? `{ metrics: ${metrics} }` : 'true'}`);
  if (postgres) lines.push(
    '  # A database of its own, as DATABASE_URL, on apps-pg, the Postgres every app shares (one going down',
    '  # takes them all). A new pod\'s first connections can be refused for a few seconds: connect with a retry.',
    '  postgres: {}',
  );
  return `${lines.join('\n')}\n`;
}

/**
 * The Grafana dashboard of an app in NAMESPACE, as an object. On loam (`runtime: 'loam'`) it is the
 * logs alone: there are no pods, and loam's requests don't pass Traefik's services. Logs are matched by
 * app, which the cluster's pods and the loam log shipper both label, so an app's log reads on across a
 * promotion.
 */
export function dashboard(name, { runtime = 'k8s', url = `https://${name}.${DOMAIN}` } = {}) {
  const logs = (id, gridPos) => ({ id, title: 'Logs', type: 'logs', gridPos, datasource: { type: 'loki', uid: 'loki' },
    targets: [{ datasource: { type: 'loki', uid: 'loki' }, expr: `{namespace="${NAMESPACE}", app="${name}"}`, refId: 'A' }] });
  if (runtime === 'loam') {
    return {
      uid: `planter-${name}`.slice(0, 40),
      title: `${name} (${NAMESPACE})`,
      tags: [NAMESPACE, 'loam'],
      timezone: 'browser',
      schemaVersion: 39,
      time: { from: 'now-6h', to: 'now' },
      links: [{ title: url, url, type: 'link', targetBlank: true }],
      panels: [logs(1, { x: 0, y: 0, w: 24, h: 20 })],
    };
  }
  const route = `service=~"${NAMESPACE}-${name}-[0-9a-f]+@kubernetescrd"`;
  const pods = `namespace="${NAMESPACE}", pod=~"${name}-[a-z0-9]+-[a-z0-9]{5}"`;
  const prom = { type: 'prometheus', uid: 'prometheus' };
  const panel = (id, title, type, gridPos, targets, extra = {}) => ({ id, title, type, gridPos, datasource: targets[0].datasource, targets, ...extra });
  const q = (expr, legendFormat, refId = 'A') => ({ datasource: prom, expr, legendFormat, refId });
  return {
    uid: `planter-${name}`.slice(0, 40),
    title: `${name} (${NAMESPACE})`,
    tags: [NAMESPACE],
    timezone: 'browser',
    schemaVersion: 39,
    time: { from: 'now-6h', to: 'now' },
    links: [{ title: url, url, type: 'link', targetBlank: true }],
    panels: [
      panel(1, 'Requests by status', 'timeseries', { x: 0, y: 0, w: 12, h: 8 },
        [q(`sum by (code) (rate(traefik_service_requests_total{${route}}[$__rate_interval]))`, '{{code}}')],
        { fieldConfig: { defaults: { unit: 'reqps' }, overrides: [] } }),
      panel(2, 'Server errors', 'timeseries', { x: 12, y: 0, w: 6, h: 8 },
        [q(`sum(rate(traefik_service_requests_total{${route}, code=~"5.."}[$__rate_interval])) / sum(rate(traefik_service_requests_total{${route}}[$__rate_interval]))`, '5xx')],
        { fieldConfig: { defaults: { unit: 'percentunit', min: 0 }, overrides: [] } }),
      panel(3, 'Latency', 'timeseries', { x: 18, y: 0, w: 6, h: 8 },
        [0.5, 0.95].map((p, i) => q(`histogram_quantile(${p}, sum by (le) (rate(traefik_service_request_duration_seconds_bucket{${route}}[$__rate_interval])))`, `p${p * 100}`, 'AB'[i])),
        { fieldConfig: { defaults: { unit: 's' }, overrides: [] } }),
      panel(4, 'Memory', 'timeseries', { x: 0, y: 8, w: 12, h: 7 },
        [q(`sum by (pod) (container_memory_working_set_bytes{${pods}, container="${name}"})`, '{{pod}}')],
        { fieldConfig: { defaults: { unit: 'bytes' }, overrides: [] } }),
      panel(5, 'CPU', 'timeseries', { x: 12, y: 8, w: 12, h: 7 },
        [q(`sum by (pod) (rate(container_cpu_usage_seconds_total{${pods}, container="${name}"}[$__rate_interval]))`, '{{pod}}')],
        { fieldConfig: { defaults: { unit: 'short' }, overrides: [] } }),
      logs(6, { x: 0, y: 15, w: 24, h: 10 }),
      panel(7, 'Traces', 'table', { x: 0, y: 25, w: 24, h: 8 },
        [{ datasource: { type: 'tempo', uid: 'tempo' }, queryType: 'traceql', query: `{resource.service.name="${name}"}`, limit: 20, refId: 'A' }]),
    ],
  };
}
