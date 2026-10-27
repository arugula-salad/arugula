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
// What it asks is a form (`SessionAskForm`): a question's choices as
// buttons, and its own words (Arugula #214), or a question card (`../ask`),
// answered by any owner or editor of its project; the platform passes the
// answer to the box. The same form is on the ask that mentions its asker
// (`./ask-card`). Move to… sends a queued or done session to another of
// the project's workspaces with its repo (Arugula #214).
//
// CI (M260): failures on its branch go back to it, at most `ci_max` rounds:
// the card counts them ("CI round 1 of 2"), and once CI failed again after
// the last, says it needs a person, until someone prompts it in the thread.
//
// Its PR (M259): once a turn pushed its branch, its asker opens a draft of
// the PR here, prefilled from the repo's PR template and what the turns
// left, edits it and sends it as themselves ("agents draft, people
// send"): GitHub's authorization in this browser, else one of their own
// machines. Nobody else sends it. Once sent, the PR button is the PR.
//
// Try 3 (M264): an owner or editor runs it two more ways at once (Claude
// Code and Codex, or Claude Code twice), each in a slot of its own on the
// box, re-running its first prompt. The card grows a compare view: the
// three side by side, each with its diffstat, how its turn finished, its
// page and its diff on GitHub, and how many run and wait for a slot.
// Pick keeps one; the others stop, "not picked". A review (`@codex
// review`) shows as a card in the thread, "reviewed" once it's posted.
//
// Once it merges, the card says "merged ✓" (M265, ./session-done).
//
// What its agent says in the thread (each turn's answer, a review) is the
// session's own message, `via` `said:<ses>` with no author (Arugula #215):
// the room draws it with the session's look (`SaidHead`, `SaidAvatar`),
// its agent and short id linked to its page, apart from people.

import "./session-card.css";
import { useEffect, useState } from "preact/hooks";
import { AskCard, type Ask } from "../ask";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";
import { usePlatformApi } from "../platform-api";
import type { ChatMessage, ChatRoom, SessionAct, SessionCard, SessionMove, SessionPick, SessionPr, SessionPrSent, SessionTry, SessionTryAsk, SessionView } from "../proto";
import { DiffLink, MergedLine, mergedOf } from "./session-done";

/** How long a turn may go without its box saying anything (`QUIET_MS`). */
export const QUIET_MS = 60_000;

const enc = encodeURIComponent;

/** The session a message is the card of, if it's one. */
export function sessionOf(m: ChatMessage): string | null {
  const v = m.via ?? "";
  return !m.author && v.startsWith("session:ses_") ? v.slice("session:".length) : null;
}

/** What a session's agent said in its thread (Arugula #215): a turn's
 * answer or a review, the session's own message (no author, `via`
 * `said:<ses>`). Its session, or null. Not its card (`session:<ses>`). */
export function saidOf(m: ChatMessage): string | null {
  const v = m.via ?? "";
  return !m.author && v.startsWith("said:ses_") ? v.slice("said:".length) : null;
}

/** "Claude Code · session 1a2b3c4d", or "Codex review · session …": who
 * said it, in words, from its session's card once it's read. */
export function saidName(ses: string, c: SessionCard | null): string {
  const short = ses.replace(/^ses_/, "").slice(0, 8);
  const agent = c ? harnessLabel(c.harness, c.ran).replace(/ \(.*\)$/, "") : "Agent";
  return `${agent}${c?.role === "review" ? " review" : ""} · session ${short}`;
}

// Each session's card, read once per page for the messages it said: a
// thread holds many of them, and the card's own reads stay with the card.
const saidCards = new Map<string, Promise<SessionCard | null>>();

/** Session `ses`'s card, for what it said; null until it's read (or if it
 * can't be). */
export function useSaidBy(ses: string): SessionCard | null {
  const [card, setCard] = useState<SessionCard | null>(null);
  useEffect(() => {
    let live = true;
    let read = saidCards.get(ses);
    if (!read) {
      read = call<SessionView>("GET", `/api/sessions/${enc(ses)}`).then(
        (v) => v.card,
        () => null,
      );
      saidCards.set(ses, read);
    }
    void read.then((c) => live && setCard(c));
    return () => {
      live = false;
    };
  }, [ses]);
  return card;
}

