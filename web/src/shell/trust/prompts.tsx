// The overlay's other prompts: the desktop app signing in (M48), adding a
// passkey, a session shared with this account, team invites (#762,
// presigned or Ask me first)
// and an owner admitting someone who used one, and what control kept to
// say (a team joined, #103; a team deleted, #206).

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import { signedAppIn, type AppLogin, type ControlSession, type ShareOffer, type Team } from "../control";
import type { TeamRole } from "../e2e/team.ts";
import { roleAs, roleLabel } from "../ui/roles";
import { addedAt, clearHash, dropHash } from "./common";

/** M48: the desktop app asks to sign in as this account. Allowing hands a
 * grant to the app on this computer (its loopback port): an app elsewhere
 * that sent this link gets nothing, so there's nothing to compare (#758).
 * From a device, it also signs the app's certificate, so the app is a
 * device at once; otherwise its window asks to be approved afterwards (the
 * "New device?" prompt). An app from before #758 still shows a code. */
export function AppLoginPrompt({ s, id }: { s: ControlSession; id: string }) {
  const [a, setA] = useState<AppLogin | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    s.showAppLogin(id).then(setA, (e: Error) => setErr(e.message));
  }, [id]);
  return (
    <Modal close={clearHash}>
      <h2>Sign in the app?</h2>
      {err ? <p class="control-error" data-app-login-error>{err}</p> : null}
      {a?.allowed ? (
        <p data-app-login-error>This sign-in was already allowed. Start it again in the app if it didn't finish.</p>
      ) : a ? (
        <>
          <p>
            <b data-app-login-name>{a.name}</b> asks to sign in as you.
            {a.code ? (
              <>
                {" "}
                Check the app shows <b data-app-login-code={a.code}>{a.code}</b>.
              </>
            ) : null}
            {s.signsApp(a) ? (
              <span data-app-login-device>
                {" "}
                Allowing also makes it one of your devices, as an app: its window reaches your machines and can approve machines, CLIs and other apps,
                but not browsers, phones or deploys.
              </span>
            ) : null}
          </p>
          {a.same_network ? null : (
            <p class="control-error" data-app-login-elsewhere>
              It asked from another network ({a.from}) than this browser's. If the app isn't on this computer, cancel.
            </p>
          )}
          <p class="dim">
            Only the app on this computer can finish it. If you didn't just press Sign in in the Arugula app, cancel: someone may have sent you
            this link.
          </p>
          <div class="prompt-buttons">
            <button onClick={clearHash}>Cancel</button>
            <button
              class="primary"
              data-app-login-allow
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  // To the app's loopback port; it sends this page back.
                  location.href = await s.allowAppLogin(id, a);
                } catch (e) {
                  setErr((e as Error).message);
                  setBusy(false);
                }
              }}
            >
              Allow
            </button>
          </div>
        </>
      ) : err ? null : (
        <p class="dim">Looking it up…</p>
      )}
    </Modal>
  );
}

/** A passkey made for an account that has some: one of them agrees before
 * control keeps it, so a session alone can't add one. Its own click, as a
 * browser asks for a passkey only on one. */
export function ConfirmPasskey({ s }: { s: ControlSession }) {
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Modal close={() => s.dropPasskey()}>
      <h2>Confirm the new passkey</h2>
      <p data-passkey-confirm>
        Your new passkey is made. Confirm it with a passkey you already have: nobody adds one to your account without one of yours. Every device
        of yours hears that it was added.
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button onClick={() => s.dropPasskey()}>Cancel</button>
        <button
          class="primary"
          data-passkey-confirm-go
          disabled={busy}
          onClick={() => {
            setBusy(true);
            s.confirmPasskey().catch((e: Error) => {
              setErr(e.name === "NotAllowedError" ? "Cancelled." : e.message);
              setBusy(false);
            });
          }}
        >
          Confirm with a passkey
        </button>
      </div>
    </Modal>
  );
}

/** Back from signing in with GitHub again to add a passkey (an account
 * with none needs a sign-in from the last few minutes): make it now. */
