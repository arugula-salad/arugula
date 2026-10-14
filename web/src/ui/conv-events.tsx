// What a conversation's links do, in its timeline (#857): a turn that
// ended (with its last message), a card for what a link asks (Allow and
// Deny, answered by the ask's id through the link's machine), who answered
// it, an exit, and a linked PR's checks. The home's watcher posts them as
// typed events (`ThreadMsg.event`); this shows them.

import { useState } from "preact/hooks";
import type { Client } from "../client";
import type { Action, ConvEvent, ThreadMsg } from "../proto";

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

function did(a: Action | undefined): string {
  return a === "allow" ? "allowed" : a === "answer" ? "answered" : "denied";
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
          {e.summary && (
            <p class="conv-event-summary" data-event-summary>
              {e.summary}
            </p>
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
              {did(e.action)} {e.link}
              {e.ask ? ":" : ""}
            </span>
            {e.ask ? <span class="conv-event-meta">{e.ask.input}</span> : null}
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

/** A link's ask, with Allow and Deny while it waits. Answered here, it
 * says by whom; once the link has moved on, it says it was answered
 * elsewhere. */
function Card({ client, conv, m, e, index }: { client: Client; conv: string; m: ThreadMsg; e: ConvEvent; index: EventIndex }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const ask = e.ask!;
  const answer = index.answered.get(cardKey(e.link, ask.id));
  const moved = (index.newest.get(e.link.toLowerCase()) ?? m.id) > m.id;
  const state = answer ? "answered" : moved ? "moved" : "open";
  const act = (action: Action) => async () => {
    setBusy(true);
    setError("");
    try {
      await client.convAct(conv, { link: e.link, id: ask.id, action });
    } catch (err) {
      setError((err as Error).message);
    }
    setBusy(false);
  };
  const approve = ask.what === "approve";
  return (
    <div class={`conv-card ${state}`} data-conv-card={ask.id} data-event-link={e.link} data-card-state={state}>
      <div class="conv-card-head">
        {approve ? "Wants to run" : "Asks"}
        {ask.tool ? <span class="msg-badge">{ask.tool}</span> : null}
      </div>
      <pre class="conv-card-input">{ask.input}</pre>
      {state === "open" ? (
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
      ) : (
        <div class="conv-card-note" data-card-done>
          {answer ? `${did(answer.event?.action)} by ${answer.name.split("@")[0]}` : "Answered elsewhere"}
        </div>
      )}
      {error && <p class="thread-error">{error}</p>}
    </div>
  );
}
