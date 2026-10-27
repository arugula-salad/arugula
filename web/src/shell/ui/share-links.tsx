// The Share dialog's links and guests (native #245): the read-only links
// and ssh invites to this session's panes that still work, each with
// Revoke, and the access log. What `arugula shares`, `arugula guests` and
// `arugula access log` show (`shares.list`, `guests.list`, `acl.get`'s
// `audit`), for one session. All are the owner's, as the dialog is; a
// machine with Guest SSH access off has no guests to list, and says
// nothing of them.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { GuestInvite, PaneId, SessionId, Share } from "../../shared/proto.gen";
import { Time } from "../../shared/ui/time";
import { paneName } from "./shell/overview";

/** One line of `GET /api/acl`'s `audit` (crates/mux/src/acl.rs). */
interface AuditLine {
  at: number;
  action: string;
  session?: SessionId;
  principal?: string;
  name?: string;
  role?: string | null;
  by?: string;
}

type Live = { kind: "share"; s: Share } | { kind: "guest"; g: GuestInvite };

const errorOf = async (res: { status: number; json<T>(): Promise<T> }) =>
  (await res.json<{ error?: string }>().catch(() => null))?.error ?? `HTTP ${res.status}`;

/** The live links and guests to `session`'s panes, with Revoke. */
export function ShareLinks({ client, session }: { client: Client; session: SessionId }) {
  const [live, setLive] = useState<Live[] | null>(null);
  const [err, setErr] = useState("");
  const [log, setLog] = useState(false);
  const ours = (pane: PaneId) => client.sessionOfPane(pane) === session;

  const load = async () => {
    const out: Live[] = [];
    try {
      const s = await client.request(OPS["shares.list"].method, opPath("shares.list"));
      if (s.ok) for (const x of await s.json<Share[]>()) if (ours(x.pane)) out.push({ kind: "share", s: x });
      // Off (404) or not built (501): no guests here, and nothing to say.
      const g = await client.request(OPS["guests.list"].method, opPath("guests.list"));
      if (g.ok) for (const x of await g.json<GuestInvite[]>()) if (ours(x.pane) && !(x.used && !x.sessions)) out.push({ kind: "guest", g: x });
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
    setLive(out);
  };
  useEffect(() => void load(), [client, session]);

  const revoke = async (l: Live) => {
    setErr("");
    const res =
      l.kind === "share"
        ? await client.request(OPS["share.revoke"].method, opPath("share.revoke", { id: l.s.id }))
        : await client.request(OPS["guest.revoke"].method, opPath("guest.revoke", { id: l.g.id }));
    if (!res.ok && res.status !== 404) setErr(await errorOf(res));
    await load();
  };

  const name = (pane: PaneId) => {
    const info = client.info(pane);
    return info ? paneName(client, info) : `pane %${pane}`;
  };

  return (
    <div class="share-links" data-share-links>
      {live && live.length > 0 && (
        <>
          <h3>Links and guests</h3>
          <ul class="share-list share-live">
            {live.map((l) => {
              const id = l.kind === "share" ? `share:${l.s.id}` : `guest:${l.g.id}`;
              const pane = l.kind === "share" ? l.s.pane : l.g.pane;
              const expires = l.kind === "share" ? l.s.expires_ms : l.g.expires_ms;
              const what =
                l.kind === "share"
                  ? "Read-only link"
                  : `ssh guest${l.g.label ? ` ${l.g.label}` : ""}, ${l.g.rw ? "may type" : "watches"}${l.g.sessions ? `, ${l.g.sessions} connected` : ""}`;
              return (
                <li key={id} data-share-live={id} data-share-live-pane={pane}>
                  <span class="share-name" title={`Pane %${pane}`}>
                    {what} · {name(pane)}
                  </span>
                  <span class="dim">
                    ends <Time at={expires} />
                  </span>
                  <button class="secondary danger small" data-revoke={id} onClick={() => void revoke(l)}>
                    Revoke
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      )}
      <p>
        <button class="link" data-access-log-open aria-expanded={log} onClick={() => setLog(!log)}>
          Access log
        </button>{" "}
        <span class="dim">(who was given or lost this session, and when)</span>
      </p>
      {log && <AccessLog client={client} session={session} />}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

/** `arugula access log` for one session, newest first. */
function AccessLog({ client, session }: { client: Client; session: SessionId }) {
  const [lines, setLines] = useState<AuditLine[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    let on = true;
    void (async () => {
      try {
        const r = await client.request(OPS["acl.get"].method, opPath("acl.get"));
        if (!r.ok) throw new Error(await errorOf(r));
        const v = await r.json<{ audit?: AuditLine[] }>();
        if (on) setLines((v.audit ?? []).filter((e) => e.session === session).reverse());
      } catch (e) {
        if (on) setErr(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      on = false;
    };
  }, [client, session]);
  if (err) return <p class="control-error">{err}</p>;
  if (!lines) return <p class="dim">Reading the log…</p>;
  if (!lines.length)
    return (
      <p class="dim" data-access-log-empty>
        Nobody has been given or lost this session yet.
      </p>
    );
  return (
    <ul class="access-log" data-access-log>
      {lines.slice(0, 50).map((e, i) => (
        <li key={`${e.at}-${i}`} data-access-action={e.action}>
          <Time at={e.at} on />
          <span>{e.action === "grant" ? `${e.role ?? "access"} for` : e.action === "link" ? "read-only link" : e.action}</span>
          <span class="share-name" title={e.principal}>
            {e.action === "link" ? "" : (e.name || e.principal)}
          </span>
          {e.by && e.by !== "owner" && <span class="dim">({e.by})</span>}
        </li>
      ))}
    </ul>
  );
}
