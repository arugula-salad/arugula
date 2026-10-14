// What a conversation's links do, in its timeline (#857): a turn that
// ended (with its last message), a card for what a link asks (Allow and
// Deny, answered by the ask's id through the link's machine), who answered
// it, an exit, and a linked PR's checks. The home's watcher posts them as
// typed events (`ThreadMsg.event`); this shows them.
//
// #859: a question (Claude's AskUserQuestion) shows its options, as the
// card beside the pane does (one pick, several, "Other"), and the lead's
// own asks are cards too (link `lead`), answered here on the lead's block.
//
// #887: an edit waiting as a diff (arugulad is Claude Code's IDE) is a card
// with the diff, Accept and Reject. A card whose ask is gone at the pane
// (the link's machine refuses the answer, 409) stops offering its buttons.

import { useState } from "preact/hooks";
import { AskCard, type Ask } from "../blocks/ask";
import { DiffText } from "./diff-card";
import { Markup } from "./markup";
import type { Client } from "../client";
import type { Action, ConvEvent, ThreadMsg } from "../proto";

/** Who the lead's messages, and its own cards, are by. */
const LEAD = "lead";

/** What the events in a timeline say about each other: which cards were
 * answered here (and by whom), and the newest event of each link. */
export interface EventIndex {
  answered: Map<string, ThreadMsg>;
  newest: Map<string, number>;
}

const cardKey = (link: string, ask: string) => `${link.toLowerCase()} ${ask}`;

export function eventIndex(msgs: ThreadMsg[]): EventIndex {
  const answered = new Map<string, ThreadMsg>();
  const newest = new Map<string, number>();
  for (const m of msgs) {
    const e = m.event;
    // The lead said something since its card: it went on (#859).
    if (!e && m.who === LEAD) newest.set(LEAD, m.id);
    if (!e) continue;
    if (e.kind === "answered" && e.ask) answered.set(cardKey(e.link, e.ask.id), m);
    else newest.set(e.link.toLowerCase(), m.id);
  }
  return { answered, newest };
}

