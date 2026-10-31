// Who is in a room (#117, #118): the new-channel dialog (public or private),
// the channel browser, the group-DM picker, and the member tools in a room's
// details. The dialogs are this module's own layer, mounted once by the
// sidebar (`ChatDialogs`), on the shell's `.prompt` look.

import { useEffect, useState } from "preact/hooks";
import type { ChatChannel, ChatPerson, ChatRoom } from "../proto";
import type { ChatStore } from "../chat-store";
import { usePlatformApi, type PlatformApi, type PlatformTeam } from "../platform-api";
import { ConfirmRemove } from "../ui/confirm";
import type { MenuItem } from "../ui/menu";
import { MsgAvatar } from "./avatar";
import { closeOnEscape } from "./escape";
import "./members.css";

/** The most people in one DM, me included: control's `DM_PEOPLE`. */
export const DM_PEOPLE = 8;

type Dialog =
  | { kind: "channel"; store: ChatStore; team: string; name: string; api: PlatformApi }
  | { kind: "browse"; store: ChatStore; team: string; name: string; api: PlatformApi }
  | { kind: "dm"; store: ChatStore; team: PlatformTeam; api: PlatformApi };

let open: Dialog | null = null;
const listeners = new Set<() => void>();

function show(d: Dialog | null) {
  open = d;
  listeners.forEach((fn) => fn());
}

export const newChannelDialog = (store: ChatStore, team: string, name: string, api: PlatformApi) => show({ kind: "channel", store, team, name, api });
export const browseChannels = (store: ChatStore, team: string, name: string, api: PlatformApi) => show({ kind: "browse", store, team, name, api });
export const newDmDialog = (store: ChatStore, team: PlatformTeam, api: PlatformApi) => show({ kind: "dm", store, team, api });

/** The dialogs, whichever is open. */
export function ChatDialogs() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!open) return null;
  const d = open;
  const close = () => show(null);
  return (
    <div class="prompt-backdrop" role="dialog" aria-label={dialogTitle(d)} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      {d.kind === "channel" && <NewChannel d={d} close={close} />}
      {d.kind === "browse" && <Browser d={d} close={close} />}
      {d.kind === "dm" && <NewDm d={d} close={close} />}
    </div>
  );
}

function dialogTitle(d: Dialog): string {
  return d.kind === "channel" ? "New channel" : d.kind === "browse" ? "Browse channels" : "New direct message";
}

function NewChannel({ d, close }: { d: Extract<Dialog, { kind: "channel" }>; close: () => void }) {
  const [name, setName] = useState("");
  const [topic, setTopic] = useState("");
  const [priv, setPriv] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <form
      class="prompt members-dialog"
      aria-label={`New channel in ${d.name}`}
      data-new-channel
      onSubmit={async (e) => {
        e.preventDefault();
        if (!name.trim() || busy) return;
        setBusy(true);
        try {
          const r = await d.store.newChannel(d.team, name.trim(), { private: priv, topic: topic.trim() });
          close();
          d.api.nav.openRoom(r.id);
        } catch (err) {
          setBusy(false);
          d.api.toast(`Couldn't make it: ${(err as Error).message}`);
        }
      }}
    >
      <label>
        New channel in {d.name}
        <input autoFocus value={name} placeholder="one word, like general" data-new-channel-name onInput={(e) => setName((e.target as HTMLInputElement).value)} onKeyDown={closeOnEscape(close)} />
      </label>
      <label>
        Topic (optional)
        <input value={topic} placeholder="what it's for" onInput={(e) => setTopic((e.target as HTMLInputElement).value)} onKeyDown={closeOnEscape(close)} />
      </label>
      <div class="members-kind" role="radiogroup" aria-label="Who can find it">
        <label class="members-radio">
          <input type="radio" name="members-kind" checked={!priv} data-new-channel-public onChange={() => setPriv(false)} />
          <span>
            <b>Public</b>
            <small>Anyone in {d.name} can find it and join.</small>
          </span>
        </label>
        <label class="members-radio">
          <input type="radio" name="members-kind" checked={priv} data-new-channel-private onChange={() => setPriv(true)} />
          <span>
            <b>Private</b>
            <small>Only people you add see it; it isn't listed.</small>
          </span>
        </label>
      </div>
      <div class="prompt-buttons">
        <button class="secondary" type="button" onClick={close}>
          Cancel
        </button>
        <button type="submit" class="primary" disabled={busy || !name.trim()}>
          Create
        </button>
      </div>
    </form>
  );
}

