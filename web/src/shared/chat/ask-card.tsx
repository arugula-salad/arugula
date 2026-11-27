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
//   Answered there, its text gains a line, "<name> answered: <choice>"
//   (Arugula #215), drawn after the question.
// - `ask:<prp>`: what an agent session (the chief of staff) proposes to do
//   as its person (Arugula #1278): start an agent on an issue, or stop
//   sessions. Its card (`ProposalCard`) says what and why, with Approve
//   and Deny for that person only; answered, it's a line saying what
//   answering did. The Overview's Needs you draws the same card. A "Run
//   this board" card (`run_board`, Arugula #1416) says the board, where its
//   lead runs, which machines its issue agents may use and how many at
//   once, and when the grant lapses. A to-do (`todo`, Arugula #1462) is
//   answered as its Needs you row answers it (`TodoCard`): its what, why
//   and how in a well to copy, a note checked for secrets, and Done or
//   Can't through the Inbox (`POST /api/inbox/{prp}`); answered, it's the
//   receipt the row shows ("Done by you 3:41 PM · <note>"), read from its
//   record: the note is its person's alone.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../maintenance";
import { usePlatformApi } from "../platform-api";
import type { ChatAsk, ChatMessage, ChatRoom, ProposalRun, SessionAct, SessionView } from "../proto";
import { Bar, BarWhere } from "../ui/bar";
import { StateChip } from "../ui/state-chip";
import { Card } from "../ui/card";
import { Button } from "../kit/button";
import { receiptLine, type Receipt } from "../kit/need-row";
import { Well } from "../kit/well";
import { TodoAnswer } from "./todo-answer";
import { ChoiceForm, SessionAskForm, useSession } from "./session-card";

const enc = encodeURIComponent;

/** What an ask's message asks of (`repo`, a session's id, or a
 * proposal's), if it's one. */
export function askOfMessage(m: ChatMessage): string | null {
  const v = m.via ?? "";
  if (m.author || !v.startsWith("ask:")) return null;
  const of = v.slice("ask:".length);
  return of === "repo" || of.startsWith("ses_") || of.startsWith("prp_") ? of : null;
}

/** Where an ask is read and answered. */
const askPath = (room: string, seq: number) => `/api/chat/rooms/${enc(room)}/asks/${seq}`;

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
      <div class="msg-body">{of === "repo" ? <RepoAsk room={room} m={m} /> : of.startsWith("prp_") ? <ProposalAsk room={room} m={m} /> : <SessionAsk ses={of} m={m} />}</div>
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
  const path = askPath(room.id, m.seq);
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
    <Card
      class="session-card session-ask"
      tone="warn"
      head={
        <Bar
          sub
          lead={
            <StateChip tone="warn" data-ask-label>
              {a?.mine ? "asks you" : `asks ${who}`}
            </StateChip>
          }
          where={<BarWhere>@arugula · which repo</BarWhere>}
        />
      }
      data-ask={m.seq}
      data-ask-state="open"
    >
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
    </Card>
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
    // Answered from its form or the card, its text says by whom and what
    // under the question (Arugula #215).
    const [question, ...answer] = m.text.split("\n");
    return (
      <p class="session-ask-done dim" data-ask={m.seq} data-ask-state={view && !open ? "answered" : "loading"}>
        {question}
        {answer.length > 0 ? (
          <span data-ask-answer> · {answer.join(" ")}</span>
        ) : (
          view && !open && <span> Answered.</span>
        )}
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
    <Card
      class="session-card session-ask"
      tone="warn"
      head={
        <Bar
          sub
          lead={
            <StateChip tone="warn" data-ask-label>
              {view.asked ? "asks you" : `asks ${c.asker_name || "its asker"}`}
            </StateChip>
          }
          where={
            <BarWhere>
              @arugula · {permission ? "permission" : "question"} · {ses.replace(/^ses_/, "").slice(0, 8)}
            </BarWhere>
          }
        />
      }
      data-ask={m.seq}
      data-ask-state="open"
      data-ask-session={ses}
    >
      <SessionAskForm c={c} may={view.may} busy={busy} act={(a) => void act(a)} />
      {said && (
        <p class="session-card-line bad" role="alert" data-ask-said>
          {said}
        </p>
      )}
    </Card>
  );
}

