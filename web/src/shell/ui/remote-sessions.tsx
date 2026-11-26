// Who reached this machine through Arugula cloud (Arugula #1094, aru-141):
// the log the daemon keeps itself (`remote_sessions.list`, from
// `remote-sessions.jsonl` in its state directory), in Settings › Machines'
// Access and sharing (and a daemon's own `#settings`), with the
// remote-input switch it goes with. Its
// owner's alone. One quiet row per session, newest first. A daemon from
// before the route answers 404, and then the section isn't there.
//
// Remote typing (Arugula #1194), that switch, is a part of Access and
// sharing of its own, read-only: only the machine changes it.

import { useEffect, useState } from "preact/hooks";
import type { RemoteSession, RemoteSessions } from "../../shared/proto.gen";
import { Section } from "../../shared/ui/settings-section";
import { Chip, StateChip } from "../../shared/ui/state-chip";
import { Time } from "../../shared/ui/time";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";

/** How long a session lasted, in a few characters. */
function lasted(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function RemoteLine({ s }: { s: RemoteSession }) {
  const role = s.role === "device" ? "device key" : s.role;
  return (
    <li data-remote-session={s.open ? "open" : "closed"}>
      <b>{s.name || s.account}</b>
      <span class="dim">
        {role} · {s.path === "relay" ? "relayed" : s.path}
      </span>
      <span class="dim remote-session-when">
        <Time at={s.opened_ms} title="Opened" />
        {s.open ? (
          <>
            {" "}
            <StateChip tone="ok">Open</StateChip>
          </>
        ) : s.closed_ms ? (
          <span title={`Closed ${new Date(s.closed_ms).toLocaleString()}`}>, for {lasted(s.closed_ms - s.opened_ms)}</span>
        ) : (
          ", until the daemon stopped"
        )}
      </span>
    </li>
  );
}

/** `typing`, false where Remote typing is a part beside it: then it doesn't say it too. */
export function RemoteSessionsSection({ client, typing = true }: { client: Client; typing?: boolean }) {
  const owner = !!client.state && !client.state.roles;
  const connected = client.connected;
  const [got, setGot] = useState<RemoteSessions | null>(null);
  useEffect(() => {
    if (!owner || !connected) return;
    let live = true;
    void (async () => {
      const res = await client.request(OPS["remote_sessions.list"].method, opPath("remote_sessions.list")).catch(() => null);
      // An older daemon (404), or any other failure: the section stays away.
      const v = res?.ok ? await res.json<RemoteSessions>().catch(() => null) : null;
      if (live) setGot(v && Array.isArray(v.sessions) ? v : null);
    })();
    return () => void (live = false);
  }, [client, owner, connected]);
  if (!owner || !got) return null;
  const { sessions } = got;
  const open = sessions.filter((s) => s.open).length;
  const summary = sessions.length ? `${sessions.length} ${sessions.length === 1 ? "session" : "sessions"}${open ? `, ${open} open` : ""}` : "No remote sessions";
  return (
    <Section
      id="remote-sessions"
      title="Remote sessions"
      summary={`${summary}${got.remote_input || !typing ? "" : " · watch only"}`}
      count={sessions.length}
      lede="Who reached this machine through Arugula cloud, newest first, from the record the machine keeps itself."
      more={
        <p>
          Each one is a line in <code>remote-sessions.jsonl</code> in the daemon's state directory; <code>arugulad sharing log</code> prints
          them there. Arugula cloud can't change it.
        </p>
      }
      hooks={{ "data-remote-sessions": "" }}
    >
      {sessions.length ? (
        <ul class="settings-rows">
          {sessions.map((s, i) => (
            <RemoteLine key={i} s={s} />
          ))}
        </ul>
      ) : (
        <p class="dim" data-remote-sessions-none>
          No one has reached this machine through Arugula cloud yet.
        </p>
      )}
      {typing && (
        <p class="dim" data-remote-input={got.remote_input ? "on" : "off"}>
          {got.remote_input ? (
            <>
              They can type here as their role allows. <code>arugulad sharing remote-input off</code>, on this machine, lets them only watch.
            </>
          ) : (
            <>
              Remote input is off: they can only watch. <code>arugulad sharing remote-input on</code>, on this machine, lets them type again.
            </>
          )}
        </p>
      )}
    </Section>
  );
}

/** Whether people who reach the machine through Arugula cloud may type
 * there (`/api/host`'s `features.remote_input`), as the machine menu's
 * read-only row says it, with the line on the machine that changes it
 * (Arugula #1194: only the machine does). A daemon from before it says
 * nothing, and the part isn't there. */
export function RemoteTypingSection({ client }: { client: Client }) {
  const on = client.features?.remote_input;
  if (on === undefined) return null;
  return (
    <Section
      id="remote-typing"
      title="Remote typing"
      summary={on ? "Remote typing on" : "Remote typing off"}
      lede="Whether people who reach this machine through Arugula cloud may type in its panes. Only the machine changes it."
      hooks={{ "data-remote-typing": on ? "on" : "off" }}
    >
      <p>
        <StateChip tone={on ? "ok" : null}>{on ? "On" : "Off"}</StateChip> <Chip kind="tag">read-only</Chip>
      </p>
      <p class="dim">
        {on ? (
          <>
            They type here as their role allows. To change it, on the machine: <code>arugulad sharing remote-input off</code>
          </>
        ) : (
          <>
            Nobody types here through Arugula cloud, its owner included: they watch. To change it, on the machine:{" "}
            <code>arugulad sharing remote-input on</code>
          </>
        )}
      </p>
    </Section>
  );
}
