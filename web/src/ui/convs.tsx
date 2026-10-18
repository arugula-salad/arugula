// Conversations (#855) on the chat page: threads of their own, kept on
// their home machine, with members of their own. The page lists them under
// *Channels*, above the machines; this is what's theirs alone: making one,
// its members (with what being a member means), adding and taking out
// people, renaming and deleting it. Their messages are any thread's
// (ThreadBody, `conv-c-…`).

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { CONV_MEMBERS_SEE, type Conversation } from "../proto";
import { ConfirmRemove } from "./confirm";
import { Avatar, getControlSession } from "./people";
import { personPin, pinPerson } from "../control";
import type { Source } from "./places";
import { useEscape } from "./keys";

/** Where a new conversation can live: the machines you own here, online. */
export function homes(all: Source[]): Source[] {
  return all.filter((s) => s.client.connected && s.client.ownsMachine());
}

/** A one-line form over the page: what the dialogs here share. */
function Dialog({ label, children, busy, ok, submit, cancel, error }: {
  label: string;
  children: preact.ComponentChildren;
  busy: boolean;
  ok: string;
  submit: () => void;
  cancel: () => void;
  error: string | null;
}) {
  useEscape("conv-dialog.cancel", cancel);
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && cancel()}>
      <form
        class="prompt conv-dialog"
        aria-label={label}
        data-conv-dialog
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) submit();
        }}
      >
        <h2>{label}</h2>
        {children}
        {error && <p class="thread-error">{error}</p>}
        <div class="prompt-buttons">
          <button type="button" onClick={cancel}>
            Cancel
          </button>
          <button type="submit" class="primary" disabled={busy}>
            {ok}
          </button>
        </div>
      </form>
    </div>
  );
}

/** *New conversation*: a title, and its home: the machine you're on, or
 * another of yours that's online. */
