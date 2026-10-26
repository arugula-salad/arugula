// Sign-in and account (#173): where you're signed in (sign one out, or
// everywhere), your passkeys (never the last way in), and deleting the
// account, after typing its login. Its usage reports switch is shared
// (../../shared/ui/account-telemetry.tsx), handed the account's answer.

import { useEffect, useState } from "preact/hooks";
import { UsageReports } from "../../shared/ui/account-telemetry";
import { agentName, api, shownName, type ControlSession } from "../control";
import { currentHand, lending, setLending } from "../hand";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { NameLine, RecoveryLeft } from "./recovery";

export interface SessionRow {
  id: string;
  created: number;
  expires: number;
  agent: string;
  /** The account's device it is, once its page said (null before). */
  device?: string | null;
  current: boolean;
}

interface Passkeys {
  passkeys: { id: string; created: number; agent: string; provider: string | null; used: number | null }[];
  github: string | null;
  ways: number;
}

interface DeletePreview {
  confirm: string;
  disband: { team: string; name: string; others: number }[];
  blockers: string[];
  pays: string[];
  machines: number;
  stays_in: string[];
}

const day = (ms: number) => new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

/** What a sign-in or passkey came from, in the words Devices uses for a
 * browser ("Safari on iPhone"). Sign-ins from before control kept the
 * browser have none. */
export function describeAgent(ua: string): string {
  if (!ua) return "An earlier sign-in";
  const name = agentName(ua);
  // UI audit #179: which program, by its own word ("curl", "node"): the
  // CLI signs its calls with its device key and keeps no sign-in, so a
  // sign-in from a program is a script's.
  const program = name === "A program" ? /\b(node|undici|curl|reqwest)\b/i.exec(ua)?.[1] : null;
  return program ? `A program (${program.toLowerCase()})` : name;
}

/** A sign-in's name: its device's, once its page said which it is, so
 * the same browser has one name in both lists. */
export function sessionName(s: ControlSession, x: SessionRow): string {
  const c = x.device ? s.trusted.get(x.device) : undefined;
  return c ? shownName(c.name) : describeAgent(x.agent);
}

/** What keeps a passkey (iCloud Keychain, Windows Hello...) when control
 * knows its maker, else the browser that added it (#208). Ones added
 * before either was kept are numbered. */
export function passkeyName(k: Passkeys["passkeys"][number], i: number): string {
  if (k.provider) return k.provider;
  if (k.agent) return `Passkey from ${describeAgent(k.agent)}`;
  return `Passkey ${i + 1}`;
}

