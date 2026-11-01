// arugula-live (#396, a spike): Claude Code tells the Arugula pane it runs
// in about its session, from inside it, as things happen:
//
// - `usage`: the window's fill and size as the engine has them, after each
//   turn (`session.measure`), with the session's cost;
// - `step`: the model and effort of a model request, when either changes
//   (`turn.step`);
// - `compact`: a compaction once it ran, with its trigger and the tokens
//   before and after (`session.compact`);
// - `start`: a new session in the pane, with Claude Code's version.
//
// Each goes to the daemon through the installed CLI, `arugula
// session-signal`, with the signal on stdin (aru-039: the plugin calls the
// CLI you installed, which finds the daemon on every platform, Windows'
// named pipe included). The CLI answers both versions and the pane's
// context, which the status line shows. Outside an Arugula pane
// (no ARUGULA_PANE) it does nothing at all.
//
// It hooks no `classic.*` event: a Team or Enterprise login's guard skips a
// user mod's classic hooks (#395), and none of this needs them.

import type { Register, EngineInterface } from 'claude-code'

const MOD_VERSION = '0.1.0'

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
  pane?: number
  cli_version?: string
  daemon_version?: string
  context?: { tokens?: number | null; window?: number; compactions?: number } | null
  error?: string
}

// The module's own state starts over on a reload, which is fine: the daemon
// keeps the context, and the next turn says it all again.
let pane: string | undefined
let claudeVersion: string | undefined
let model: string | undefined
let effort: string | undefined
let queue: Promise<void> = Promise.resolve()

function tokensShort(n: number): string {
  return n >= 1_000_000 ? `${n / 1_000_000}M` : `${Math.round(n / 1000)}k`
}

function show($: EngineInterface, a: Answer): void {
  if (a.error !== undefined) {
    $.ui.status(`arugula %${pane} \u25cb ${a.error.split('\n')[0]?.slice(0, 80)}`)
    return
  }
  const parts = [`arugula %${a.pane ?? pane} \u25cf live`]
  const c = a.context
  if (c && c.tokens && c.window) parts.push(`${Math.round((100 * c.tokens) / c.window)}% of ${tokensShort(c.window)}`)
  if (c && c.compactions) parts.push(`${c.compactions} compacted`)
  if (a.cli_version && a.daemon_version && a.cli_version !== a.daemon_version) {
    parts.push(`cli ${a.cli_version} \u2260 daemon ${a.daemon_version}`)
  } else if (a.daemon_version) {
    parts.push(`v${a.daemon_version}`)
  }
  $.ui.status(parts.join(' \u00b7 '))
}

async function deliver($: EngineInterface, s: Signal): Promise<void> {
  let answer: Answer
  try {
    const body = JSON.stringify({
      ...s,
      session_id: await $.session.id(),
      claude_version: claudeVersion,
      mod_version: MOD_VERSION,
    })
    const r = await $.process.run(['arugula', 'session-signal'], { stdin: body, timeoutMs: 5000 })
    answer = (r.stdout.trim() ? JSON.parse(r.stdout) : {}) as Answer
    if (r.exitCode !== 0 && answer.error === undefined) answer.error = r.stderr.trim() || `exit ${r.exitCode}`
  } catch (err) {
    // No `arugula` on PATH, or it hung.
    answer = { error: err instanceof Error ? err.message : String(err) }
  }
  show($, answer)
}

// One at a time, in order: a compaction's signal never overtakes the usage
// before it.
function send($: EngineInterface, s: Signal): void {
  if (!pane) return
  queue = queue.then(() => deliver($, s))
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const res = await next(e)
    pane = await $.env.get('ARUGULA_PANE')
    if (!pane) return res
    claudeVersion = (await $.session.version()).version
    send($, { event: 'start' })
    return res
  }).catch(($, e, next) => next(e))

  on('turn.step', async function* ($, e, next) {
    if (!e.agentId) {
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
    if (e.changed.includes('context') || e.changed.includes('cost')) {
      send($, {
        event: 'usage',
        tokens: e.context.tokens,
        window: e.context.window,
        cost_usd: e.cost?.usd,
        model,
        effort,
      })
    }
    return next(e)
  }).catch(($, e, next) => next(e))

  on('session.compact', async ($, e, next) => {
    const res = await next(e)
    if (e.trigger !== 'precompute' && res.skip === undefined) {
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