/** The team's public channels, with Join; the ones I'm in open. */
function Browser({ d, close }: { d: Extract<Dialog, { kind: "browse" }>; close: () => void }) {
  const [q, setQ] = useState("");
  const [list, setList] = useState<ChatChannel[] | null>(null);
  const [failed, setFailed] = useState("");
  const load = () =>
    d.store
      .channels(d.team, q.trim())
      .then(setList)
      .catch((e: Error) => setFailed(e.message));
  useEffect(() => {
    const t = setTimeout(load, q ? 150 : 0);
    return () => clearTimeout(t);
  }, [q]);
  const go = async (c: ChatChannel) => {
    try {
      const r = c.joined ? d.store.room(c.id) : await d.store.join(c.id);
      close();
      d.api.nav.openRoom((r ?? { id: c.id }).id);
    } catch (e) {
      d.api.toast(`Couldn't join it: ${(e as Error).message}`);
    }
  };
  return (
    <div class="prompt members-dialog" aria-label={`Channels in ${d.name}`} data-channel-browser>
      <label>
        Channels in {d.name}
        <input autoFocus value={q} placeholder="search by name or topic" onInput={(e) => setQ((e.target as HTMLInputElement).value)} onKeyDown={closeOnEscape(close)} />
      </label>
      {failed && <p class="chat-none">{failed}</p>}
      {list && list.length === 0 && <p class="chat-none">{q ? "No public channel matches." : "No public channels yet."}</p>}
      <ul class="members-list">
        {(list ?? []).map((c) => (
          <li key={c.id} data-browse-channel={c.name}>
            <div class="members-what">
              <b>#{c.name}</b>
              {c.archived && <span class="dim"> archived</span>}
              <small>{c.topic || "No topic"}</small>
              <small>
                {c.members} {c.members === 1 ? "member" : "members"}
              </small>
            </div>
            {c.joined ? (
              <button type="button" data-browse-open onClick={() => void go(c)}>
                Open
              </button>
            ) : (
              <button type="button" class="primary" data-browse-join onClick={() => void go(c)}>
                Join
              </button>
            )}
          </li>
        ))}
      </ul>
      <div class="prompt-buttons">
        <button class="secondary" type="button" onClick={close}>
          Close
        </button>
      </div>
    </div>
  );
}

/** Up to seven others of the team to write to: one is a DM, more a group's. */
function NewDm({ d, close }: { d: Extract<Dialog, { kind: "dm" }>; close: () => void }) {
  const t = d.team;
  const me = d.store.me;
  const people = t.roster.members.filter((m) => m.account !== me).map((m) => ({ account: m.account, name: t.names?.[m.account] || m.name }));
  const [picked, setPicked] = useState<string[]>([]);
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const word = q.trim().toLowerCase();
  const shown = people.filter((p) => !word || p.name.toLowerCase().includes(word));
  const full = picked.length >= DM_PEOPLE - 1;
  const toggle = (a: string) => setPicked(picked.includes(a) ? picked.filter((x) => x !== a) : full ? picked : [...picked, a]);
  const start = async () => {
    if (!picked.length || busy) return;
    setBusy(true);
    try {
      const r = await d.store.newDm(t.team, picked);
      close();
      d.api.nav.openRoom(r.id);
    } catch (e) {
      setBusy(false);
      d.api.toast(`Couldn't open it: ${(e as Error).message}`);
    }
  };
  return (
    <form
      class="prompt members-dialog"
      aria-label={`Message people in ${t.roster.name}`}
      data-new-dm
      onSubmit={(e) => {
        e.preventDefault();
        void start();
      }}
    >
      <label>
        Message people in {t.roster.name}
        <input autoFocus value={q} placeholder="search their names" onInput={(e) => setQ((e.target as HTMLInputElement).value)} onKeyDown={closeOnEscape(close)} />
      </label>
      {people.length === 0 && <p class="chat-none">Nobody else is in {t.roster.name} yet.</p>}
      <ul class="members-list">
        {shown.map((p) => (
          <li key={p.account} data-dm-person={p.account}>
            <label class="members-radio">
              <input type="checkbox" checked={picked.includes(p.account)} disabled={full && !picked.includes(p.account)} onChange={() => toggle(p.account)} />
              <MsgAvatar who={p.account} name={p.name} account={p.account} dot />
              <span>{p.name}</span>
            </label>
          </li>
        ))}
      </ul>
      <p class="chat-none members-hint">
        {picked.length > 1 ? `A group of ${picked.length + 1}, named after you all.` : "Pick up to seven; more than one makes a group."}
        {full && " That's the most."}
      </p>
      <div class="prompt-buttons">
        <button class="secondary" type="button" onClick={close}>
          Cancel
        </button>
        <button type="submit" class="primary" disabled={busy || !picked.length} data-dm-start>
          {picked.length > 1 ? "Start group" : "Open"}
        </button>
      </div>
    </form>
  );
}

