// Everything on control's page before the app (M17): signing in (GitHub or
// a passkey), and why you're here when a link brought you; this browser
// waiting for a device the account trusts to approve it, with its code; and
// the ways that can go wrong: approved without its code, its key lost, no
// longer trusted, or turned down.

import { useEffect, useState } from "preact/hooks";
import { CopyText } from "../../shared/ui/copy";
import { cameToRecover, DEVICE_HASH, deviceName, inApp, inviteInHash, joinInHash, passkeyRegister, passkeySignIn, previewInvite, signInNext, wholeJoinInHash, type ControlSession } from "../control";
import { fingerprint } from "../e2e/cert.ts";
import { addedAt, Center, QrCode, useControl } from "./common";
import { DeviceDetails } from "./devices";
import { RecoveryForm } from "./recovery";

/** The hosted control's terms (#172); a control you run yourself has its
 * own, or none. */
const HOSTED = "control.arugula.io";
const SITE = "https://arugula.io";

function HostedTerms() {
  if (location.hostname !== HOSTED) return null;
  return (
    <p class="control-legal dim" data-legal>
      Free during the beta, provided as is. By signing in you agree to the <a href={`${SITE}/terms`}>terms</a>; the{" "}
      <a href={`${SITE}/privacy`}>privacy notice</a> says what this service keeps.
    </p>
  );
}

/** Everything before the app: sign in, approval, no machines yet. */
export function ControlGate({ s }: { s: ControlSession }) {
  useControl(s);
  if (s.phase === "loading") return <Center>Connecting to control…</Center>;
  if (s.phase === "error")
    return (
      <Center>
        <h1>Something went wrong</h1>
        <p class="control-error">{s.error}</p>
        <button class="primary" onClick={() => location.reload()}>
          Try again
        </button>
      </Center>
    );
  if (s.phase === "signed-out") {
    const next = encodeURIComponent(signInNext());
    return (
      <Center>
        <h1>Arugula</h1>
        <WhyHere />
        <p>Your terminals, on every machine, from any device. End to end encrypted: this service introduces your devices to your machines and relays for them, but can't read what they say.</p>
        <div class="control-signins">
          {s.info.github ? (
            <a class="primary control-signin" href={`/auth/github?next=${next}`} data-signin="github">
              Sign in with GitHub
            </a>
          ) : null}
          {s.info.passkeys ? <PasskeyButtons /> : null}
        </div>
        {!s.info.github && !s.info.passkeys ? <p class="control-error">No sign-in is configured on this control.</p> : null}
        <HostedTerms />
      </Center>
    );
  }
  if (s.phase === "waiting") return <Waiting s={s} />;
  if (s.phase === "unvouched" && s.vouch) return <Unvouched s={s} />;
  if (s.phase === "lost-key")
    return (
      <Center>
        <h1>This browser lost its device key</h1>
        <p data-lost-key>
          It can't read back the key it was approved with (<b>{s.enrollment?.cert.name ?? "this browser"}</b>
          {s.enrollment ? `, added ${addedAt(s.enrollment.cert.created)}` : ""}), so it can't approve machines or reach them. Some versions of Safari lose keys this way.
        </p>
        {s.enrollment ? <DeviceDetails s={s} c={s.enrollment.cert} /> : null}
        <p>
          Enroll it again as a new device: another of your devices approves it, or a recovery code does. The old entry stays in <i>Devices and machines</i>.
        </p>
        <button class="primary" data-enroll-again onClick={() => void s.enrollAgain()}>
          Forget this browser and enroll again
        </button>
        <SignOuts s={s} />
      </Center>
    );
  // #327: the account doesn't trust this browser's key any more (removed,
  // or its approval went), so its approvals would be refused. Say so before
  // anyone clicks Approve.
  if (s.phase === "untrusted")
    return (
      <Center>
        <h1>This browser isn't trusted any more</h1>
        <p data-untrusted={s.untrustedWhy}>
          Your account {s.untrustedWhy === "removed" ? "removed this browser" : "no longer trusts this browser"} (<b>{s.enrollment?.cert.name ?? "this browser"}</b>
          {s.enrollment ? `, added ${addedAt(s.enrollment.cert.created)}` : ""}), so it can't approve machines or devices, or reach your machines.
        </p>
        {s.enrollment ? <DeviceDetails s={s} c={s.enrollment.cert} /> : null}
        <p>
          Forget it here and enroll it again as a new device. Another of your devices approves it, or a recovery code does: the next screen asks for one.
        </p>
        <button class="primary" data-enroll-again onClick={() => void s.enrollAgain(true)}>
          Forget this browser and enroll again
        </button>
        <SignOuts s={s} />
      </Center>
    );
  if (s.phase === "turned-down")
    return (
      <Center>
        <h1>Turned down</h1>
        <p data-turned-down>
          <b>{s.turnedDownBy || "Another device"}</b> turned this browser down. If that was a mistake, ask again and approve it there.
        </p>
        <button class="primary" data-try-again onClick={() => s.tryAgain()}>
          Try again
        </button>
        <RecoveryForm s={s} label="Use a recovery code" />
        <SignOuts s={s} />
      </Center>
    );
  return null;
}

