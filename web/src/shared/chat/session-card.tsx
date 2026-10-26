// An agent session's status card (M257, thread = session): the one card a
// chat timeline shows ("A thread's agent session runs on the project's
// hosted box"). Its message is the platform's (no author, `via`
// `session:<ses>`) and never changes; the card is the session as it is
// now (`GET /api/sessions/:ses`), read again whenever the platform says it
// changed (`ChatLive::Session`, heard through chat's stream), so the card
// moves in place, on every page that shows it.
//
// It says the session's state (queued, working, needs input, done,
// failed, stopped responding, stopped), its repo and branch, how long it's
// run, the tool it's on or the error it hit, and what its last turn left,
// with what the workspace's finish step made of it (M261): "finish: ✓ just
// check", or "finish failed:" and the last lines of its output.
// A box that hasn't been heard from for 60 s is "stopped responding" here
// at once, against the platform's clock, before the platform marks it so.
// What it asks is a question card (`../ask`), answered by any owner or
// editor of its project; the platform passes the answer to the box.

import "./session-card.css";
import { useEffect, useState } from "preact/hooks";
import { AskCard, type Ask } from "../ask";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";
import { usePlatformApi } from "../platform-api";
import type { ChatMessage, ChatRoom, SessionAct, SessionCard, SessionView } from "../proto";

/** How long a turn may go without its box saying anything (`QUIET_MS`). */
export const QUIET_MS = 60_000;

const enc = encodeURIComponent;