/** A duration as people say it: 42s, 3m 5s, 1h 2m. */
function took(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function did(a: Action | undefined, question = false): string {
  if (a === "accept") return "accepted";
  if (a === "reject") return "rejected";
  return a === "allow" ? "allowed" : a === "answer" ? "answered" : question ? "skipped" : "denied";
}

/** A message that carries an event, as its line or its card. */
export function EventBody({ client, conv, m, index }: { client: Client; conv: string; m: ThreadMsg; index: EventIndex }) {
  const e = m.event!;
  switch (e.kind) {
    case "turn_ended": {
      const parts = [e.worked_ms != null ? `worked ${took(e.worked_ms)}` : "", e.approvals ? `${e.approvals} approval${e.approvals === 1 ? "" : "s"}` : ""].filter(Boolean);
      return (
        <div class="conv-event" data-conv-event="turn_ended" data-event-link={e.link}>
          <div class="conv-event-line">
            <span class="conv-event-dot done" aria-hidden="true" />
            Turn ended{parts.length ? <span class="conv-event-meta"> · {parts.join(", ")}</span> : null}
          </div>
          {/* The agent's own words, in the chat's small Markdown (#889):
              Markup makes only Preact nodes, so it stays text. */}
          {e.summary && (
            <div class="conv-event-summary" data-event-summary>
              <Markup text={e.summary} />
            </div>
          )}
        </div>
      );
    }
    case "needs_input":
      return <Card client={client} conv={conv} m={m} e={e} index={index} />;
    case "answered":
      return (
        <div class="conv-event" data-conv-event="answered" data-event-link={e.link}>
          <div class="conv-event-line">
            <span>
              {did(e.action, e.ask?.what === "question")} {e.link}
              {e.ask ? ":" : ""}
            </span>
            {e.summary ? (
              <span class="conv-event-meta" data-event-answer>
                {e.summary}
              </span>
            ) : e.ask ? (
              <span class="conv-event-meta">{e.ask.input}</span>
            ) : null}
          </div>
        </div>
      );
    case "exited":
      return (
        <div class="conv-event" data-conv-event="exited" data-event-link={e.link}>
          <div class="conv-event-line">
            <span class="conv-event-dot gone" aria-hidden="true" />
            Exited{e.code != null ? ` (${e.code})` : ""}
          </div>
        </div>
      );
    case "pr":
      return (
        <div class="conv-event" data-conv-event="pr" data-event-link={e.link} data-event-ok={e.ok ? "1" : "0"}>
          <div class="conv-event-line">
            <span class={`conv-event-dot ${e.ok ? "done" : "failed"}`} aria-hidden="true" />
            {e.summary ?? "Its PR changed"}
          </div>
        </div>
      );
  }
}

/** A link's ask (or the lead's own), with Allow and Deny while it waits,
 * or a question's options. Answered here, it says by whom; once the link
 * has moved on, it says it was answered elsewhere. */
function Card({ client, conv, m, e, index }: { client: Client; conv: string; m: ThreadMsg; e: ConvEvent; index: EventIndex }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // The link's machine said its ask is gone (#887): answered at the pane,
  // or a diff took its place.
  const [gone, setGone] = useState("");
  const ask = e.ask!;
  const answer = index.answered.get(cardKey(e.link, ask.id));
  const moved = gone || (index.newest.get(e.link.toLowerCase()) ?? m.id) > m.id;
  const state = answer ? "answered" : moved ? "moved" : "open";
  const send = async (action: Action, content?: Record<string, unknown>) => {
    setBusy(true);
    setError("");
    try {
      await client.convAct(conv, { link: e.link, id: ask.id, action, content });
    } catch (err) {
      const status = (err as { status?: number }).status;
      if (status === 409 || status === 410) setGone((err as Error).message);
      else setError((err as Error).message);
    }
    setBusy(false);
  };
  const act = (action: Action) => () => send(action);
  const diff = ask.diff;
  const approve = ask.what === "approve";
  const questions = !approve && ask.questions?.length ? ask.questions : null;
  const asked: Ask | null = questions && {
    id: ask.id,
    kind: "questions",
    // A hook's headline is its first question, which the card shows.
    message: ask.input === questions[0]?.question ? "" : ask.input,
    questions,
    source: "conversation",
    at_ms: m.at,
  };
  const who = answer?.name.split("@")[0];
  if (diff) {
    return (
      <div class={`conv-card ${state} diff`} data-conv-card={ask.id} data-event-link={e.link} data-card-state={state} data-card-diff>
        <div class="conv-card-head">
          Wants to {diff.new ? "create" : "edit"}
          {ask.tool ? <span class="msg-badge">{ask.tool}</span> : null}
        </div>
        <div class="conv-card-file">
          <b>{diff.file}</b>{" "}
          <span class="diff-counts">
            <i class="add">+{diff.added}</i> <i class="del">−{diff.removed}</i>
          </span>
        </div>
        <details class="conv-card-diff" open={state === "open"}>
          <summary>The diff</summary>
          <DiffText text={diff.text} />
        </details>
        {state === "open" ? (
          <div class="conv-card-buttons">
            <button class="primary" data-card-accept disabled={busy} onClick={act("accept")}>
              Accept
            </button>
            <button data-card-reject disabled={busy} onClick={act("reject")}>
              Reject
            </button>
          </div>
        ) : (
          <div class="conv-card-note" data-card-done title={gone || undefined}>
            {answer ? `${did(answer.event?.action)} by ${who}` : "Answered elsewhere"}
          </div>
        )}
        {error && <p class="thread-error">{error}</p>}
      </div>
    );
  }
  return (
    <div class={`conv-card ${state}${asked ? " question" : ""}`} data-conv-card={ask.id} data-event-link={e.link} data-card-state={state}>
      <div class="conv-card-head">
        {approve ? "Wants to run" : "Asks"}
        {ask.tool ? <span class="msg-badge">{ask.tool}</span> : null}
      </div>
      {asked && state === "open" ? (
        <fieldset class="conv-card-form" disabled={busy}>
          <AskCard ask={asked} actions={{ answer: (content) => send("answer", content), decline: () => send("deny") }} />
        </fieldset>
      ) : (
        <pre class="conv-card-input">{approve ? ask.input.replace(/^wants to run /, "") : ask.input}</pre>
      )}
      {state === "open" ? (
        asked ? null : (
          <div class="conv-card-buttons">
            {approve ? (
              <>
                <button class="primary" data-card-allow disabled={busy} onClick={act("allow")}>
                  Allow
                </button>
                <button data-card-deny disabled={busy} onClick={act("deny")}>
                  Deny
                </button>
              </>
            ) : (
              <>
                <span class="conv-card-note">Answer it at the pane, or</span>
                <button data-card-deny disabled={busy} onClick={act("deny")}>
                  Skip
                </button>
              </>
            )}
          </div>
        )
      ) : (
        <div class="conv-card-note" data-card-done title={gone || undefined}>
          {answer
            ? `${did(answer.event?.action, !approve)} by ${who}${answer.event?.summary ? `: ${answer.event.summary}` : ""}`
            : "Answered elsewhere"}
        </div>
      )}
      {error && <p class="thread-error">{error}</p>}
    </div>
  );
}