/** The head of what a session said: its agent and short id, linked to its
 * page, with a badge that says it's an agent session's, not a person's. */
export function SaidHead({ ses, project }: { ses: string; project: string | null }) {
  const api = usePlatformApi();
  const c = useSaidBy(ses);
  const where = c?.project ?? project;
  const page = where ? api.sessionHref?.(where, ses) : undefined;
  const name = saidName(ses, c);
  return (
    <>
      {page ? (
        <a class="thread-name said-name" href={page} title="This agent session's own page" data-said-page>
          {name}
        </a>
      ) : (
        <span class="thread-name said-name">{name}</span>
      )}
      <span class="msg-badge said-badge" title={c?.role === "review" ? "A read-only review by an agent session" : "What an agent session said at the end of a turn"}>
        {c?.role === "review" ? "review" : "agent session"}
      </span>
    </>
  );
}

/** A session's mark in the gutter: its agent's letter on the session's
 * own tone, not a person's picture. */
export function SaidAvatar({ ses }: { ses: string }) {
  const c = useSaidBy(ses);
  const letters = c && (c.ran ?? c.harness) === "codex" ? "Cx" : "CC";
  return (
    <span class="msg-avatar said-avatar" aria-hidden="true" data-said-avatar>
      {c ? letters : "·"}
    </span>
  );
}

/** A session's answer from before Arugula #215 (a turn's last message,
 * posted in its thread as its asker's `via` the session): the badge's
 * words and its title, or null. */
