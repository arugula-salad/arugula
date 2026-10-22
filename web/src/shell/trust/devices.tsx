// Devices and machines (M17): every device and machine the account trusts,
// each labelled by kind, when it was added and who approved it (#759),
// whether a browser is signed in, the unused ones to clear out, a machine's
// team (*Move to…*, #100), and removing one. Also adding a phone or
// browser, the passkey nudge, and the words someone verifying you reads.

import { useEffect, useState } from "preact/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { Modal } from "../../shared/ui/modal";
import { api, shownName, type ControlSession } from "../control";
import { fingerprint, type Cert } from "../e2e/cert.ts";
import { verifyCode, verifyWords } from "../e2e/words.ts";
import { directory } from "../hosts";
import { machinePlace, machineState, placeMeans } from "../ui/hosts";
import type { SessionRow } from "./account";
import { addedAt, openPanel, QrCode, since } from "./common";
import { HostedMachines } from "./hosted";

/** #763: for someone sharing with you who wants to verify it's you: a QR
 * code for their camera, and the same as words for over the phone. From
 * the root this device pinned, not control's word for it. */
export function VerifyMe({ account, root }: { account: string; root: string }) {
  const [words, setWords] = useState<string[]>([]);
  useEffect(() => {
    void verifyWords(account, root).then(setWords);
  }, [account, root]);
  return (
    <details class="control-older" data-verify-me>
      <summary>Someone verifying you?</summary>
      <QrCode text={verifyCode(account, root)} label="QR code to verify your account" />
      <p class="dim">Or read them these words:</p>
      <p class="share-verify-words" data-verify-me-words>
        {words.join(" ")}
      </p>
    </details>
  );
}

/** The control URL as a QR code, a link to copy, and what to do (#105).
 * When the new device asks, DevicePrompt comes up over this. */
export function AddDevice({ s, close }: { s: ControlSession; close: () => void }) {
  return (
    <Modal close={close}>
      <h2>Add a phone or browser</h2>
      <QrCode text={s.info.url} label={`QR code for ${s.info.url}`} />
      <CopyText text={s.info.url} share data-control-url />
      <ol class="control-steps">
        <li>Open it on the phone or browser (scan the code, or send the link).</li>
        <li>Sign in the way you did here{s.login && s.login !== "you" ? `, as ${s.login}` : ""}.</li>
        <li>Approve it here: this browser asks. Scan the code it shows, or type it.</li>
      </ol>
      <div class="prompt-buttons">
        <button onClick={close}>Done</button>
      </div>
    </Modal>
  );
}

/** For a GitHub-only account: a passkey, so it isn't tied to GitHub. */
export function PasskeyNudge({ s }: { s: ControlSession }) {
  const [err, setErr] = useState("");
  if (!s.info.passkeys || s.passkeys > 0) return null;
  return (
    <p class="dim" data-passkey-nudge>
      Add a passkey so this account isn't tied to GitHub.{" "}
      <button class="control-linkish" data-add-passkey onClick={() => s.addPasskey().catch((e: Error) => setErr(e.name === "NotAllowedError" ? "Cancelled." : e.message))}>
        Add a passkey
      </button>
      {err ? <span class="control-error"> {err}</span> : null}
    </p>
  );
}

/** *Move to…* on a machine (#100): into a team you're in (#332), or back
 * to just you. Only shown when there's somewhere to move it. */