export function NewConv({ all, done, cancel }: { all: Source[]; done: (s: Source, c: Conversation) => void; cancel: () => void }) {
  const choices = homes(all);
  const [title, setTitle] = useState("");
  const [home, setHome] = useState((choices.find((s) => s.shown) ?? choices[0])?.host ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(choices.length ? null : "None of your machines here can keep one: make it on a machine of yours.");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const submit = async () => {
    const s = choices.find((x) => x.host === home);
    if (!s) return;
    setBusy(true);
    setError(null);
    try {
      done(s, await s.client.createConv(title));
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Dialog label="New conversation" ok="Create" busy={busy || !choices.length} submit={() => void submit()} cancel={cancel} error={error}>
      <label>
        Title
        <input ref={input} value={title} maxLength={100} placeholder="calc tests" data-conv-title onInput={(e) => setTitle((e.target as HTMLInputElement).value)} />
      </label>
      {choices.length > 1 && (
        <label class="conv-home">
          Kept on
          <select value={home} data-conv-home onChange={(e) => setHome((e.target as HTMLSelectElement).value)}>
            {choices.map((s) => (
              <option key={s.host} value={s.host}>
                {s.host || "this machine"}
              </option>
            ))}
          </select>
        </label>
      )}
      <p class="conv-note">
        It lives on {choices.length > 1 ? "that machine" : (choices[0]?.host || "this machine")}, and is there while it's online. You own it: you add the people in it.
      </p>
    </Dialog>
  );
}

/** *Add people…*: someone who can reach the home machine (a team's member,
 * or someone something there is shared with), by name or id, from now on or
 * with its history. The invite says what members can do. Someone who can't
 * reach it yet (#856), a teammate or anyone control finds by their login,
 * is added with their account's root as this browser has it: being a
 * member lets them in, for its conversations only. */
function AddMember({ client, conv, cancel }: { client: Client; conv: Conversation; cancel: () => void }) {
  const [who, setWho] = useState("");
  const [history, setHistory] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const inIt = new Set([conv.owner, ...conv.members.map((m) => m.who)]);
  const known = new Map<string, string>();
  for (const p of client.state?.presence ?? []) if (!inIt.has(p.who) && p.who !== "owner") known.set(p.who, p.name);
  // Teammates, from the rosters this browser checked: their roots too.
  const control = getControlSession();
  const teammates = new Map<string, { name: string; root: string }>();
  for (const t of control?.teams ?? []) {
    for (const m of t.roster.members) {
      const id = `account:${m.account}`;
      if (m.account !== control!.account && !inIt.has(id) && !known.has(id)) teammates.set(id, { name: m.name, root: m.root });
    }
  }
  const submit = async () => {
    setBusy(true);
    setError(null);
    const w = who.trim();
    try {
      const mate = teammates.get(w);
      if (mate) await client.addConvMember(conv.id, w, history, mate);
      else {
        try {
          await client.addConvMember(conv.id, w, history);
        } catch (e) {
          // Not someone who reaches it: find them on control, as a share does.
          if (!control || w.startsWith("account:") || !/can reach this machine/.test((e as Error).message)) throw e;
          const p = await control.person(w.replace(/^@/, ""));
          const pinned = personPin(p.account);
          if (pinned && pinned.root !== p.root) throw new Error(`${p.name}'s key isn't the one this browser has for them: check it with them in Share… first`);
          pinPerson(p.account, p.root);
          await client.addConvMember(conv.id, `account:${p.account}`, history, { name: p.name, root: p.root });
        }
      }
      cancel();
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Dialog label={`Add people to ${conv.title}`} ok="Add" busy={busy || !who.trim()} submit={() => void submit()} cancel={cancel} error={error}>
      <p class="conv-warning" data-conv-warning>
        {CONV_MEMBERS_SEE}
      </p>
      <label>
        Who
        <input ref={input} value={who} list="conv-people" placeholder="@sam, or their id" data-conv-who onInput={(e) => setWho((e.target as HTMLInputElement).value)} />
        <datalist id="conv-people">
          {[...known, ...[...teammates].map(([id, t]) => [id, t.name] as const)].map(([id, name]) => (
            <option key={id} value={id}>
              {name}
            </option>
          ))}
        </datalist>
      </label>
      {teammates.size > 0 && (
        <p class="conv-teammates">
          Teammates:{" "}
          {[...teammates].map(([id, t]) => (
            <button type="button" key={id} class="control-linkish" data-conv-teammate={id} onClick={() => setWho(id)}>
              {t.name}
            </button>
          ))}
        </p>
      )}
      <label class="control-check">
        <input type="checkbox" checked={history} data-conv-history onChange={(e) => setHistory((e.target as HTMLInputElement).checked)} /> With its history (else from now on)
      </label>
      <p class="conv-note">
        {control
          ? "Anyone in your teams, or anyone on Arugula by their login. Someone who can't reach this machine yet can once they're in, for its conversations only."
          : "They must reach this machine already: someone in your team, or someone a session here is shared with."}
      </p>
    </Dialog>
  );
}

function Rename({ client, conv, cancel }: { client: Client; conv: Conversation; cancel: () => void }) {
  const [title, setTitle] = useState(conv.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    try {
      await client.renameConv(conv.id, title);
      cancel();
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Dialog label="Rename conversation" ok="Rename" busy={busy} submit={() => void submit()} cancel={cancel} error={error}>
      <label>
        Title
        <input value={title} maxLength={100} data-conv-rename onInput={(e) => setTitle((e.target as HTMLInputElement).value)} />
      </label>
    </Dialog>
  );
}

function since(at: number): string {
  return new Date(at).toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The details panel's insides for a conversation: who's in it and what
 * that lets them do; for its owner, adding and taking out people, renaming
 * and deleting it; for a member, leaving. */
export function ConvMembers({ s, conv, gone }: { s: Source; conv: Conversation; gone: () => void }) {
  const c = s.client;
  const me = c.me();
  const owner = conv.owner === me;
  const [dialog, setDialog] = useState<"add" | "rename" | "delete" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const remove = async (who: string) => {
    setError(null);
    try {
      await c.removeConvMember(conv.id, who);
      if (who === me) gone();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <>
      <h3>Members</h3>
      <p class="conv-warning" data-conv-warning>
        {CONV_MEMBERS_SEE}
      </p>
      <ul class="chat-people" data-conv-members>
        <li data-conv-member={conv.owner}>
          <Avatar p={{ who: conv.owner, name: conv.owner_name || "owner" }} />
          <span>{conv.owner_name || "owner"}</span>
          <span class="dim">owner</span>
        </li>
        {conv.members.map((m) => (
          <li key={m.who} data-conv-member={m.who}>
            <Avatar p={{ who: m.who, name: m.name }} />
            <span>{m.name}</span>
            <span class="dim" title={m.history ? "Reads all of it" : "Reads from when they joined"}>
              {m.history ? "since the start" : `since ${since(m.joined)}`}
            </span>
            {(owner || m.who === me) && (
              <button class="conv-remove" title={m.who === me ? "Leave this conversation" : `Take ${m.name} out`} data-conv-remove={m.who} onClick={() => void remove(m.who)}>
                {m.who === me ? "Leave" : "Remove"}
              </button>
            )}
          </li>
        ))}
      </ul>
      {conv.members.length === 0 && <p class="chat-none">Just you, so far.</p>}
      {error && <p class="thread-error">{error}</p>}
      {owner && (
        <div class="conv-actions">
          <button class="primary" data-conv-add onClick={() => setDialog("add")}>
            Add people…
          </button>
          <button data-conv-rename-open onClick={() => setDialog("rename")}>
            Rename…
          </button>
          <button class="danger" data-conv-delete onClick={() => setDialog("delete")}>
            Delete…
          </button>
        </div>
      )}
      {dialog === "add" && <AddMember client={c} conv={conv} cancel={() => setDialog(null)} />}
      {dialog === "rename" && <Rename client={c} conv={conv} cancel={() => setDialog(null)} />}
      {dialog === "delete" && (
        <ConfirmRemove
          title={`Delete ${conv.title}?`}
          label="Delete"
          cancel={() => setDialog(null)}
          go={() => {
            setDialog(null);
            c.deleteConv(conv.id).then(gone, (e: Error) => setError(e.message));
          }}
        >
          <p>Its messages go with it, for everyone in it. This can't be undone.</p>
        </ConfirmRemove>
      )}
    </>
  );
}