export function answerVia(m: ChatMessage, name: string): { label: string; title: string } | null {
  const v = m.via ?? "";
  if (!m.author || !v.startsWith("session:ses_")) return null;
  const short = v.slice("session:ses_".length, "session:ses_".length + 8);
  return { label: `agent session ${short}`, title: `What agent session ${short} said at the end of a turn, posted for ${name}` };
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

/** What a try's or a review's state says, past the plain states: a try
 * that wasn't kept is "not picked", a queued one with no slot on its box
 * "waiting for a slot", a posted review "reviewed", and one whose PR
 * merged (M265) "merged ✓". */
export function triedAs(state: string, reason: string | null, noSlot = false): { label: string; tone: Tone } | null {
  if (state === "stopped" && reason?.startsWith("not_picked")) return { label: "not picked", tone: "muted" };
  if (state === "stopped" && reason?.startsWith("reviewed")) return { label: "reviewed", tone: "ok" };
  if (state === "stopped" && reason?.startsWith("merged")) return { label: "merged ✓", tone: "ok" };
  if (state === "queued" && noSlot) return { label: "waiting for a slot", tone: "warn" };
  return null;
}

/** "2 running, 1 waiting for a slot": a Try 3's sessions, counted. */
export function triesLine(tries: SessionTry[]): string {
  const n = (f: (t: SessionTry) => boolean) => tries.filter(f).length;
  const parts: string[] = [];
  const running = n((t) => t.state === "working" || t.state === "needs_input" || t.state === "unresponsive");
  const starting = n((t) => t.state === "queued" && !t.no_slot);
  const waiting = n((t) => t.state === "queued" && t.no_slot);
  const done = n((t) => t.state === "done");
  const merged = n((t) => t.state === "stopped" && !!t.reason?.startsWith("merged"));
  if (running) parts.push(`${running} running`);
  if (starting) parts.push(`${starting} starting`);
  if (waiting) parts.push(`${waiting} waiting for a slot`);
  if (done) parts.push(`${done} done`);
  if (merged) parts.push(`${merged} merged`);
  return parts.join(", ");
}

/** The agent a try runs on, in words: "Codex", or "Claude Code (no Codex
 * on its box)" when its box stood in. */
export function harnessLabel(harness: string, ran: string | null): string {
  const name = (h: string) => (h === "codex" ? "Codex" : "Claude Code");
  return ran && ran !== harness ? `${name(ran)} (no ${name(harness)} on its box)` : name(ran ?? harness);
}

/** The model a session's agent runs on (Arugula #215), as its box said
 * ("Opus 5.5"); until it has, the one it was asked to use ("Sonnet"). */
export function modelLabel(c: { model: string | null; model_asked: string | null }): string | null {
  if (c.model) return c.model;
  const a = c.model_asked;
  return a ? a.charAt(0).toUpperCase() + a.slice(1) : null;
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

/** A question with a button per choice, each answering it at once, and
 * (`words`) the asker's own words. */
export function ChoiceForm({ question, choices, words, busy, answer }: { question: string; choices: string[]; words: boolean; busy: boolean; answer(text: string, own: boolean): void }) {
  const [own, setOwn] = useState("");
  return (
    <div class="session-ask-form" data-ask-form>
      <p class="session-card-line session-ask-question">{question}</p>
      {choices.length > 0 && (
        <div class="session-ask-choices" role="group" aria-label={question}>
          {choices.map((c) => (
            <button key={c} type="button" disabled={busy} data-ask-choice={c} onClick={() => answer(c, false)}>
              {c}
            </button>
          ))}
        </div>
      )}
      {words && (
        <form
          class="session-ask-words"
          onSubmit={(e) => {
            e.preventDefault();
            if (own.trim()) answer(own.trim(), true);
          }}
        >
          <input aria-label="Your own answer" placeholder={choices.length ? "Or in your own words" : "Your answer"} value={own} maxLength={2000} onInput={(e) => setOwn(e.currentTarget.value)} data-ask-words />
          <button type="submit" disabled={busy || !own.trim()} data-ask-send>
            Send
          </button>
        </form>
      )}
    </div>
  );
}

/** A session's open question as a form: its choices as buttons, and words
 * too (Arugula #214); a permission, or a question with no choices, as its
 * question card. `act` sends the answer to its box. */
export function SessionAskForm({ c, may, busy, act }: { c: SessionCard; may: boolean; busy: boolean; act(a: SessionAct): void }) {
  const asking = askOf(c);
  const o = c.open;
  if (!asking || !o) return null;
  if (c.answered) {
    return (
      <p class="session-card-line" data-session-answered>
        {c.answered.kind === "deny" ? "Denied" : c.answered.kind === "approve" ? "Allowed" : "Answered"} by {c.answered.name || "someone"}: waiting for its box to take it.
      </p>
    );
  }
  if (!may) {
    return (
      <div class="session-card-ask" data-session-ask>
        <p class="session-card-line">
          It asks: <b>{asking.ask.message}</b>
        </p>
        <p class="dim">The project's owners and editors answer it.</p>
      </div>
    );
  }
  const request = o.request ?? undefined;
  if (o.ask !== "permission" && o.options.length > 0) {
    return (
      <div class="session-card-ask" data-session-ask>
        <ChoiceForm
          question={o.question ?? "It waits for an answer"}
          choices={o.options}
          words
          busy={busy}
          answer={(text) => act({ kind: "answer", request, answer: { question_0: text } })}
        />
      </div>
    );
  }
  return (
    <div class="session-card-ask" data-session-ask>
      <AskCard key={asking.ask.id} ask={asking.ask} actions={{ answer: (content) => act(asking.act(content)), decline: () => act(asking.act(null)) }} />
    </div>
  );
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

/** Its PR, as its asker drafts and sends it: the title and body (from
 * the repo's template and the turns), the base, and whether it opens as a
 * draft on GitHub. */
export function PrEditor({ ses, onClose }: { ses: string; onClose: () => void }) {
  const [pr, setPr] = useState<SessionPr | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [base, setBase] = useState("");
  const [draft, setDraft] = useState(true);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const take = (p: SessionPr) => {
    setPr(p);
    setTitle(p.title);
    setBody(p.body);
    setBase(p.base);
    setDraft(p.draft);
  };
  useEffect(() => {
    let live = true;
    call<SessionPr>("GET", `/api/sessions/${enc(ses)}/pr`).then(
      (p) => live && take(p),
      (e: Error) => live && setSaid(e.message),
    );
    return () => void (live = false);
  }, [ses]);
  if (!pr) {
    return (
      <div class="session-pr" data-session-pr-editor="loading">
        <p class={said ? "session-card-line bad" : "session-card-line dim"}>{said ?? "Reading its PR…"}</p>
        {said && (
          <div class="session-card-buttons">
            <button type="button" onClick={onClose}>
              Close
            </button>
          </div>
        )}
      </div>
    );
  }
  const edit = { title, body, base: base.trim() || undefined, draft };
  const send = async () => {
    setBusy(true);
    setSaid(null);
    try {
      const back = location.pathname + location.search;
      const v = await call<SessionPrSent>("POST", `/api/sessions/${enc(ses)}/pr/send`, { edit, via: "browser", back });
      if (v.authorize) {
        location.assign(v.authorize);
        return;
      }
      take(v.pr);
      if (v.pr.state === "sent") onClose();
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const save = async () => {
    setBusy(true);
    setSaid(null);
    try {
      take(await call<SessionPr>("PUT", `/api/sessions/${enc(ses)}/pr`, edit));
      setSaid("Kept: it waits here until you send it.");
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      class="session-pr"
      data-session-pr-editor={pr.state}
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <div class="session-pr-where">
        {pr.repo}: <code>{pr.head}</code> into{" "}
        <input class="session-pr-base" aria-label="Base branch" value={base} onInput={(e) => setBase(e.currentTarget.value)} data-session-pr-base />
      </div>
      <input class="session-pr-title" aria-label="Title" value={title} maxLength={256} required onInput={(e) => setTitle(e.currentTarget.value)} data-session-pr-title />
      <textarea class="session-pr-body" aria-label="Description" rows={10} value={body} onInput={(e) => setBody(e.currentTarget.value)} data-session-pr-body />
      <label class="session-pr-draft">
        <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.currentTarget.checked)} data-session-pr-draft /> Open it as a draft on GitHub
      </label>
      {pr.template && <p class="dim session-pr-note">Its description starts from the repo's {pr.template}.</p>}
      {(said ?? pr.error) && (
        <p class={said?.startsWith("Kept") ? "session-card-line dim" : "session-card-line bad"} role="alert" data-session-pr-said>
          {said ?? pr.error}
        </p>
      )}
      <div class="session-card-buttons">
        <button type="submit" disabled={busy || !title.trim()} title="It goes out on GitHub as you" data-session-pr-send>
          Send as you
        </button>
        <button type="button" disabled={busy} onClick={() => void save()} data-session-pr-save>
          Keep
        </button>
        <span class="session-card-fill" />
        <button type="button" onClick={onClose} data-session-pr-close>
          Close
        </button>
      </div>
    </form>
  );
}

/** A Try 3's three, side by side (M264): each one's harness, state,
 * diffstat and finish, its page and its diff on GitHub, and Pick. */
function TriesView({ c, may, busy, pick }: { c: SessionCard; may: boolean; busy: boolean; pick: (ses: string) => void }) {
  const api = usePlatformApi();
  return (
    <div class="session-tries" data-session-tries={c.tries.length}>
      <p class="session-card-line" data-session-tries-line>
        <b>Tried 3 ways</b>
        <span class="dim"> · {c.picked ? "one kept" : triesLine(c.tries)}</span>
      </p>
      <div class="session-tries-grid">
        {c.tries.map((t, i) => {
          const special = triedAs(t.state, t.reason, t.no_slot);
          const { label, tone } = special ?? stateOf(t.state, false);
          const kept = c.picked === t.session;
          const page = api.sessionHref?.(c.project, t.session);
          const diff = t.pushed && c.repo ? `https://github.com/${c.repo}/compare/${t.branch}` : null;
          const short = t.session.replace(/^ses_/, "").slice(0, 8);
          return (
            <div key={t.session} class={`session-try tone-${kept ? "ok" : tone}${kept ? " kept" : ""}`} data-session-try={t.session} data-session-try-state={special?.label ?? t.state}>
              <div class="session-try-top">
                <span class="session-try-n">{i === 0 ? "this one" : `try ${i + 1}`}</span>
                <span class="session-try-state" data-session-try-label>
                  {kept ? "kept" : label}
                </span>
              </div>
              <div class="session-try-harness" data-session-try-harness>
                {harnessLabel(t.harness, t.ran)}
              </div>
              {t.turn?.diffstat ? <pre data-session-try-diffstat>{t.turn.diffstat}</pre> : <p class="dim session-try-none">{t.turn ? "No changes" : "No turn yet"}</p>}
              {t.turn?.finish && (
                <p class="session-try-finish" data-session-try-finish={t.turn.finish.ok ? "ok" : "failed"}>
                  {t.turn.finish.ok ? (
                    <>
                      finish: <span class="ok">✓</span>
                    </>
                  ) : (
                    <span class="bad">finish failed</span>
                  )}{" "}
                  <code>{t.turn.finish.command}</code>
                </p>
              )}
              <div class="session-try-links">
                {page ? (
                  <a href={page} title="Its page: its card and history" data-session-try-page>
                    {short}
                  </a>
                ) : (
                  <span>{short}</span>
                )}
                {diff && (
                  <a href={diff} target="_blank" rel="noopener" title={`What it pushed, against the repo's default branch, on GitHub`} data-session-try-diff>
                    diff ↗
                  </a>
                )}
                <span class="session-card-fill" />
                {may && !c.picked && c.merged_at === null && t.state !== "failed" && t.state !== "stopped" && (
                  <button type="button" disabled={busy} title="Keep this one: the others stop, and what they didn't push goes" data-session-pick={t.session} onClick={() => pick(t.session)}>
                    Pick
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
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
  const [drafting, setDrafting] = useState(false);
  const [tryWith, setTryWith] = useState("codex");
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
  const { label, tone } = mergedOf(c) ?? triedAs(c.state, c.reason) ?? stateOf(c.state, quiet);
  const short = c.session.replace(/^ses_/, "").slice(0, 8);
  const ended = c.state === "done" || c.state === "failed" || c.state === "stopped";
  const took = elapsed((c.ended_at ?? now) - c.created_at);
  const act = async (path: string, body?: SessionAct | SessionTryAsk | SessionPick | SessionMove) => {
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
  const idle = idleOf(c);
  // The last turn's finish step failed (M261): it wasn't pushed. While the
  // session still works, the agent has its one turn to fix it.
  const failed = c.turn?.finish?.ok === false;
  const fixing = failed && (c.state === "working" || c.state === "needs_input");
  const port = c.ports[0];
  const page = api.sessionHref?.(c.project, c.session);
  const parentPage = c.parent_session ? api.sessionHref?.(c.project, c.parent_session) : undefined;
  const parentShort = c.parent_session?.replace(/^ses_/, "").slice(0, 8);
  const review = c.role === "review";
  // Not once its PR merged (M265): its thread is done.
  const mayTry = view.may && c.role === "thread" && c.tries.length === 0 && c.merged_at === null && c.state !== "failed" && c.state !== "stopped";
  return (
    <div class={`session-card tone-${tone}${large ? " large" : ""}`} data-session-card={c.session} data-session-state={quiet && c.state !== "unresponsive" ? "quiet" : c.state}>
      <div class="session-card-top">
        <span class="session-card-state" data-session-label>
          {label}
        </span>
        <span class="session-card-what">{review ? "review" : c.role === "try" ? "try" : "agent session"}</span>
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
        {c.harness !== "claude" || (c.ran && c.ran !== "claude") ? (
          <span class="dim" data-session-harness>
            · {harnessLabel(c.harness, c.ran)}
          </span>
        ) : null}
        {modelLabel(c) && (
          <span
            class="dim"
            data-session-model
            title={c.model ? (c.model_asked ? `Asked for ${c.model_asked}; its agent runs on ${c.model}` : `Its agent runs on ${c.model}`) : `Asked for ${c.model_asked}`}
          >
            · {modelLabel(c)}
          </span>
        )}
      </div>
      {c.parent_session && (
        <p class="session-card-line dim" data-session-parent>
          {review ? "A read-only review of what " : "One of three tries of "}
          {parentPage ? <a href={parentPage}>{parentShort}</a> : parentShort}
          {review ? " pushed: it pushes nothing, and its review is posted in the thread." : c.picked === c.session ? ": kept." : "."}
        </p>
      )}
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
      <MergedLine c={c} />
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
      {c.needs_person ? (
        <p class="session-card-line bad" data-session-ci="needs-person">
          CI failed again after {c.ci_max} rounds: needs a person. Reply in the thread to take it on.
        </p>
      ) : c.ci_rounds > 0 ? (
        <p class="session-card-line dim" data-session-ci={String(c.ci_rounds)}>
          CI round {c.ci_rounds} of {c.ci_max}
        </p>
      ) : null}
      {c.turn && (
        <div class="session-card-turn" data-session-turn>
          <span class={c.turn.pushed ? "ok" : "dim"}>{review ? "Read only" : c.turn.pushed ? "Pushed" : "Not pushed"}</span>
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
                  {fixing && <span class="dim"> · the agent is fixing it</span>}
                </p>
                {c.turn.finish.output_tail && <pre data-session-finish-output>{lastLines(c.turn.finish.output_tail)}</pre>}
              </div>
            ))}
        </div>
      )}
      <SessionAskForm c={c} may={view.may} busy={busy} act={(a) => void act("actions", a)} />
      {c.cancelling && <p class="session-card-line dim">Cancelling the turn: waiting for its box.</p>}
      {said && (
        <p class="session-card-line bad" role="alert" data-session-said>
          {said}
        </p>
      )}
      {c.tries.length > 0 && c.role === "thread" && <TriesView c={c} may={view.may} busy={busy} pick={(ses) => void act("pick", { session: ses })} />}
      {drafting && !c.pr && <PrEditor ses={c.session} onClose={() => setDrafting(false)} />}
      <div class="session-card-buttons">
        {c.machine && block && (
          <button type="button" title="Its agent block, live on its box" data-session-open onClick={() => open(`pane=${c.machine}.${block}`, () => api.goToPane(c.machine!, block, null))}>
            Open ↗
          </button>
        )}
        {c.machine && port && (
          <button
            type="button"
            title={port.url ? `Its dev server on :${port.port}, public` : `Its dev server on :${port.port}, on Control`}
            data-session-preview
            onClick={() => (port.url ? window.open(port.url, "_blank", "noopener") : open(`machine=${c.machine}`, () => api.goToSession(c.machine!)))}
          >
            Preview ↗
          </button>
        )}
        {review ? null : <DiffLink c={c} />}
        {review ? null : c.pr && /^https:\/\//.test(c.pr) ? (
          <a class="button" href={c.pr} target="_blank" rel="noopener" data-session-pr>
            PR ↗
          </a>
        ) : c.pushed && view.asked ? (
          <button
            type="button"
            aria-pressed={drafting}
            title={failed ? "Draft its PR, then send it as you: it has what was pushed, not the last turn, whose finish step failed" : "Draft its PR, then send it as you"}
            data-session-pr
            onClick={() => setDrafting((d) => !d)}
          >
            PR…
          </button>
        ) : (
          <button
            type="button"
            disabled
            title={
              c.pushed ? `${c.asker_name || "Its asker"} sends its PR: it goes out as them` : failed ? "PR after its finish step passes and a turn pushes its branch" : "PR after a turn pushes its branch"
            }
            data-session-pr
          >
            PR
          </button>
        )}
        {mayTry && (
          <span class="session-try3" data-session-try3>
            <button
              type="button"
              disabled={busy}
              title="Run it two more ways at once, each in a slot of its own on its box, re-running its first prompt; then keep the one you like"
              data-session-try-go
              onClick={() => void act("try", { harnesses: ["claude", tryWith] })}
            >
              Try 3
            </button>
            <select aria-label="The two more" title="The agents the two more run on" value={tryWith} disabled={busy} onChange={(e) => setTryWith(e.currentTarget.value)} data-session-try-with>
              <option value="codex">+ Claude, Codex</option>
              <option value="claude">+ Claude ×2</option>
            </select>
          </span>
        )}
        {c.moves.length > 0 && (c.state === "queued" || c.state === "done") && (
          <select
            class="session-move"
            aria-label="Move to…"
            title="Move it to another of the project's workspaces with its repo: its next turn starts there, from the thread"
            value=""
            disabled={busy}
            data-session-move
            onChange={(e) => {
              const workspace = e.currentTarget.value;
              if (workspace) void act("move", { workspace });
            }}
          >
            <option value="">Move to…</option>
            {c.moves.map((p) => (
              <option key={p.workspace} value={p.workspace}>
                {p.name}
                {p.awake ? "" : " (asleep)"}
                {p.busy ? ` · ${p.busy} going` : ""}
              </option>
            ))}
          </select>
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
