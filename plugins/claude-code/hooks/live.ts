// Live session signals (#396, #447, #453; aru-128): Claude Code tells
// Arugula about its session, from inside it, as things happen:
//
// - `usage`: the window's fill and size as the engine has them, after each
//   turn (`session.measure`), with the session's cost;
// - `step`: the model and effort of a model request, when either changes
//   (`turn.step`);
// - `compact`: a compaction once it ran, with its trigger and the tokens
//   before and after (`session.compact`);
// - `start`: a new session, with Claude Code's version.
//
// Opt in: the plugin's `live` option (plugin.json's userConfig), off by
// default. Off, register() adds no hook at all. The command hooks in
// hooks.json beside this module don't depend on it: a Claude Code without
// mods ignores `modules`, and one that can't load this module still runs
// them (#449).
//
// Each signal goes to the daemon through the installed CLI, `arugula
// session-signal`, with the signal on stdin (aru-039: the plugin calls the
// CLI you installed, which finds the daemon on every platform, Windows'
// named pipe included). Each signal carries Claude Code's session id, and
// the daemon keeps it by that (#453): the conversations list shows every
// session's figures, and a pane's strip shows the session running in that
// pane. So it reports from anywhere, a pane or none: VS Code, the desktop
// app, the editor block. The CLI adds the pane its environment names
// (ARUGULA_PANE), if any, and answers the pane it shows on, both versions
// and the session's context, which the status line shows. On Windows the
// engine runs `arugula` without a shell: `arugula.exe` must be on PATH.
//
// The mod API is early access. The engine checks every `on(...)` name when
// it loads the module and refuses the whole module for one it doesn't know,
// so these four names are pinned (a test in crates/cli/src/hooks.rs holds
// them): session.start, turn.step, session.measure (not the old
// session.usage) and session.compact. An event whose input or result comes
// in another shape is passed on untouched and named once on the status line.
// It hooks no `classic.*` event: a Team or Enterprise login's guard skips a
// user mod's classic hooks (#395), and none of this needs them. Every hook
// passes its event on, and a failure in one passes it on too.

import type { Register, EngineInterface } from 'claude-code'

// This module's own version, sent with every signal as `mod_version`. The
// plugin itself carries none (it follows main, aru-039); bump this when the
// signals change.
const MOD_VERSION = '0.3.0'

type Signal = {
  event: 'start' | 'usage' | 'step' | 'compact'
  agent_id?: string
  model?: string
  effort?: string
  tokens?: number
  window?: number
  trigger?: string
  tokens_before?: number
  tokens_after?: number
  cost_usd?: number
}

type Answer = {
  pane?: number | null
  session_id?: string | null
  cli_version?: string
  daemon_version?: string
  context?: { tokens?: number | null; window?: number; compactions?: number } | null
  error?: string
}

// The module's own state starts over on a reload, which is fine: the daemon
// keeps the context, and the next turn says it all again.
let session: string | undefined
let claudeVersion: string | undefined
let model: string | undefined
let effort: string | undefined
let queue: Promise<void> = Promise.resolve()
// Events that came in a shape this module doesn't know: named on the status
// line, so a Claude Code update that moved them says so instead of going
// quiet.
const changed = new Set<string>()

function tokensShort(n: number): string {
  return n >= 1_000_000 ? `${n / 1_000_000}M` : `${Math.round(n / 1000)}k`
}

// Where the figures show: the pane it runs in, else its conversation.
function where(a: Answer): string {
  if (typeof a.pane === 'number') return `arugula %${a.pane}`
  const id = a.session_id ?? session
  return id ? `arugula ${id.slice(0, 8)}` : 'arugula'
}

function show($: EngineInterface, a: Answer): void {
  if (a.error !== undefined) {
    $.ui.status(`${where(a)} ○ ${a.error.split('\n')[0]?.slice(0, 80)}`)
    return
  }
  const parts = [`${where(a)} ● live`]
  const c = a.context
  if (c && c.tokens && c.window) parts.push(`${Math.round((100 * c.tokens) / c.window)}% of ${tokensShort(c.window)}`)
  if (c && c.compactions) parts.push(`${c.compactions} compacted`)
  if (a.cli_version && a.daemon_version && a.cli_version !== a.daemon_version) {
    parts.push(`cli ${a.cli_version} ≠ daemon ${a.daemon_version}`)
  } else if (a.daemon_version) {
    parts.push(`v${a.daemon_version}`)
  }
  if (changed.size) parts.push(`${[...changed].join(', ')} changed in Claude Code ${claudeVersion ?? ''}`.trim())
  $.ui.status(parts.join(' · '))
}

