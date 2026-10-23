// Arugula's planter renderer (M233): what a deploy's box asks of planter's own code, vendored beside
// this file (platform-app/render.mjs and prod-scaffold.mjs, unchanged), so the platform renders an app
// exactly as planter did. `arugulad _planter` writes the three files into the run's own directory and
// runs `node renderer.mjs`, one request as JSON on standard input, one answer as JSON on standard
// output: `{ ... }`, or `{ "error": "..." }` (exit 1) when planter's code refused it.
//
//   { op: "render", manifest, repo? }              app.yaml (parsed) to its spec, k8s/platform/ and a
//                                                  first k8s/kustomization.yaml
//   { op: "hosts", name, domains }                 the hosts app.yaml's needs.domain asks for, as
//                                                  validate() sorts them (own, second, custom)
//   { op: "ship", name, port, health, portEnv, secrets }
//                                                  what planter's ship writes (ship.mjs ship()): app.yaml
//                                                  for runtime loam, its render and kustomization
//   { op: "promote", name, old: { port, health, portEnv, secrets, secretsOn } }
//                                                  what planter's promote writes (ship.mjs promote())
//   { op: "scaffold", name, app: { dir, fields }, packageJson, lockfile, options }
//                                                  what planter's Prod writes (prod.mjs scaffold()):
//                                                  readAppFacts over the files given, then prodScaffold
//
// It has no dependencies, as render.mjs has none: planter parses YAML with its `yaml` package, and here
// the YAML planter writes itself (manifestText) is turned back into the object it parses to by
// `manifestOf`, which the fixtures (planter's own output, captured) hold to planter's parse.

import { manifestText, render, SECOND_DOMAIN, topKustomization, validate } from './platform-app/render.mjs';
import { prodScaffold, readAppFacts } from './prod-scaffold.mjs';

const PORT = 8080;

/**
 * The object `parse(manifestText(o))` is, for the choices planter's ship and promote pass it: the same
 * keys, in the same order, with the same values (YAML's `true`, numbers and flow lists), comments aside.
 */
export function manifestOf({ name, runtime = 'k8s', domains = null, port = PORT, health = '/health', volume = null, portEnv = 'PORT', dataEnv = 'APP_DATA', secrets = false, metrics = null, postgres = false }) {
  const m = { name };
  if (port !== PORT) m.port = port;
  m.health = health;
  if (runtime === 'loam') {
    m.runtime = 'loam';
    if (portEnv !== 'PORT') m.env = { port: portEnv };
    m.needs = { domain: [`${name}.run.inevitable.fyi`] };
    if (secrets === true) m.needs.secrets = true;
    else if (secrets.length) m.needs.secrets = [...secrets];
    m.needs.telemetry = 'logs';
    return m;
  }
  m.runtime = 'k8s';
  if (volume) m.volume = volume;
  if (portEnv !== 'PORT' || dataEnv !== 'APP_DATA') m.env = { port: portEnv, data: dataEnv };
  m.needs = { domain: [...(domains ?? [`${name}.apps.arugula.io`])] };
  if (secrets === true) m.needs.secrets = true;
  else if (secrets.length) m.needs.secrets = [...secrets];
  m.needs.telemetry = metrics ? { metrics } : true;
  if (postgres) m.needs.postgres = {};
  return m;
}

export async function answer(req) {
  switch (req?.op) {
    case 'render': {
      const spec = validate(req.manifest, req.repo != null ? { repo: req.repo } : {});
      return { spec, files: render(spec), top: topKustomization(spec) };
    }
    case 'hosts': {
      const spec = validate({ name: req.name, needs: { domain: req.domains } }, { repo: req.name });
      return { host: spec.host, ownHost: spec.ownHost, secondHost: spec.secondHost, custom: spec.custom };
    }
    case 'ship': {
      const o = { name: req.name, runtime: 'loam', port: req.port, health: req.health, portEnv: req.portEnv, secrets: req.secrets };
      const appYaml = manifestText(o);
      const spec = validate(manifestOf(o), { repo: req.name });
      return { appYaml, spec, files: render(spec), top: topKustomization(spec) };
    }
    case 'promote': {
      const old = req.old;
      const o = { name: req.name, runtime: 'k8s', domains: [`${req.name}.${SECOND_DOMAIN}`], port: old.port, health: old.health, portEnv: old.portEnv, secrets: old.secrets.length ? old.secrets : old.secretsOn };
      const appYaml = manifestText(o);
      const spec = validate(manifestOf(o), { repo: req.name });
      return { appYaml, spec, files: render(spec) };
    }
    case 'scaffold': {
      const files = { [`${req.app.dir.replace(/^\.\/|\/+$/g, '')}/package.json`]: req.packageJson };
      const read = (path) => (path in files ? files[path] : path.endsWith('/package-lock.json') && req.lockfile ? '{}' : null);
      const facts = await readAppFacts(read, { app: req.app });
      return { facts, files: prodScaffold({ ...facts, ...(req.options ?? {}), name: req.name }) };
    }
    default:
      throw new Error(`no op ${JSON.stringify(req?.op)}: render, hosts, ship, promote or scaffold`);
  }
}

const main = process.argv[1] && import.meta.url === (await import('node:url')).pathToFileURL(process.argv[1]).href;
if (main) {
  const chunks = [];
  for await (const c of process.stdin) chunks.push(c);
  try {
    process.stdout.write(`${JSON.stringify(await answer(JSON.parse(Buffer.concat(chunks).toString('utf8'))))}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ error: String(error?.message ?? error) })}\n`);
    process.exitCode = 1;
  }
}