/** Sign out, keeping this browser's key for next time, or forgetting it
 * too (#327): a key the account no longer trusts only gets in the way. */
function SignOuts({ s }: { s: ControlSession }) {
  return (
    <p class="control-signouts">
      <button class="control-linkish" data-sign-out onClick={() => void s.signOut(false)}>
        Sign out
      </button>
      {" · "}
      <button class="control-linkish" data-sign-out-forget onClick={() => void s.signOut(true)}>
        Sign out and forget this browser
      </button>
    </p>
  );
}

/** Here from forgetting a stale browser: the recovery form starts open. */
let recover: boolean | undefined;
const recovering = () => (recover ??= cameToRecover());

/** This browser waits for a device the account trusts to approve it.
 * #326: it says it's the browser (or the app's window) being approved, by
 * name, and that adding a machine is a separate approval that doesn't need
 * this one. Here from a machine's approval link (`#join=`), it leads with
 * that: the code goes to a device already in the account, which sees this
 * browser's request alongside and approves both at once.
 *
 * #761: it shows a code (a QR code, and as text) rather than its
 * fingerprint: the device that approves it scans or types it, and answers
 * its secret. A page from before codes still asks that device to compare
 * the fingerprint, so it's a click away. */
function Waiting({ s }: { s: ControlSession }) {
  const [hash, setHash] = useState(location.hash);
  useEffect(() => {
    const on = () => setHash(location.hash);
    addEventListener("hashchange", on);
    return () => removeEventListener("hashchange", on);
  }, []);
  const code = joinInHash(hash);
  // The link to hand on keeps the secret (#760); control is told the code.
  const whole = wholeJoinInHash(hash) ?? code;
  const mine = s.deviceCode;
  const name = deviceName();
  // "the Arugula app on jake-air", or "this browser (Chrome on Mac)".
  // #764: keys from a passkey are that passkey's device, on every browser.
  const what = s.fromPasskey ? "your passkey" : inApp() ? `the ${name}` : `this browser (${name})`;
  const fp = (
    <p class="fingerprint" data-fingerprint={s.keys.id}>
      {fingerprint(s.keys.id)}
    </p>
  );
  const older = (
    <details class="control-older" data-waiting-older>
      <summary>The other device asks you to compare a fingerprint?</summary>
      <p class="dim">Its page is from before codes: reload it to scan or type the code instead. Or check it shows this one:</p>
      {fp}
    </details>
  );
  if (code) {
    // #761: the link carries this browser's code too, so the device that
    // approves the machine approves this browser with it as well.
    const link = `${s.info.url}/#join=${whole}${mine ? `&device=${mine}` : ""}`;
    return (
      <Center>
        <h1>Approve the machine on a device you use</h1>
        <p data-waiting-join={code}>
          You're signed in as <b>{s.login}</b>, here to add a machine with code <b>{code}</b>. Only a device already in your account can approve it,
          and this browser isn't one yet. On a browser or phone you use with Arugula, {mine ? "scan this or open the link" : "open"}:
        </p>
        {mine ? <QrCode text={link} label="QR code for the machine's approval link" /> : null}
        <CopyText text={link} data-control-join-link />
        <p>Approve the code there, and pick where the machine goes: your account, or a team you own. That's the one approval the machine needs.</p>
        {mine ? (
          <>
            <p class="dim" data-waiting-also data-device-code={mine}>
              The link carries {what}'s code too, so that device lists it next to the machine. Approve it too only if you want this browser to reach
              your machines; the machine joins either way.
            </p>
            {older}
          </>
        ) : (
          <>
            <p class="dim" data-waiting-also>
              That device also lists {what} next to the machine, with this fingerprint. Approve it too only if you want this browser to reach your
              machines; the machine joins either way.
            </p>
            {fp}
          </>
        )}
        <p class="dim">Waiting…</p>
        <PasskeyInstead s={s} />
        <RecoveryForm s={s} open={recovering()} />
        <SignOuts s={s} />
      </Center>
    );
  }
  return (
    <Center>
      <h1>{inApp() ? "Approve this app as a device" : "Approve this browser"}</h1>
      {mine ? (
        <>
          <p data-waiting-browser={name} data-waiting-passkey={s.fromPasskey ? "" : undefined}>
            You're signed in as <b>{s.login}</b>. This approves {what} as one of your devices, so it can reach your machines.
            {s.fromPasskey ? " Approve it once, and it comes back on any browser the passkey is synced to." : ""} On a device you already use, scan this code with its camera, or open <CopyText inline text={s.info.url} data-control-url /> there and type the code when it asks.
          </p>
          <div class="control-device-code" data-device-code={mine}>
            <QrCode text={`${s.info.url}/#device=${mine}`} label="QR code for this browser's code" />
            <CopyText text={mine} data-device-code-text />
          </div>
          <p class="dim">Waiting…</p>
          {older}
        </>
      ) : (
        <>
          <p data-waiting-browser={name}>
            You're signed in as <b>{s.login}</b>. This approves {what} as one of your devices, so it can reach your machines. A device you already use
            approves it: open <CopyText inline text={s.info.url} data-control-url /> on that device; it asks there.
          </p>
          <p>It shows this fingerprint; check it matches:</p>
          {fp}
          <p class="dim">Waiting…</p>
        </>
      )}
      <p class="control-aside" data-waiting-machine>
        Here to add a machine to your account or a team? That's a separate approval, and it doesn't need this one. The machine shows a code (in
        Getting started, or where you ran <code>arugulad join</code>): approve that code on a device you already use.
      </p>
      <PasskeyInstead s={s} />
      <RecoveryForm s={s} open={recovering()} />
      <SignOuts s={s} />
    </Center>
  );
}