// The CLI's answer to a signal, or why there's none. An `arugula` from
// before `session-signal` (older than this plugin) says so in clap's words.
function answerOf(r: { exitCode: number; stdout: string; stderr: string }): Answer {
  let answer: Answer = {}
  try {
    answer = (r.stdout.trim() ? JSON.parse(r.stdout) : {}) as Answer
  } catch {
    answer = {}
  }
  if (r.exitCode !== 0 && answer.error === undefined) {
    answer.error = /unrecognized subcommand|unexpected argument/.test(r.stderr)
      ? `live ${MOD_VERSION} needs a newer arugula CLI: update Arugula`
      : r.stderr.trim() || `exit ${r.exitCode}`
  }
  return answer
}

async function deliver($: EngineInterface, s: Signal): Promise<void> {
  let answer: Answer
  try {
    session = await $.session.id()
    const body = JSON.stringify({
      ...s,
      session_id: session,
      claude_version: claudeVersion,
      mod_version: MOD_VERSION,
    })
    answer = answerOf(await $.process.run(['arugula', 'session-signal'], { stdin: body, timeoutMs: 5000 }))
  } catch (err) {
    // No `arugula` on PATH, or it hung.
    answer = { error: err instanceof Error ? err.message : String(err) }
  }
  show($, answer)
}

// One at a time, in order: a compaction's signal never overtakes the usage
// before it.
function send($: EngineInterface, s: Signal): void {
  queue = queue.then(() => deliver($, s))
}

// Notes an event that came in a shape this module doesn't read, once, on
// the status line.
function shapeChanged($: EngineInterface, event: string): void {
  if (changed.has(event)) return
  changed.add(event)
  show($, {})
}

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

export const register: Register = (on, options) => {
  // Off unless the person turned it on (`/config`, or `claude plugin
  // configure arugula@arugula`). A value set from a shell arrives as text.
  if (options.live !== true && options.live !== 'true') return

  on('session.start', async ($, e, next) => {
    const res = await next(e)
    claudeVersion = (await $.session.version()).version
    send($, { event: 'start' })
    return res
  }).catch(($, e, next) => next(e))

  on('turn.step', async function* ($, e, next) {
    if (typeof e.model !== 'string') {
      shapeChanged($, 'turn.step')
    } else if (!e.agentId) {
      const ef = e.effort === undefined ? undefined : String(e.effort)
      if (e.model !== model || ef !== effort) {
        model = e.model
        effort = ef
        send($, { event: 'step', model, effort })
      }
    }
    return yield* next(e)
  }).catch(async function* ($, e, next) {
    return yield* next(e)
  })

  on('session.measure', async ($, e, next) => {
    const ctx = e.context as { tokens?: unknown; window?: unknown } | undefined
    if (!Array.isArray(e.changed) || !ctx || !isNum(ctx.tokens) || !isNum(ctx.window)) {
      shapeChanged($, 'session.measure')
    } else if (e.changed.includes('context') || e.changed.includes('cost')) {
      send($, {
        event: 'usage',
        tokens: ctx.tokens,
        window: ctx.window,
        cost_usd: isNum(e.cost?.usd) ? e.cost.usd : undefined,
        model,
        effort,
      })
    }
    return next(e)
  }).catch(($, e, next) => next(e))

  on('session.compact', async ($, e, next) => {
    const res = await next(e)
    if (e.trigger !== 'precompute' && res.skip === undefined) {
      if (!isNum(res.tokensBefore) || !isNum(res.tokensAfter)) {
        shapeChanged($, 'session.compact')
        return res
      }
      const { context } = await $.session.usage()
      send($, {
        event: 'compact',
        agent_id: e.agentId,
        trigger: e.trigger,
        tokens_before: res.tokensBefore,
        tokens_after: res.tokensAfter,
        window: context.window,
      })
    }
    return res
  }).catch(($, e, next) => next(e))
}