function MoveMachine({ s, c, team, online }: { s: ControlSession; c: Cert; team: string | null; online: boolean }) {
  const [to, setTo] = useState<string | null | undefined>(undefined);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const teams = s.addableTeams();
  if (teams.length === 0 && !team) return null;
  const name = (id: string | null) => (id ? s.teams.find((t) => t.team === id)?.roster.name ?? "a team" : "just you");
  return (
    <div class="control-move" data-move={c.device}>
      <label class="dim control-move-label">
        Team{" "}
        <select
          class="control-select"
          data-move-to
          value={to === undefined ? (team ?? "") : (to ?? "")}
          onChange={(e) => {
            const v = (e.target as HTMLSelectElement).value || null;
            setErr("");
            setTo(v === team ? undefined : v);
          }}
        >
          <option value="">Just me</option>
          {teams.map((t) => (
            <option key={t.team} value={t.team}>
              {t.roster.name}
            </option>
          ))}
          {team && !teams.some((t) => t.team === team) ? <option value={team}>{name(team)}</option> : null}
        </select>
      </label>
      {to !== undefined ? (
        <p class="dim control-explain" data-move-explain>
          {to
            ? `Everyone in ${name(to)} sees ${c.name} and reaches it by their role${team ? `, and ${name(team)}'s members lose it` : ""}; its owners also see private panes. It stays yours, and you can take it out again.`
            : `${name(team)}'s members lose ${c.name} at once; only your devices reach it.`}
          {/* #778: a machine is in one team at a time; sharing reaches the
              other team without taking it from this one. */}
          {to && team ? ` To work with both, keep it in ${name(team)} and share a session with ${name(to)} instead (Share, beside the session's name).` : ""}
          {online ? " " : " It's offline, so it moves when it next connects. "}
          <button
            class="primary"
            data-move-go
            disabled={busy}
            onClick={() => {
              setBusy(true);
              s.moveDaemon(c.device, to).then(
                () => {
                  setBusy(false);
                  setTo(undefined);
                },
                (e: Error) => {
                  setBusy(false);
                  setErr(e.message);
                },
              );
            }}
          >
            Move
          </button>{" "}
          <button data-move-cancel onClick={() => setTo(undefined)}>
            Cancel
          </button>
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

/** Whether a browser is signed in, with Sign out for one that isn't this
 * one. Nothing until the sign-ins load, or from a control that doesn't
 * say which sign-in is which device. */
function SignedIn({ sessions, end }: { sessions: SessionRow[] | null; end: (id: string) => void }) {
  if (!sessions) return null;
  if (!sessions.length) return <span class="dim" data-signed-in="no"> · signed out</span>;
  const first = Math.min(...sessions.map((x) => x.created));
  const here = sessions.some((x) => x.current);
  return (
    <span class="dim" data-signed-in="yes">
      {" "}
      · signed in since {new Date(first).toLocaleDateString(undefined, { month: "short", day: "numeric" })}
      {here ? null : (
        <>
          {" · "}
          <button class="control-linkish" data-sign-out-device onClick={() => sessions.forEach((x) => end(x.id))}>
            Sign out
          </button>
        </>
      )}
    </span>
  );
}

/** A device's kind, in a row's words. Machines are under their own
 * heading, so they don't say it. */
const kindText = (k: Cert["kind"]) => (k === "cli" ? "command line" : k === "recovery" ? "recovery code" : k === "daemon" ? "machine" : "browser or app");

/** What labels a device in a row, a dialog or a banner (#759): its kind
 * and when it was added. Its fingerprint labels it too, but nobody
 * compares it there, so it waits behind *Details*. */
function DeviceLabel({ s, c, kind = true }: { s: ControlSession; c: Cert; kind?: boolean }) {
  return (
    <span class="dim control-device-label">
      {kind ? `${kindText(c.kind)} · ` : ""}added <span data-added={c.created}>{addedAt(c.created)}</span> <DeviceDetails s={s} c={c} />
    </span>
  );
}

export function DeviceDetails({ s, c }: { s: ControlSession; c: Cert }) {
  return (
    <details class="control-details" data-details={c.device}>
      <summary>Details</summary>
      <span class="control-details-body">
        <span>
          Fingerprint <span class="fingerprint-inline">{fingerprint(c.device)}</span>
        </span>
        {/* Who approved it, while the account still trusts it: a banner's
            device may be one it doesn't. */}
        {s.trusted.has(c.device) ? <span>{approvedText(s, c)}</span> : null}
      </span>
    </details>
  );
}

/** When a device was added and which device approved it (#327), in its
 * row's *Details*. */
function approvedText(s: ControlSession, c: Cert): string {
  if (c.approver === c.device) return `Added ${since(c.created)}, the account's first device`;
  const by = s.trusted.get(c.approver);
  const on = !by ? "on a device since removed" : by.kind === "recovery" ? `with ${by.name}` : `on ${shownName(by.name)}${by.device === s.keys.id ? " (this browser)" : ""}`;
  return `Added ${since(c.created)}, approved ${on}`;
}

/** Not used for this long, and signed out: offered for removal. */
const UNUSED_MS = 30 * 24 * 3600 * 1000;

export function Devices({ s, close }: { s: ControlSession; close: () => void }) {
  const [err, setErr] = useState("");
  // The machine or device whose Remove was clicked: a dialog asks.
  const [removing, setRemoving] = useState<Cert | null>(null);
  const devices = [...s.trusted.values()].filter((c) => c.kind !== "recovery");
  const machines = devices.filter((c) => c.kind === "daemon");
  // The ones in use first.
  const browsers = devices
    .filter((c) => c.kind !== "daemon")
    .sort((a, b) => +(b.device === s.keys.id) - +(a.device === s.keys.id) || (s.used.get(b.device) ?? 0) - (s.used.get(a.device) ?? 0));
  const remove = (c: Cert) => (
    <button class="control-revoke quiet-danger" data-remove={c.device} onClick={() => setRemoving(c)}>
      Remove
    </button>
  );
  // Which browsers are signed in: a device and a sign-in are two things
  // (Remove takes the key away; Sign out only ends the sign-in), shown
  // on one row.
  const [sessions, setSessions] = useState<SessionRow[] | null>(null);
  const loadSessions = () =>
    void api<{ sessions: SessionRow[] }>("/api/me/sessions").then(
      (r) => setSessions(r.sessions),
      () => {},
    );
  useEffect(loadSessions, []);
  const endSession = (id: string) => api(`/api/me/sessions/${id}/end`, {}).then(loadSessions, (e: Error) => setErr(e.message));
  // Signed out (once the sign-ins have loaded), and not used in 30 days or
  // never seen used: what "clean up" offers. Never this browser.
  const unused = sessions
    ? browsers.filter((c) => {
        if (c.device === s.keys.id || sessions.some((x) => x.device === c.device)) return false;
        const at = s.used.get(c.device);
        return !at || Date.now() - at > UNUSED_MS;
      })
    : [];
  const [clearing, setClearing] = useState(false);
  const [showUnused, setShowUnused] = useState(false);
  const removeUnused = async () => {
    setClearing(false);
    try {
      for (const c of unused) await s.revoke(c.device);
      await s.refresh();
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <Modal close={close}>
      <h2>Devices and machines</h2>
      <p class="dim">
        Signed in as <b data-account={s.account}>{s.login}</b>.
      </p>
      {s.enrollment ? (
        <p class="dim">
          Your account's fingerprint:{" "}
          <span class="fingerprint-inline" data-account-fingerprint={s.enrollment.root}>
            {fingerprint(s.enrollment.root)}
          </span>
          , your first device's. Someone verifying you uses it (below), and an older machine may ask you to check it when it joins.
        </p>
      ) : null}
      {s.enrollment && !s.rootMismatch ? <VerifyMe account={s.account} root={s.enrollment.root} /> : null}
      {s.rootMismatch ? (
        <p class="control-error">Control reports a different first device for this account than this browser pinned. New devices and machines won't be trusted here.</p>
      ) : null}
      <h3 class="control-group">Machines</h3>
      <p class="dim">They run your terminals.</p>
      {machines.length === 0 ? <p class="dim">None yet.</p> : null}
      <ul class="control-devices" data-machines>
        {machines.map((c) => {
          const d = s.daemons.find((x) => x.id === c.device);
          const team = d?.team ? s.teams.find((t) => t.team === d.team)?.roster.name ?? "a team" : null;
          const path = d?.name === directory.current && directory.path ? directory.path : d?.urls.length ? "direct" : "relayed";
          return (
            <li key={c.device} data-device={c.device}>
              <span>
                {c.name}
                <span class="dim" data-status>
                  {" · "}
                  {d ? machineState(d.name) : "offline"}
                  {d?.online ? ` · ${path}` : ""}
                </span>
                <DeviceLabel s={s} c={c} kind={false} />
              </span>
              {remove(c)}
              {/* #778: its place and what that means, before a join or a
                  share elsewhere runs into it. */}
              <p class="dim control-explain" data-place={team ?? ""}>
                <b>{machinePlace(team)}</b>: {placeMeans(team)}
              </p>
              {/* M124: chat lives in control now; an older machine keeps
                  its pane and session threads to itself until it's updated. */}
              {d?.chat_update ? (
                <p class="dim control-explain" data-chat-update>
                  <b>Update to bring its threads into chat.</b> It's from before chat moved here, so its pane and session threads and
                  conversations stay on it until it runs the latest Arugula.
                </p>
              ) : null}
              <MoveMachine s={s} c={c} team={d?.team ?? null} online={!!d?.online} />
            </li>
          );
        })}
      </ul>
      <button data-add-machine onClick={openPanel("add")}>
        Add a machine…
      </button>
      <HostedMachines s={s} />
      <h3 class="control-group">Browsers and phones</h3>
      <p class="dim">They reach your machines.</p>
      {unused.length ? (
        <p class="dim control-unused" data-unused={unused.length}>
          {clearing ? (
            <>
              Remove {unused.length} unused {unused.length === 1 ? "device" : "devices"}? Their keys stop reaching your machines.{" "}
              <button class="control-revoke danger" data-remove-unused onClick={() => void removeUnused()}>
                Remove {unused.length}
              </button>{" "}
              <button class="control-linkish" onClick={() => setClearing(false)}>
                Cancel
              </button>
            </>
          ) : (
            <>
              {unused.length} more {unused.length === 1 ? "looks" : "look"} unused: signed out, and not used in 30 days.{" "}
              <button class="control-linkish" data-show-unused aria-expanded={showUnused} onClick={() => setShowUnused(!showUnused)}>
                {showUnused ? "Hide them" : "Show them"}
              </button>
              {" · "}
              <button class="control-linkish" data-clear-unused onClick={() => setClearing(true)}>
                Remove them…
              </button>
            </>
          )}
        </p>
      ) : null}
      <ul class="control-devices" data-browsers>
        {browsers.map((c) => {
          const at = s.used.get(c.device);
          const old = unused.includes(c);
          // Unused ones are folded into the line above until asked for.
          if (old && !showUnused) return null;
          return (
            <li key={c.device} data-device={c.device} class={old ? "unused" : undefined}>
              <span>
                {shownName(c.name)}
                {c.device === s.keys.id ? <b data-this-browser> (this browser)</b> : ""}
                <span class="dim" data-used={at ?? ""}>
                  {" · "}
                  {c.device === s.keys.id ? "using it now" : at ? `used ${since(at)}` : "last use unknown"}
                </span>
                {old ? <span class="control-badge unused-badge">unused</span> : null}
                <SignedIn sessions={sessions?.filter((x) => x.device === c.device) ?? null} end={endSession} />
                <DeviceLabel s={s} c={c} />
              </span>
              {c.device !== s.keys.id ? remove(c) : <span />}
            </li>
          );
        })}
      </ul>
      <button data-add-device onClick={openPanel("add-device")}>
        Add a phone or browser…
      </button>
      <p class="dim">
        Recovery codes, your name and passkeys are in{" "}
        <button class="control-linkish" data-to-account onClick={openPanel("account")}>
          Sign-in and account…
        </button>
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      {removing ? (
        <ConfirmRemove
          title={`Remove ${removing.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            setRemoving(null);
            s.revoke(removing.device).catch((e: Error) => setErr(e.message));
          }}
        >
          {removing.kind === "daemon" ? (
            <p data-remove-explain>
              It loses access at once: it's taken off your account. Arugula keeps running on it, reachable only locally. To add it back, join it again with{" "}
              <code>arugulad join</code>, which makes a new key.
            </p>
          ) : (
            <p data-remove-explain>It loses access to your machines at once. To use it again, add it as a new device: it gets a new key.</p>
          )}
          {s.passkeyDevices.has(removing.device) ? <p data-remove-passkey-device>It's your passkey's device, so this signs it out on every browser the passkey is synced to.</p> : null}
          <div data-remove-which>
            <b>{shownName(removing.name)}</b> <DeviceLabel s={s} c={removing} />
          </div>
        </ConfirmRemove>
      ) : null}
      <div class="prompt-buttons">
        <button onClick={() => s.signOut(false)}>Sign out</button>
        <button onClick={close}>Done</button>
      </div>
    </Modal>
  );
}