/** A proposal's message (`ask:<prp>`): its card, once it's read. */
function ProposalAsk({ room, m }: { room: ChatRoom; m: ChatMessage }) {
  const [a, setA] = useState<ChatAsk | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const path = askPath(room.id, m.seq);
  // Read again when it's answered elsewhere (it stops mentioning anyone).
  useEffect(() => {
    let live = true;
    call<ChatAsk>("GET", path).then(
      (v) => live && setA(v),
      (e: Error) => live && setSaid(e.message),
    );
    return () => void (live = false);
  }, [path, m.mentions.length]);
  if (!a?.proposal) {
    return (
      <p class="session-ask-done dim" data-ask={m.seq} data-ask-state="loading" data-proposal>
        {m.text}
        {said && (
          <span class="bad" data-ask-said>
            {" "}
            {said}
          </span>
        )}
      </p>
    );
  }
  return <ProposalCard a={a} project={room.project} answered={setA} />;
}

/** A "Run this board" card's lines (Arugula #1416): the board, where its
 * lead runs, where its issue agents may and how many at once, and when the
 * grant lapses. With a repo, each machine's clone of it as the machine said
 * when the card was raised (Arugula #1458): a machine with none is said, as
 * is one that couldn't be asked. */
function RunLines({ run, project }: { run: ProposalRun; project: string | null }) {
  const board = project ? `/p/${enc(project)}/boards/${enc(run.board)}` : null;
  const repo = run.repo ?? null;
  const said = new Map((run.checkouts ?? []).map((c) => [c.machine, c.repo ?? null]));
  const lead = run.lead_machine;
  const leadAt = repo && said.has(lead.id) ? said.get(lead.id) : undefined;
  const others = run.machines.filter((m) => m.id !== lead.id);
  const without = repo ? others.filter((m) => said.has(m.id) && !said.get(m.id)) : [];
  const unasked = repo ? run.machines.filter((m) => !said.has(m.id)) : [];
  return (
    <>
      <p class="session-card-line session-ask-question" data-proposal-board={run.board}>
        Run {board ? <a href={board}>{run.board_title || run.board}</a> : <b>{run.board_title || run.board}</b>}
      </p>
      <p class={`session-card-line${leadAt === null ? " bad" : ""}`} data-proposal-lead={lead.id}>
        Lead on <b>{lead.name}</b>
        {leadAt ? ` · in ${leadAt}` : leadAt === null ? ` · no clone of ${repo} there, so it won't start` : ""}
      </p>
      <p class="session-card-line" data-proposal-machines>
        Issue agents on {run.machines.map((m) => m.name).join(", ")} · up to {run.cap} at once
      </p>
      {without.length > 0 && (
        <p class="session-card-line bad" data-proposal-no-clone>
          No clone of {repo} on {without.map((m) => m.name).join(", ")}: no issue agent starts there
        </p>
      )}
      {unasked.length > 0 && (
        <p class="session-card-line" data-proposal-unasked>
          Couldn't ask {unasked.map((m) => m.name).join(", ")} for a clone of {repo}
        </p>
      )}
      <p class="session-card-line" data-proposal-lapse>
        {lapseWords(run.lapse_days)}
      </p>
    </>
  );
}

/** When a grant lapses, as its card says it. */
export function lapseWords(days: number | null | undefined): string {
  if (!days) return "Until the board closes";
  return days === 1 ? "For 1 day, or until the board closes" : `For ${days} days, or until the board closes`;
}

/** What answering a proposal did, in a line: who, and what came of it. */
export function answeredLine(a: ChatAsk): string {
  const p = a.proposal!;
  const who = p.answered_by_name || "Someone";
  if (p.state === "denied") return `${who} denied it.`;
  if (p.state === "failed") return `${who} approved it, and it failed: ${p.outcome ?? "no reason given"}`;
  return `${who} approved: ${p.outcome ?? "done"}`;
}