/** What a DM is called: the name it was given, else its other people's. */
export function dmLabel(r: ChatRoom): string {
  if (r.name) return r.name;
  const names = r.others.map((p) => p.name).filter(Boolean);
  return names.length ? names.join(", ") : r.with?.name || "someone who left";
}

/** What leaving a channel or a DM is called, or null for a room you
 * can't leave. */
function leaveLabel(room: ChatRoom): string | null {
  if (room.kind === "channel") return `Leave #${room.name}`;
  return room.kind === "dm" ? "Leave this conversation" : null;
}

/** Leave a channel or a DM; the room goes from my list. */
function leave(store: ChatStore, room: ChatRoom, api: PlatformApi) {
  void store
    .leave(room.id)
    .then(() => api.nav.openChat())
    .catch((e: Error) => api.toast(e.message));
}

/** Leaving, as an item of the room's menu (UI audit #169: ⎋ in the header
 * read as refresh, between the bell and Details). */
export function leaveItem(store: ChatStore, room: ChatRoom, api: PlatformApi): MenuItem | null {
  const label = leaveLabel(room);
  return label ? { label, danger: true, run: () => leave(store, room, api) } : null;
}

/** Leaving, in words, at the foot of the room's Details. */
export function LeaveButton({ store, room }: { store: ChatStore; room: ChatRoom }) {
  const api = usePlatformApi();
  const label = leaveLabel(room);
  if (!label) return null;
  return (
    <button class="secondary danger small members-leave" data-room-leave onClick={() => leave(store, room, api)}>
      {label}
    </button>
  );
}

/** A channel's creator and its team's owners add and remove its members;
 * a DM's people are fixed (adding someone is a new DM). */
export function MemberTools({ store, room, people }: { store: ChatStore; room: ChatRoom; people: ChatPerson[] }) {
  const api = usePlatformApi();
  const [adding, setAdding] = useState(false);
  // Taking someone out asks first (UI audit #158).
  const [removing, setRemoving] = useState<ChatPerson | null>(null);
  const manage = room.kind === "channel" && store.canManage(room.id);
  const team = api.teams().find((t) => t.team === room.team);
  const here = new Set(people.map((p) => p.account));
  const others = (team?.roster.members ?? []).filter((m) => !here.has(m.account)).map((m) => ({ account: m.account, name: team?.names?.[m.account] || m.name }));
  const fail = (e: Error) => api.toast(e.message);
  return (
    <>
      <ul class="chat-people">
        {people.map((p) => (
          <li key={p.account} data-room-person={p.account}>
            {/* Their picture and presence, as the header's strip has them (UI audit #169). */}
            <MsgAvatar who={p.account} name={p.name} account={p.account} dot />
            <span>{p.name}</span>
            {manage && p.account !== store.me && (
              <button class="secondary danger small members-remove" aria-label={`Remove ${p.name}`} data-member-remove={p.account} onClick={() => setRemoving(p)}>
                Remove
              </button>
            )}
          </li>
        ))}
      </ul>
      {manage && (
        <>
          <button class="secondary small members-add" data-member-add onClick={() => setAdding(!adding)} aria-expanded={adding}>
            Add people
          </button>
          {adding && (
            <ul class="chat-people members-addable" data-member-addable>
              {others.length === 0 && <li class="chat-none">Everyone in the team is in.</li>}
              {others.map((p) => (
                <li key={p.account}>
                  <MsgAvatar who={p.account} name={p.name} account={p.account} dot />
                  <span>{p.name}</span>
                  <button class="secondary small" data-member-invite={p.account} onClick={() => void store.addMembers(room.id, [p.account]).catch(fail)}>
                    Add
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      {room.kind === "dm" && room.others.length > 0 && team && (
        <button class="secondary small members-add" data-dm-add onClick={() => newDmDialog(store, team, api)}>
          New conversation with more people
        </button>
      )}
      {removing && (
        <ConfirmRemove
          title={`Remove ${removing.name} from #${room.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            const who = removing.account;
            setRemoving(null);
            void store.removeMember(room.id, who).catch(fail);
          }}
        >
          <p>They stop seeing it at once. Anyone who manages the channel can add them back.</p>
        </ConfirmRemove>
      )}
    </>
  );
}
