// Builds the hosted workspace's E2B template (M177) from a context
// directory scripts/ci-hosted-template made: Dockerfile, ready, start,
// arugulad.service, arugulad.json, arugulad and arugula. E2B runs the start
// command at the end of the build and snapshots the box once the ready
// check passes, so a box made from it has the daemon running already.
//
//   node template.mjs CONTEXT NAME TAG [--cpus N] [--memory-mb M] [--start CMD] [--dry-run]
//
// It prints {name, tag, template, templateId, buildId} as JSON on stdout;
// `template` is what a box is created from (NAME:TAG). --cpus and
// --memory-mb are the size of every box made from it (2 and 2048 unless
// given): E2B gives a box its template's, so each size is a template of its
// own (M266), which scripts/ci-hosted-template builds. --dry-run prints
// {name, tag, cpuCount, memoryMB, steps}, what E2B would be sent, instead,
// and needs no key. E2B_API_KEY is the key (E2B_DOMAIN another E2B, as the
// SDK reads it).
import path from 'node:path'
import { Template } from 'e2b'

// The hosted daemon, waiting for its sandbox id to change (M180), under its
// supervisor (`start`, M258: E2B runs this once, so it must be what keeps
// the daemon running), and the check that it's up. --start replaces the
// first for a test build with a daemon older than --hosted.
const START = 'arugula-hosted-start'
const READY = 'arugula-hosted-ready'

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
const count = (name, otherwise) => {
  const v = flag(name)
  if (v === undefined) return otherwise
  if (!/^[1-9][0-9]*$/.test(v)) {
    console.error(`${name} takes a whole number, not ${v}`)
    process.exit(2)
  }
  return Number(v)
}
// A box's size: the template's, as E2B gives a box its template's.
const cpuCount = count('--cpus', 2)
const memoryMB = count('--memory-mb', 2048)
const [context, name, tag] = args
if (!context || !name || !tag) {
  console.error('usage: node template.mjs CONTEXT NAME TAG [--cpus N] [--memory-mb M] [--start CMD] [--dry-run]')
  process.exit(2)
}

const ctx = path.resolve(context)
const template = Template({ fileContextPath: ctx })
  .fromDockerfile(path.join(ctx, 'Dockerfile'))
  .setStartCmd(start, READY)

if (dryRun) {
  const steps = JSON.parse(await Template.toJSON(template, false))
  console.log(JSON.stringify({ name, tag, cpuCount, memoryMB, steps }))
  process.exit(0)
}
if (!process.env.E2B_API_KEY) {
  console.error('E2B_API_KEY is not set')
  process.exit(1)
}
const info = await Template.build(template, name, {
  tags: [tag],
  cpuCount,
  memoryMB,
  // The build's log on stderr; stdout is the answer.
  onBuildLogs: (entry) => process.stderr.write(`${entry.toString()}\n`),
})
console.log(JSON.stringify({ name, tag, template: `${name}:${tag}`, templateId: info.templateId, buildId: info.buildId }))
