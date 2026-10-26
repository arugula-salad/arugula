// Asks (Arugula #214): what a thread's agent session still needs from its
// asker, as a form where it was posted, in the thread and, sent there too,
// the channel. Its message is the platform's (no author, `via` `ask:repo`
// or `ask:<ses>`) and mentions its asker while it's open, so it's their
// unread mention and was pushed to them once; answered, it mentions nobody
// and draws as a line.
//
// - `ask:repo`: which repo `@arugula` works in, when the project's
//   workspaces have several and the ask named none. One button each
//   (`GET`/`POST /api/chat/rooms/:room/asks/:seq`), for its asker only;
//   the session starts on the best box with that repo, and its card comes
//   into the thread.
// - `ask:<ses>`: what the session's agent asks while it's `needs_input`:
//   the session's card's own form (`SessionAskForm`, `./session-card`), a
//   button per choice, and the agent's question in words where it takes
//   them. Its project's owners and editors answer it, as on the card.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../maintenance";
import type { ChatAsk, ChatMessage, ChatRoom, SessionAct, SessionView } from "../proto";
import { ChoiceForm, SessionAskForm, useSession } from "./session-card";

const enc = encodeURIComponent;

/** What an ask's message asks of (`repo`, or a session's id), if it's one. */
export function askOfMessage(m: ChatMessage): string | null {
  const v = m.via ?? "";
  if (m.author || !v.startsWith("ask:")) return null;
  const of = v.slice("ask:".length);
  return of === "repo" || of.startsWith("ses_") ? of : null;
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

/** An ask's message, in place of its text. */
export function AskMessage({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const of = askOfMessage(m)!;
  return (
    <div class="thread-msg first session-msg" data-msg={m.seq}>
      <div class="msg-gutter" />
      <div class="msg-body">{of === "repo" ? <RepoAsk room={room} m={m} /> : <SessionAsk ses={of} m={m} />}</div>
    </div>
  );
}

/** The ask's question without its typed fallback ("Reply with one: …"). */
function bare(text: string): string {
  return text.replace(/ Reply with one: .*$/s, "");
}

/** Which repo (`ask:repo`): its choices for its asker, and where it went. */
function RepoAsk({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const [a, setA] = useState<ChatAsk | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const path = `/api/chat/rooms/${enc(room.id)}/asks/${m.seq}`;
  // Read again when it's answered (it stops mentioning anyone).
  useEffect(() => {
    let live = true;
    call<ChatAsk>("GET", path).then(
      (v) => live && setA(v),
      (e: Error) => live && setSaid(e.message),
    );
    return () => void (live = false);
  }, [path, m.mentions.length]);
  const answer = async (choice: string) => {
    setBusy(true);
    try {
      setA(await call<ChatAsk>("POST", path, { choice }));
      setSaid(null);
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const open = a?.open ?? m.mentions.length > 0;
  if (!open) {
    return (
      <p class="session-ask-done dim" data-ask={m.seq} data-ask-state="answered">
        {bare(m.text)} <span>{a?.session ? "Answered." : "Asked again since."}</span>
      </p>
    );
  }
  const who = a?.asker_name || "its asker";
  return (
    <div class="session-card tone-warn session-ask" data-ask={m.seq} data-ask-state="open">
      <div class="session-card-top">
        <span class="session-card-state" data-ask-label>
          {a?.mine ? "asks you" : `asks ${who}`}
        </span>
        <span class="session-card-what">@arugula · which repo</span>
      </div>
      {a?.mine ? (
        <ChoiceForm question={bare(a.question)} choices={a.choices} words={false} busy={busy} answer={(c) => void answer(c)} />
      ) : (
        <>
          <p class="session-card-line session-ask-question">{bare(m.text)}</p>
          <p class="dim">{a ? `Waiting for ${who} to choose.` : "Reading it…"}</p>
        </>
      )}
      {said && (
        <p class="session-card-line bad" role="alert" data-ask-said>
          {said}
        </p>
      )}
    </div>
  );
}

/** What a session's agent asks (`ask:<ses>`): its card's form, here too. */
function SessionAsk({ ses, m }: { ses: string; m: ChatMessage }) {
  const { view, set } = useSession(ses);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const c = view?.card;
  const open = m.mentions.length > 0 && !!c?.open && !c.answered;
  if (!view || !c || !open) {
    return (
      <p class="session-ask-done dim" data-ask={m.seq} data-ask-state={view && !open ? "answered" : "loading"}>
        {m.text}
        {view && !open && <span> Answered.</span>}
      </p>
    );
  }
  const act = async (a: SessionAct) => {
    setBusy(true);
    try {
      set(await call<SessionView>("POST", `/api/sessions/${enc(ses)}/actions`, a));
      setSaid(null);
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const permission = c.open?.ask === "permission";
  return (
    <div class="session-card tone-warn session-ask" data-ask={m.seq} data-ask-state="open" data-ask-session={ses}>
      <div class="session-card-top">
        <span class="session-card-state" data-ask-label>
          {view.asked ? "asks you" : `asks ${c.asker_name || "its asker"}`}
        </span>
        <span class="session-card-what">
          @arugula · {permission ? "permission" : "question"} · {ses.replace(/^ses_/, "").slice(0, 8)}
        </span>
      </div>
      <SessionAskForm c={c} may={view.may} busy={busy} act={(a) => void act(a)} />
      {said && (
        <p class="session-card-line bad" role="alert" data-ask-said>
          {said}
        </p>
      )}
    </div>
  );
}