/** "3:41 PM". */
export const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** An answered to-do's receipt from its record (#1462): Done or Can't, when,
 * and the note, which control sends its person alone (`a.mine`). */
export function todoReceipt(a: ChatAsk): Partial<Receipt> | null {
  const p = a.proposal;
  if (!p?.todo) return null;
  return {
    ...(p.state === "done" ? { what: "Done" } : p.state === "cant" ? { what: "Can't" } : {}),
    ...(p.todo.answered_at ? { at: clock(p.todo.answered_at) } : {}),
    ...(p.todo.note && a.mine ? { note: p.todo.note } : {}),
  };
}

/** What answering a to-do left, in a line: the row's receipt; cancelled, its agent's. */
function todoLine(a: ChatAsk): string {
  const p = a.proposal!;
  if (p.state === "cancelled") return "Its agent cancelled it.";
  const r = todoReceipt(a);
  return receiptLine({ what: r?.what ?? "Answered", by: a.mine ? "you" : p.answered_by_name || "someone", at: r?.at ?? "", note: r?.note });
}

/** An open to-do's card, for its person: what, why, how (copied as it
 * is), and its answer. Answered, the card reads its record again, which
 * draws it as its receipt. Anyone else waits. */
function TodoCard({ a, answered }: { a: ChatAsk; answered?: (a: ChatAsk) => void }) {
  const api = usePlatformApi();
  const [copied, setCopied] = useState(false);
  const p = a.proposal!;
  const how = p.todo?.how ?? "";
  const answer = async (choice: string, note: string) => {
    // Through the Inbox, as the row answers it; a page without one, the card's own route.
    if (api.inbox) await api.inbox.answer(p.id, choice, note);
    else await call<ChatAsk>("POST", askPath(a.room, a.seq), note.trim() ? { choice, note: note.trim() } : { choice });
    answered?.(await call<ChatAsk>("GET", askPath(a.room, a.seq)));
  };
  return (
    <Card
      class="session-card session-ask proposal-card"
      tone="warn"
      head={
        <Bar
          sub
          lead={
            <StateChip tone="warn" data-ask-label>
              {a.mine ? "waits on you" : `waits on ${a.asker_name || "its person"}`}
            </StateChip>
          }
          where={<BarWhere>To do</BarWhere>}
        />
      }
      data-ask={a.seq}
      data-ask-state="open"
      data-proposal={p.id}
      data-proposal-action="todo"
    >
      <p class="session-card-line session-ask-question" data-proposal-todo>
        {p.todo?.what ?? a.question}
      </p>
      {p.why && (
        <p class="session-card-line" data-proposal-why>
          <span class="dim">Why: </span>
          {p.why}
        </p>
      )}
      {how && (
        <Well label="how">
          <pre>{how}</pre>
          <div class="k-row-actions">
            <Button
              onClick={() => {
                void navigator.clipboard?.writeText(how);
                setCopied(true);
              }}
            >
              {copied ? "Copied" : "Copy"}
            </Button>
          </div>
        </Well>
      )}
      {a.mine ? <TodoAnswer options={a.choices.length ? a.choices : ["Done", "Can't"]} answer={answer} data-proposal-todo-answer /> : <p class="dim">Waiting for {a.asker_name || "its person"}.</p>}
    </Card>
  );
}

/** A proposal (Arugula #1278) as a card: what its agent wants to do as
 * its person and why, with Approve and Deny for them (`a.mine`); anyone
 * else waits. Answered, one dim line, linking the session it started.
 * `answered` hears the ask as answering left it; `open`, where there is
 * one, opens the room it was asked in (the Overview's staff tab). */