/** The session a message is the card of, if it's one. */
export function sessionOf(m: ChatMessage): string | null {
  const v = m.via ?? "";
  return !m.author && v.startsWith("session:ses_") ? v.slice("session:".length) : null;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

export type Tone = "accent" | "ok" | "warn" | "danger" | "muted";

/** What the card says of a session's state, and its tone. `quiet`: its
 * box hasn't been heard from for [`QUIET_MS`] mid-turn. */
export function stateOf(state: string, quiet: boolean): { label: string; tone: Tone } {
  if (state === "unresponsive" || (quiet && (state === "working" || state === "needs_input"))) return { label: "stopped responding", tone: "danger" };
  switch (state) {
    case "queued":
      return { label: "queued", tone: "warn" };
    case "working":
      return { label: "working", tone: "accent" };
    case "needs_input":
      return { label: "needs input", tone: "warn" };
    case "done":
      return { label: "done", tone: "ok" };
    case "failed":
      return { label: "failed", tone: "danger" };
    default:
      return { label: "stopped", tone: "muted" };
  }
}

/** Whether its box has gone quiet mid-turn, at `now` (the platform's clock). */
export function isQuiet(c: SessionCard, now: number): boolean {
  if (c.state !== "working" && c.state !== "needs_input") return false;
  return now - (c.heard_at ?? c.updated_at) >= QUIET_MS;
}

/** "4s", "3m 05s", "2h 10m". */
export function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${String(m % 60).padStart(2, "0")}m`;
}

/** The last `n` lines of a finish step's output. */
export function lastLines(text: string, n = 6): string {
  return text.trimEnd().split("\n").slice(-n).join("\n");
}

/** What an idle session's stop says (M261, the platform's `idle: …`). */
export function idleOf(c: SessionCard): string | null {
  return c.state === "stopped" && c.reason?.startsWith("idle: ") ? c.reason.slice("idle: ".length) : null;
}

/** A permission's option id in words: `allow-once` is "Allow once". */
export function optionLabel(id: string): string {
  const w = id.replace(/[-_]+/g, " ").trim();
  return w ? w[0].toUpperCase() + w.slice(1) : id;
}

const refuses = (id: string) => /^(reject|deny)/i.test(id);

/** What the card asks, as a question card, and how its answer becomes the
 * box's action: a permission's options (or allow and deny), picked; a
 * question, answered in words. */
export function askOf(c: SessionCard): { ask: Ask; act(content: Record<string, unknown> | null): SessionAct } | null {
  const o = c.open;
  if (!o) return null;
  const request = o.request ?? undefined;
  const base = { id: request ?? `${c.session}-open`, source: "session", agent: "The session", at_ms: o.at };
  if (o.ask === "permission") {
    const ids = o.options.length ? o.options : ["allow", "deny"];
    const labels = ids.map(optionLabel);
    return {
      ask: {
        ...base,
        kind: "questions",
        message: o.question ?? "It asks to use a tool",
        questions: [{ question: o.question ?? "Allow it?", header: "Permission", options: labels.map((label) => ({ label })) }],
      },
      act(content) {
        const picked = typeof content?.question_0 === "string" ? ids[labels.indexOf(content.question_0)] : undefined;
        const note = typeof content?.question_0_custom === "string" ? content.question_0_custom : undefined;
        if (!picked || refuses(picked)) return { kind: "deny", request, ...(note ? { reason: note } : {}) };
        return { kind: "approve", request, ...(o.options.length ? { option: picked } : {}) };
      },
    };
  }
  return {
    ask: {
      ...base,
      kind: "form",
      message: o.question ?? "It waits for an answer",
      schema: { type: "object", properties: { answer: { type: "string", title: "Your answer", maxLength: 2000 } }, required: ["answer"] },
    },
    // Skipped: an empty answer, which the box takes as declined.
    act: (content) => ({ kind: "answer", request, answer: typeof content?.answer === "string" ? { question_0: content.answer } : {} }),
  };
}

/** A session, read now and again each time it changes; `null` until it's
 * read, and the platform's clock against this one's. */
export function useSession(ses: string): { view: SessionView | null; error: string | null; skew: number; set(v: SessionView): void } {
  const [view, setView] = useState<SessionView | null>(null);
  const [skew, setSkew] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const take = (v: SessionView) => {
    setView(v);
    setSkew(v.now - Date.now());
    setError(null);
  };
  useEffect(() => {
    let live = true;
    const read = () =>
      void call<SessionView>("GET", `/api/sessions/${enc(ses)}`).then(
        (v) => live && take(v),
        (e: Error) => live && setError(e.message),
      );
    read();
    const off = chat.onObject(ses, read);
    return () => {
      live = false;
      off();
    };
  }, [ses]);
  return { view, error, skew, set: take };
}

/** The platform's clock now, moving each second while `on`. */
function useNow(skew: number, on: boolean): number {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [on]);
  return Date.now() + skew;
}

/** The card in a room's timeline or a thread, in place of its message. */
export function SessionCardMessage({ m }: { room: ChatRoom; m: ChatMessage }) {
  const ses = sessionOf(m)!;
  return (
    <div class="thread-msg first session-msg" data-msg={m.seq}>
      <div class="msg-gutter" />
      <div class="msg-body">
        <SessionCardView ses={ses} />
      </div>
    </div>
  );
}

/** A session's card; `large` on its own page. */
export function SessionCardView({ ses, large = false, onView }: { ses: string; large?: boolean; onView?: (v: SessionView) => void }) {
  const api = usePlatformApi();
  const { view, error, skew, set } = useSession(ses);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const c = view?.card;
  const going = !!c && (c.state === "queued" || c.state === "working" || c.state === "needs_input" || c.state === "unresponsive");
  const now = useNow(skew, going);
  useEffect(() => void (view && onView?.(view)), [view]);
  if (!view || !c) {
    return (
      <div class="session-card tone-muted" data-session-card={ses} data-session-state="loading">
        <span class="dim">{error ? `This session can't be read: ${error}` : "Reading the session…"}</span>
      </div>
    );
  }
  const quiet = isQuiet(c, now);
  const { label, tone } = stateOf(c.state, quiet);
  const short = c.session.replace(/^ses_/, "").slice(0, 8);
  const ended = c.state === "done" || c.state === "failed" || c.state === "stopped";
  const took = elapsed((c.ended_at ?? now) - c.created_at);
  const act = async (path: string, body?: SessionAct) => {
    setBusy(true);
    try {
      set(await call<SessionView>("POST", `/api/sessions/${enc(c.session)}/${path}`, body));
      setSaid(null);
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const open = (hash: string, inPage: () => void) => (api.controlHref ? window.open(api.controlHref(hash), "_blank", "noopener") : inPage());
  const block = c.block && /^\d+$/.test(c.block) ? c.block : null;
  const asking = askOf(c);
  const idle = idleOf(c);
  const port = c.ports[0];
  const page = api.sessionHref?.(c.project, c.session);
  return (
    <div class={`session-card tone-${tone}${large ? " large" : ""}`} data-session-card={c.session} data-session-state={quiet && c.state !== "unresponsive" ? "quiet" : c.state}>
      <div class="session-card-top">
        <span class="session-card-state" data-session-label>
          {label}
        </span>
        <span class="session-card-what">agent session</span>
        {page ? (
          <a class="session-card-id" href={page} title="This session's own page" data-session-page>
            {short}
          </a>
        ) : (
          <span class="session-card-id">{short}</span>
        )}
        <span class="session-card-fill" />
        <time class="session-card-time" title={ended ? "How long it ran" : "How long it's run"} data-session-elapsed>
          {took}
        </time>
      </div>
      <div class="session-card-where">
        {c.repo && <span data-session-repo>{c.repo}</span>}
        <span class="session-card-branch" data-session-branch>
          {c.branch}
        </span>
        {c.workspace_name && <span class="dim">on {c.workspace_name}</span>}
      </div>
      {quiet || c.state === "unresponsive" ? (
        <p class="session-card-line bad" data-session-quiet>
          Its box hasn't said anything for {elapsed(now - (c.heard_at ?? c.updated_at))}: it may be down. It picks up where it was when it's back.
        </p>
      ) : c.state === "working" && c.last_tool ? (
        <p class="session-card-line" data-session-tool>
          Running <code>{c.last_tool}</code>
        </p>
      ) : c.state === "queued" ? (
        <p class="session-card-line dim">Waiting for its box to take it.</p>
      ) : null}
      {c.last_error && (
        <p class="session-card-line bad" data-session-error>
          {c.last_error}
        </p>
      )}
      {c.reason && c.state !== "stopped" && (
        <p class={c.state === "queued" ? "session-card-line dim" : "session-card-line bad"} data-session-reason>
          {c.reason.replace(/^[a-z_]+: /, "")}
        </p>
      )}
      {idle && (
        <p class="session-card-line dim" data-session-idle>
          {idle[0].toUpperCase() + idle.slice(1)}.
        </p>
      )}
      {c.turn && (
        <div class="session-card-turn" data-session-turn>
          <span class={c.turn.pushed ? "ok" : "dim"}>{c.turn.pushed ? "Pushed" : "Not pushed"}</span>
          <span class="dim">
            {" "}
            · {c.turn.commits.length} commit{c.turn.commits.length === 1 ? "" : "s"}
            {c.turn.uncommitted ? ` · ${c.turn.uncommitted} uncommitted` : ""}
          </span>
          {c.turn.diffstat && <pre data-session-diffstat>{c.turn.diffstat}</pre>}
          {c.turn.finish &&
            (c.turn.finish.ok ? (
              <p class="session-card-finish" data-session-finish="ok">
                finish: <span class="ok">✓</span> <code>{c.turn.finish.command}</code>
              </p>
            ) : (
              <div class="session-card-finish" data-session-finish="failed">
                <p class="bad">
                  finish failed: <code>{c.turn.finish.command}</code>
                </p>
                {c.turn.finish.output_tail && <pre data-session-finish-output>{lastLines(c.turn.finish.output_tail)}</pre>}
              </div>
            ))}
        </div>
      )}
      {asking &&
        (c.answered ? (
          <p class="session-card-line" data-session-answered>
            {c.answered.kind === "deny" ? "Denied" : c.answered.kind === "approve" ? "Allowed" : "Answered"} by {c.answered.name || "someone"}: waiting for its box to
            take it.
          </p>
        ) : view.may ? (
          <div class="session-card-ask" data-session-ask>
            <AskCard key={asking.ask.id} ask={asking.ask} actions={{ answer: (content) => void act("actions", asking.act(content)), decline: () => void act("actions", asking.act(null)) }} />
          </div>
        ) : (
          <div class="session-card-ask" data-session-ask>
            <p class="session-card-line">
              It asks: <b>{asking.ask.message}</b>
            </p>
            <p class="dim">The project's owners and editors answer it.</p>
          </div>
        ))}
      {c.cancelling && <p class="session-card-line dim">Cancelling the turn: waiting for its box.</p>}
      {said && (
        <p class="session-card-line bad" role="alert" data-session-said>
          {said}
        </p>
      )}
      <div class="session-card-buttons">
        {c.machine && block && (
          <button type="button" title="Its agent block, live on its box" data-session-open onClick={() => open(`pane=${c.machine}.${block}`, () => api.goToPane(c.machine!, block, null))}>
            Open ↗
          </button>
        )}
        {c.machine && port && (
          <button
            type="button"
            title={port.url ? `Its dev server on :${port.port}, public` : `Its dev server on :${port.port}, on control's page`}
            data-session-preview
            onClick={() => (port.url ? window.open(port.url, "_blank", "noopener") : open(`machine=${c.machine}`, () => api.goToSession(c.machine!)))}
          >
            Preview ↗
          </button>
        )}
        {c.pr && /^https:\/\//.test(c.pr) ? (
          <a class="button" href={c.pr} target="_blank" rel="noopener" data-session-pr>
            PR ↗
          </a>
        ) : (
          <button type="button" disabled title="PR after the turn pushes" data-session-pr>
            PR
          </button>
        )}
        <span class="session-card-fill" />
        {view.may && (c.state === "working" || c.state === "needs_input") && !c.cancelling && (
          <button type="button" disabled={busy} title="Cancel the turn under way: the session goes on" data-session-cancel onClick={() => void act("actions", { kind: "cancel" })}>
            Cancel turn
          </button>
        )}
        {view.may && c.state !== "failed" && c.state !== "stopped" && (
          <button type="button" class="danger" disabled={busy} title="Stop the session: its box stops the agent" data-session-stop onClick={() => void act("stop")}>
            Stop
          </button>
        )}
      </div>
      <div class="session-card-foot dim">Asked by {c.asker_name || "someone who left"}</div>
    </div>
  );
}