export function AddPasskeyAgain({ s }: { s: ControlSession }) {
  const [err, setErr] = useState("");
  return (
    <Modal close={clearHash}>
      <h2>Add a passkey</h2>
      <p data-add-passkey-again>You signed in with GitHub again. Add the passkey now: it signs you in without GitHub from then on.</p>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button onClick={clearHash}>Not now</button>
        <button
          class="primary"
          data-add-passkey
          onClick={() =>
            s.addPasskey().then(clearHash, (e: Error) => setErr(e.name === "NotAllowedError" ? "Cancelled." : e.message))
          }
        >
          Add a passkey
        </button>
      </div>
    </Modal>
  );
}

/** Back from handing the app its grant (and, from a device, its
 * certificate). */
export function AppLoginDone() {
  const [signed] = useState(signedAppIn);
  useEffect(() => {
    // Out of the way of the device prompt that follows.
    const t = setTimeout(clearHash, 6000);
    return () => clearTimeout(t);
  }, []);
  return (
    <Modal close={clearHash}>
      <h2>Sign in the app?</h2>
      {signed ? (
        <p data-app-login-done>
          Signed in, and the app is one of your devices now: its window reaches your machines. As an app, it approves machines and CLIs, not
          browsers, phones or deploys. Its own machine joins separately, with a code, and doesn't need this.
        </p>
      ) : (
        <p data-app-login-done>
          Signed in. Next the app's window asks to be approved as a device, so it reaches your machines: the prompt shows here in a moment. As an
          app, it approves machines and CLIs, not browsers, phones or deploys. Its own machine joins separately, with a code, and doesn't need this.
        </p>
      )}
      <div class="prompt-buttons">
        <button class="primary" onClick={clearHash}>
          Done
        </button>
      </div>
    </Modal>
  );
}

/** Someone shares a session on their machine with this account: it's
 * listed (and its notifications reach here) only once accepted. */
export function ShareOfferPrompt({ s, o }: { s: ControlSession; o: ShareOffer }) {
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const who = o.owner_name || o.owner_login || "Someone";
  const answer = (accept: boolean) => async () => {
    setBusy(true);
    try {
      await s.answerShare(o.daemon, accept);
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy(false);
  };
  return (
    <Modal>
      <h2>A shared session</h2>
      <p data-share-offer={o.daemon}>
        <b data-share-offer-owner>{who}</b>
        {o.owner_login && o.owner_login !== who ? ` (${o.owner_login})` : ""} wants to share a session on their machine{" "}
        <b data-share-offer-machine>{o.name}</b> with you.
      </p>
      <p class="dim">Accept only if you know them. Accepting lists the machine here and lets its notifications reach you.</p>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button data-share-decline disabled={busy} onClick={answer(false)}>
          Decline
        </button>
        <button class="primary" data-share-accept disabled={busy} onClick={answer(true)}>
          Accept
        </button>
      </div>
    </Modal>
  );
}

/** An owner said yes to this account's request (#103). */
export function Joined({ s }: { s: ControlSession }) {
  const j = s.joined!;
  const has = s.daemons.some((d) => d.team === j.team);
  return (
    <Modal close={() => s.sawJoined()}>
      <h2 data-joined={j.team}>You're in {j.name}</h2>
      <p>{has ? "Its machines are in your host list now." : "Its machines appear here when an owner adds one."}</p>
      <div class="prompt-buttons">
        <button class="primary" onClick={() => s.sawJoined()}>
          OK
        </button>
      </div>
    </Modal>
  );
}

/** Something control kept to tell this account (#206: a team it was in
 * was deleted while this page wasn't open, or was). */
export function Notice({ s, n }: { s: ControlSession; n: ControlSession["notices"][number] }) {
  return (
    <Modal close={() => void s.sawNotice(n.id)}>
      <h2 data-notice={n.id}>{n.title}</h2>
      <p>{n.body}</p>
      <div class="prompt-buttons">
        <button class="primary" onClick={() => void s.sawNotice(n.id)}>
          OK
        </button>
      </div>
    </Modal>
  );
}

/** An Ask me first link: `code` is its one-time key's seed (#762), or,
 * for a link from before (`old`), a code that no longer works. A link that
 * can't be used (old, expired, never was) says so plainly with Close: it
 * isn't the person's mistake. */
export function InvitePrompt({ s, team, code, old }: { s: ControlSession; team: string; code: string; old: boolean }) {
  const [info, setInfo] = useState<{ name: string; role: TeamRole } | null>(null);
  const [gone, setGone] = useState("");
  const [err, setErr] = useState("");
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (old) return setGone("This invite link is from an older version of Arugula. Ask for a new one.");
    s.showInvite(team, code).then(setInfo, (e: Error) => setGone(e.message));
  }, [team, code, old]);
  return (
    <Modal close={clearHash}>
      <h2>Join a team?</h2>
      {gone ? <p data-invite-gone>{gone}</p> : null}
      {err ? <p class="control-error">{err}</p> : null}
      {info && !done ? (
        <p>
          You're invited to <b data-invite-team={team}>{info.name}</b> {roleAs(info.role)}.
        </p>
      ) : null}
      {done ? <p data-invite-pending>Asked to join. An owner adds you when they're next here; their machines appear once they do.</p> : null}
      <div class="prompt-buttons">
        <button onClick={clearHash}>{gone ? "Close" : done ? "Done" : "Not now"}</button>
        {!done && !gone ? (
          <button
            class="primary"
            data-accept-invite
            disabled={!info}
            onClick={() =>
              s.acceptInvite(team, code).then(
                () => {
                  dropHash();
                  setDone(true);
                },
                (e: Error) => setErr(e.message),
              )
            }
          >
            Join
          </button>
        ) : null}
      </div>
    </Modal>
  );
}