/** #764: a passkey whose PRF output gives the device another browser
 * enrolled with it: this browser becomes that device, with no approval. */
function PasskeyInstead({ s }: { s: ControlSession }) {
  const [err, setErr] = useState("");
  if (!s.info.passkeys || !s.passkeys || s.fromPasskey || inApp()) return null;
  return (
    <>
      <button class="control-secondary" data-use-passkey onClick={() => s.usePasskey().catch((e: Error) => setErr(e.name === "NotAllowedError" ? "Cancelled." : e.message))}>
        Use your passkey instead
      </button>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}

/** #761: approved, but not with the code this browser showed. A wrong
 * answer is refused (the code is used up); none at all came from a page
 * from before codes, and the person checks the account's fingerprint on
 * that device instead, the old way. */
function Unvouched({ s }: { s: ControlSession }) {
  const v = s.vouch!;
  const by = v.by ? <b>{v.by}</b> : "Another device";
  if (v.why === "wrong")
    return (
      <Center>
        <h1>The approval doesn't match this browser's code</h1>
        <p data-unvouched="wrong">
          {by} approved this browser, but its answer doesn't match the code this browser showed: the code was mistyped, or control changed the account
          it was approved into. This browser doesn't trust it, and that code is used up.
        </p>
        <p>Enroll it again to get a new code. If the old one is listed under <i>Devices and machines…</i> on your other device, remove it there.</p>
        <button class="primary" data-enroll-again onClick={() => void s.enrollAgain()}>
          Forget this browser and enroll again
        </button>
        <SignOuts s={s} />
      </Center>
    );
  return (
    <Center>
      <h1>Check your account</h1>
      <p data-unvouched="missing">
        {by} approved this browser from a page that didn't use its code, so this browser can't tell your account from one control made up. On{" "}
        {v.by ? "that device" : "a device you use"}, open <i>Devices and machines…</i> and check your account's fingerprint there is this one:
      </p>
      <p class="fingerprint" data-unvouched-account={v.root}>
        {fingerprint(v.root)}
      </p>
      <div class="prompt-buttons">
        <button data-enroll-again onClick={() => void s.enrollAgain()}>
          It isn't: forget this browser
        </button>
        <button class="primary" data-vouch onClick={() => s.vouchFor()}>
          It matches
        </button>
      </div>
      <SignOuts s={s} />
    </Center>
  );
}

/** Signed out, but following a link (#103): say what it was for. The hash
 * survives signing in, so it opens once you're in. */
function WhyHere() {
  const [hash, setHash] = useState(location.hash);
  const invite = inviteInHash(hash);
  const [team, setTeam] = useState<{ name: string; by: string } | null>(null);
  useEffect(() => {
    const on = () => setHash(location.hash);
    addEventListener("hashchange", on);
    on();
    return () => removeEventListener("hashchange", on);
  }, []);
  useEffect(() => {
    setTeam(null);
    if (invite) void previewInvite(invite).then(setTeam);
  }, [hash]);
  if (invite)
    return (
      <p class="control-why" data-why="invite">
        {team ? (
          <>
            {team.by ? `${team.by} invited you` : "You've been invited"} to <b data-why-team>{team.name}</b>.
          </>
        ) : (
          "You've been invited to a team."
        )}{" "}
        Sign in or make an account to accept.
      </p>
    );
  if (/^#join=/.test(hash))
    return (
      <p class="control-why" data-why="join">
        Sign in to approve this machine.
      </p>
    );
  if (DEVICE_HASH.test(hash))
    return (
      <p class="control-why" data-why="device">
        Sign in to approve the browser or phone that showed you this code.
      </p>
    );
  if (/^#app=/.test(hash))
    return (
      <p class="control-why" data-why="app">
        Sign in to let the Arugula app use your account.
      </p>
    );
  return null;
}

function PasskeyButtons() {
  const [err, setErr] = useState("");
  // #102: a new account says what teammates call it first.
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const go = (f: () => Promise<unknown>) =>
    f().then(
      () => location.reload(),
      (e: Error) => setErr(e.name === "NotAllowedError" ? "Cancelled." : e.message),
    );
  return (
    <>
      <button class="primary control-signin" data-signin="passkey" onClick={() => go(passkeySignIn)}>
        Sign in with a passkey
      </button>
      {naming ? (
        <form
          class="control-code control-name"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) void go(() => passkeyRegister(name));
          }}
        >
          <input
            data-signup-name
            placeholder="Your name"
            maxLength={64}
            value={name}
            onInput={(e) => setName((e.target as HTMLInputElement).value)}
            aria-label="Your name"
            autoFocus
          />
          <button type="submit" class="primary" data-signup-go disabled={!name.trim()}>
            Make the account
          </button>
          <p class="dim">Teammates see this name. You can change it later.</p>
        </form>
      ) : (
        <button class="control-signin control-secondary" data-signup="passkey" onClick={() => setNaming(true)}>
          New here? Make an account with a passkey
        </button>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