export function AccountPanel({ s, close }: { s: ControlSession; close: () => void }) {
  const [sessions, setSessions] = useState<SessionRow[] | null>(null);
  const [keys, setKeys] = useState<Passkeys | null>(null);
  const [err, setErr] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  // The passkey whose Remove was clicked: a dialog asks.
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null);
  const [deleting, setDeleting] = useState(false);
  const load = () => {
    api<{ sessions: SessionRow[] }>("/api/me/sessions").then((r) => setSessions(r.sessions), (e: Error) => setErr(e.message));
    api<Passkeys>("/api/me/passkeys").then(setKeys, (e: Error) => setErr(e.message));
  };
  useEffect(load, []);
  const act = (f: () => Promise<unknown>) => {
    setErr("");
    setConfirming(null);
    f().then(load, (e: Error) => setErr(e.message));
  };
  if (deleting) return <DeleteAccount s={s} back={() => setDeleting(false)} />;
  const others = (sessions ?? []).filter((x) => !x.current).length;
  // Sign-ins from before control kept the browser all read "An earlier
  // sign-in": folded into a count by Sign out everywhere (UI audit #174:
  // ten rows of them).
  const earlier = (x: SessionRow) => !x.current && !x.agent && !(x.device && s.trusted.has(x.device));
  const unnamed = (sessions ?? []).filter(earlier).length;
  return (
    <>
      <h2>Sign-in and account</h2>
      <p class="dim">
        Signed in as <b>{s.login}</b>
        {keys?.github ? ` (GitHub: ${keys.github})` : ""}.
      </p>
      <NameLine s={s} />
      <RecoveryLeft s={s} />
      <h3 class="control-group">Where you're signed in</h3>
      <ul class="control-devices control-rows" data-sessions>
        {(sessions ?? []).filter((x) => !earlier(x)).map((x) => (
          <li key={x.id} data-session={x.id}>
            <span>
              {sessionName(s, x)}
              {x.current ? " (this browser)" : ""}
            </span>
            <span class="dim">since {day(x.created)}</span>
            {x.current ? (
              <span />
            ) : (
              <button
                class={confirming === x.id ? "primary danger small" : "secondary danger small"}
                data-end-session={x.id}
                onClick={() => (confirming === x.id ? act(() => api(`/api/me/sessions/${x.id}/end`, {})) : setConfirming(x.id))}
              >
                {confirming === x.id ? "Really sign out?" : "Sign out"}
              </button>
            )}
          </li>
        ))}
      </ul>
      <p class="dim">
        {others ? `${others} other session${others === 1 ? "" : "s"}${unnamed ? `, ${unnamed} of them from before control kept which browser` : ""}. ` : "Nowhere else. "}
        <button
          class={confirming === "all" ? "link danger" : "link"}
          data-end-all
          onClick={() => {
            if (confirming !== "all") return setConfirming("all");
            api("/api/me/sessions/end-all", {}).then(
              () => void s.signOut(false),
              (e: Error) => setErr(e.message),
            );
          }}
        >
          {confirming === "all" ? "Sign out everywhere, here too?" : "Sign out everywhere"}
        </button>
      </p>
      <p class="dim">Devices and machines lists the same browsers, each with whether it's signed in.</p>
      {s.info.passkeys || keys?.passkeys.length ? (
        <>
          <h3 class="control-group">Passkeys</h3>
          {keys && keys.passkeys.length === 0 ? <p class="dim">None.</p> : null}
          <ul class="control-devices control-rows" data-passkeys>
            {(keys?.passkeys ?? []).map((k, i) => (
              <li key={k.id} data-passkey={k.id}>
                <span data-passkey-name>{passkeyName(k, i)}</span>
                <span class="dim">
                  added {day(k.created)}
                  {k.used ? `, last used ${day(k.used)}` : ""}
                </span>
                <button
                  class="secondary danger small"
                  data-remove-passkey={k.id}
                  disabled={(keys?.ways ?? 0) <= 1}
                  title={(keys?.ways ?? 0) <= 1 ? "Your only way to sign in" : undefined}
                  onClick={() => setRemoving({ id: k.id, name: passkeyName(k, i) })}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
          {removing ? (
            <ConfirmRemove
              title={`Remove ${removing.name}?`}
              cancel={() => setRemoving(null)}
              go={() => {
                const id = removing.id;
                setRemoving(null);
                act(async () => {
                  await api(`/api/me/passkeys/${encodeURIComponent(id)}/remove`, {});
                  await s.refreshMe();
                });
              }}
            >
              <p>It stops signing you in at once. To use it again, add it as a new passkey.</p>
            </ConfirmRemove>
          ) : null}
          {s.info.passkeys ? (
            <button class="secondary" data-add-passkey onClick={() => act(() => s.addPasskey())}>
              Add a passkey
            </button>
          ) : null}
        </>
      ) : null}
      <LendDevice s={s} />
      <UsageReports telemetry={s.telemetry} setTelemetry={(on) => s.setTelemetry(on)} />
      <h3 class="control-group">Delete your account</h3>
      <p class="dim">
        Removes your account from this control, with your devices, machines, sessions and passkeys.{" "}
        <button class="link danger" data-delete-account onClick={() => setDeleting(true)}>
          Delete account…
        </button>
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={close}>Done</button>
      </div>
    </>
  );
}

function DeleteAccount({ s, back }: { s: ControlSession; back: () => void }) {
  const [p, setP] = useState<DeletePreview | null>(null);
  const [typed, setTyped] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api<DeletePreview>("/api/me/delete").then(setP, (e: Error) => setErr(e.message));
  }, []);
  const blocked = !p || p.blockers.length > 0 || p.pays.length > 0;
  const matches = !!p && typed.trim().toLowerCase() === p.confirm.trim().toLowerCase();
  return (
    <>
      <h2>Delete your account</h2>
      {p ? (
        <>
          <p>This can't be undone. Control forgets:</p>
          <ul class="control-steps" data-delete-what>
            <li>your account, your passkeys, its link to your GitHub account, and every session;</li>
            <li>
              your devices, and your {p.machines} machine{p.machines === 1 ? "" : "s"}: they stop reaching control (Arugula keeps running on them, reachable only locally);
            </li>
            {p.disband.map((t) => (
              <li key={t.team} data-disband={t.team}>
                the team <b>{t.name}</b>, which you founded{t.others ? `: its ${t.others} other member${t.others === 1 ? " loses" : "s lose"} it, and its machines go back to their owners` : ""};
              </li>
            ))}
            <li>your push subscriptions, usage counts and team requests.</li>
          </ul>
          {p.stays_in.length ? (
            <p class="dim">
              You stay listed in {p.stays_in.join(", ")} until an owner removes you; with no devices behind your name, it lets nothing in. Its owners are told.
            </p>
          ) : null}
          {p.blockers.length || p.pays.length ? (
            <div class="control-error" data-delete-blocked>
              <p>First:</p>
              <ul>
                {p.blockers.map((b) => (
                  <li key={b}>{b}.</li>
                ))}
                {p.pays.map((b) => (
                  <li key={b}>Cancel {b}.</li>
                ))}
              </ul>
            </div>
          ) : (
            <label class="control-confirm">
              Type <b>{p.confirm}</b> to confirm:
              <input
                type="text"
                data-delete-confirm
                autocomplete="off"
                spellcheck={false}
                value={typed}
                onInput={(e) => setTyped((e.target as HTMLInputElement).value)}
              />
            </label>
          )}
        </>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={back}>Back</button>
        <button
          class="primary danger"
          data-delete-go
          disabled={blocked || !matches || busy}
          onClick={() => {
            setBusy(true);
            setErr("");
            api("/api/me/delete", { confirm: typed }).then(
              () => void s.signOut(true),
              (e: Error) => {
                setBusy(false);
                setErr(e.message);
              },
            );
          }}
        >
          Delete my account
        </button>
      </div>
    </>
  );
}

/** S33: this device's tools for agents on your machines. */
function LendDevice({ s }: { s: ControlSession }) {
  const [on, setOn] = useState(lending());
  const [, tick] = useState(0);
  useEffect(() => {
    const h = currentHand();
    if (!h) return;
    const fn = () => tick((n) => n + 1);
    h.listeners.add(fn);
    return () => void h.listeners.delete(fn);
  }, [on]);
  const reached = currentHand()?.connected() ?? 0;
  return (
    <>
      <h3 class="control-group">Lend this device to agents</h3>
      <p class="dim">
        Agents on your machines may ask to use this device: its location, camera, microphone, clipboard, or you. You allow or
        deny each request here, while this page is open.
      </p>
      <label class="control-check">
        <input
          type="checkbox"
          data-lend-device
          checked={on}
          onChange={(e) => {
            const v = (e.target as HTMLInputElement).checked;
            setLending(s, v);
            setOn(v);
          }}
        />{" "}
        Lend this device{on ? ` (reaching ${reached} machine${reached === 1 ? "" : "s"})` : ""}
      </label>
    </>
  );
}