/** A presigned invite: accepting it is the whole of joining. */
export function PresignedPrompt({ s, team, seed }: { s: ControlSession; team: string; seed: string }) {
  const [info, setInfo] = useState<{ name: string; role: TeamRole } | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  // In it, whether by this click or before (the page may redraw this
  // prompt as the team's machines arrive).
  const member = s.teams.find((t) => t.team === team && t.role);
  useEffect(() => {
    if (member) return dropHash();
    s.showPresigned(team, seed).then(
      (p) => setInfo({ name: p.name, role: p.invite.role }),
      (e: Error) => setErr(e.message),
    );
  }, [team, seed]);
  const join = () => {
    setBusy(true);
    s.redeem(team, seed)
      .then(dropHash)
      .catch((e: Error) => setErr(e.message))
      .finally(() => setBusy(false));
  };
  return (
    <Modal close={clearHash}>
      <h2>{member ? `You're in ${member.roster.name}` : `Join ${info?.name ?? "a team"}?`}</h2>
      {err ? <p class="control-error">{err}</p> : null}
      {member ? (
        <p data-invite-joined>Its machines appear in the host menu.</p>
      ) : info ? (
        <p>
          You're invited to <b data-invite-team={team}>{info.name}</b> {roleAs(info.role)}. Joining adds you right away; its owners are told.
        </p>
      ) : null}
      <div class="prompt-buttons">
        <button onClick={clearHash}>{member ? "Done" : "Not now"}</button>
        {!member ? (
          <button class="primary" data-accept-invite disabled={!info || busy} onClick={join}>
            {busy ? "Joining…" : `Join ${info?.name ?? ""}`.trim()}
          </button>
        ) : null}
      </div>
    </Modal>
  );
}

/** Someone used an Ask me first link (#762). The link's one-time key
 * vouched for their account (`askOk`), so there's nothing to compare:
 * the owner only decides. Not now turns the request down; the link still
 * works if they ask again. */
export function AdmitPrompt({ s, team, req }: { s: ControlSession; team: Team; req: Team["requests"][number] }) {
  const [err, setErr] = useState("");
  const whose = req.by === s.account ? "your" : `${team.names?.[req.by] ?? team.roster.members.find((m) => m.account === req.by)?.name ?? "an owner"}'s`;
  return (
    <Modal>
      <h2>Add to {team.roster.name}?</h2>
      <p data-admit={req.account}>
        <b>{req.name}</b> used {whose} invite link (made {addedAt(req.invite.made)}, for {roleFor(req.invite.role)}).
      </p>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button data-admit-no onClick={() => s.rejectRequest(team.team, req.account).catch((e: Error) => setErr(e.message))}>
          Not now
        </button>
        <button class="primary" data-admit-yes onClick={() => s.admit(team.team, req).catch((e: Error) => setErr(e.message))}>
          Add them
        </button>
      </div>
    </Modal>
  );
}

/** "an owner", "someone who drives". */
const roleFor = (role: TeamRole) => (role === "owner" ? "an owner" : `someone who ${roleLabel(role)}`);