export function ProposalCard({ a, project, answered, open }: { a: ChatAsk; project: string | null; answered?: (a: ChatAsk) => void; open?: () => void }) {
  const api = usePlatformApi();
  // The choice being sent, while it is.
  const [busy, setBusy] = useState<"Approve" | "Deny" | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const p = a.proposal!;
  const action = p.action === "close" ? "close" : p.action === "run_board" ? "run_board" : "start";
  if (p.state !== "open" || !a.open) {
    // A machine's start is a block there (`%N`), with no page of its own.
    const started = p.started?.startsWith("ses_") && project ? api.sessionHref?.(project, p.started) : undefined;
    return (
      <p class="session-ask-done dim" data-ask={a.seq} data-ask-state="answered" data-proposal={p.id} data-proposal-action={action} data-proposal-state={p.state}>
        {p.action === "todo" ? todoLine(a) : answeredLine(a)}
        {started && (
          <>
            {" "}
            <a href={started} title="The session it started" data-proposal-started>
              Its session
            </a>
          </>
        )}
      </p>
    );
  }
  if (p.action === "todo") return <TodoCard a={a} answered={answered} />;
  const answer = async (choice: "Approve" | "Deny") => {
    setBusy(choice);
    try {
      const v = await call<ChatAsk>("POST", askPath(a.room, a.seq), { choice });
      setSaid(null);
      answered?.(v);
    } catch (e) {
      setSaid((e as Error).message);
    } finally {
      setBusy(null);
    }
  };
  const what = `Chief of staff · ${action === "close" ? "stop sessions" : action === "run_board" ? "run a board" : "start an agent"}`;
  const issue = p.issue;
  // A native issue's page, in its project where the page knows it.
  const issueHref = issue && (project ? `/p/${enc(project)}/issues/${enc(issue.id)}` : `/issues/${enc(issue.id)}`);
  return (
    <Card
      class="session-card session-ask proposal-card"
      tone="warn"
      head={
        <Bar
          sub
          lead={
            <StateChip tone="warn" data-ask-label>
              {a.mine ? "asks you" : `asks ${a.asker_name || "its person"}`}
            </StateChip>
          }
          where={
            <BarWhere>
              {open ? (
                <button type="button" class="link" title="Open the chief of staff's room" data-proposal-open onClick={open}>
                  {what}
                </button>
              ) : (
                what
              )}
            </BarWhere>
          }
        />
      }
      data-ask={a.seq}
      data-ask-state="open"
      data-proposal={p.id}
      data-proposal-action={action}
    >
      {action === "run_board" && p.run ? (
        <RunLines run={p.run} project={project} />
      ) : action === "start" ? (
        <>
          {issue && (
            <p class="session-card-line session-ask-question" data-proposal-issue={issue.number}>
              <a href={issueHref!}>
                #{issue.number} {issue.title}
              </a>
            </p>
          )}
          {p.repo && (
            <p class="session-card-line dim" data-proposal-repo>
              in <code>{p.repo}</code>
            </p>
          )}
          {p.prompt && (
            <p class="session-card-line" data-proposal-prompt>
              “{p.prompt}”
            </p>
          )}
        </>
      ) : (
        <ul class="proposal-sessions" data-proposal-sessions>
          {p.sessions.map((s) => {
            const page = project ? api.sessionHref?.(project, s.id) : undefined;
            return (
              <li key={s.id} data-proposal-session={s.id}>
                {page ? <a href={page}>{s.title}</a> : s.title} <span class="dim">· {s.state}</span>
              </li>
            );
          })}
        </ul>
      )}
      {p.why && (
        <p class="session-card-line" data-proposal-why>
          <span class="dim">Why: </span>
          {p.why}
        </p>
      )}
      {a.mine ? (
        <div class="session-ask-choices" role="group" aria-label={what}>
          <button type="button" class="primary small" disabled={!!busy} aria-busy={busy === "Approve" || undefined} data-proposal-approve onClick={() => void answer("Approve")}>
            {busy === "Approve" ? (action === "close" ? "Stopping…" : "Starting…") : "Approve"}
          </button>
          <button type="button" class="secondary small" disabled={!!busy} aria-busy={busy === "Deny" || undefined} data-proposal-deny onClick={() => void answer("Deny")}>
            Deny
          </button>
        </div>
      ) : (
        <p class="dim">Waiting for {a.asker_name || "its person"}.</p>
      )}
      {said && (
        <p class="session-card-line bad" role="alert" data-ask-said>
          {said}
        </p>
      )}
    </Card>
  );
}
