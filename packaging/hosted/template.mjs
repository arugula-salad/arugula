// Builds the hosted workspace's E2B template (M177) from a context
// directory scripts/ci-hosted-template made: Dockerfile, ready, arugulad.json,
// arugulad and arugula. E2B starts `arugulad --hosted` at the end of the
// build and snapshots the box once the ready check passes, so a box made
// from it has the daemon running already.
//
//   node template.mjs CONTEXT NAME TAG [--start CMD] [--dry-run]
//
// It prints {name, tag, template, templateId, buildId} as JSON on stdout;
// `template` is what a box is created from (NAME:TAG). --dry-run prints the
// steps E2B would be sent instead, and needs no key. E2B_API_KEY is the
// key (E2B_DOMAIN another E2B, as the SDK reads it).
import path from 'node:path'
import { Template } from 'e2b'

// The hosted daemon, waiting for its sandbox id to change (M180), and the
// check that it's up. --start replaces the first for a test build with a
// daemon older than --hosted.
const START = 'arugulad --hosted'
const READY = 'arugula-hosted-ready'
// A box's size: the template's, as E2B gives a box its template's.
const CPUS = 2
const MEMORY_MB = 2048

const args = process.argv.slice(2)
const flag = (name) => {
  const i = args.indexOf(name)
  if (i < 0) return undefined
  const [, value] = args.splice(i, 2)
  return value
}
const dryRun = args.includes('--dry-run')
if (dryRun) args.splice(args.indexOf('--dry-run'), 1)
const start = flag('--start') ?? START
const [context, name, tag] = args
if (!context || !name || !tag) {
  console.error('usage: node template.mjs CONTEXT NAME TAG [--start CMD] [--dry-run]')
  process.exit(2)
}

const ctx = path.resolve(context)
const template = Template({ fileContextPath: ctx })
  .fromDockerfile(path.join(ctx, 'Dockerfile'))
  .setStartCmd(start, READY)

if (dryRun) {
  console.log(await Template.toJSON(template, false))
  process.exit(0)
}
if (!process.env.E2B_API_KEY) {
  console.error('E2B_API_KEY is not set')
  process.exit(1)
}
const info = await Template.build(template, name, {
  tags: [tag],
  cpuCount: CPUS,
  memoryMB: MEMORY_MB,
  // The build's log on stderr; stdout is the answer.
  onBuildLogs: (entry) => process.stderr.write(`${entry.toString()}\n`),
})
console.log(JSON.stringify({ name, tag, template: `${name}:${tag}`, templateId: info.templateId, buildId: info.buildId }))
