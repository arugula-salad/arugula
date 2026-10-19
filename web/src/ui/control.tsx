import { cloudPanes, useCloudPanes } from "../cloud";
// The control-mode screens (M17): signing in, waiting for this browser to
// be approved, approving other devices and daemons' join codes, the device
// list, and how to add a machine.

import { useEffect, useState } from "preact/hooks";
import { CloudPanel } from "./cloud";
import { enableControlPush } from "../push";
import { AlongsideError, api, cameToRecover, DEVICE_HASH, deviceName, inApp, inviteInHash, JOIN_HASH, joinInHash, passkeyRegister, passkeySignIn, previewInvite, RefusedError, shownName, signedAppIn, signInNext, splitDeviceCode, takesSecret, wholeJoinInHash, type AppLogin, type ControlSession, type DeviceCode, type JoinRequest } from "../control";
import { fingerprint, normalizeCode, normalizeSecret, splitJoinCode, type Cert } from "../e2e/cert.ts";
import { useSubscribe } from "./hooks";
import { chat } from "../chat-store";
import { directory } from "../hosts";
import { CopyButton, CopyText, download } from "./copy";
import { ROLE_HELP, roleAs, roleLabel } from "./roles";
import { MenuLayer, type MenuItem } from "./menu";
import { AccountButton } from "./app-bar";
import type { LabsFlags, PresignedInvite, ShareOffer, Team } from "../control";
import { FLAGS } from "../proto.gen";
import type { TeamRole } from "../e2e/team.ts";
import { qr, qrPath } from "./qr";
import { AccountPanel, type SessionRow } from "./account";
import { machinePlace, machineState, placeMeans } from "./hosts";
import { ConfirmRemove } from "./confirm";
import { canScan, Scanner } from "./scan";
import { verifyCode, verifyWords } from "../e2e/words.ts";
import { TelemetryCard } from "./telemetry-card";
import { SharedDocs } from "./shared-docs";

export function useControl(s: ControlSession) {
  useSubscribe((fn) => s.subscribe(fn));
}

function Center({ children }: { children: preact.ComponentChildren }) {
  return <div class="control-center">{children}</div>;
}

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

/** #763: for someone sharing with you who wants to verify it's you: a QR
 * code for their camera, and the same as words for over the phone. From
 * the root this device pinned, not control's word for it. */
function VerifyMe({ account, root }: { account: string; root: string }) {
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

/** A link as a QR code, for a phone's camera. */
function QrCode({ text, label }: { text: string; label: string }) {
  const code = qrPath(qr(text));
  return (
    <svg class="control-qr" viewBox={`0 0 ${code.n} ${code.n}`} role="img" aria-label={label} data-qr={text}>
      <rect width={code.n} height={code.n} fill="#fff" />
      <path d={code.d} fill="#000" />
    </svg>
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

/** What other people see (#102), changed in place. */
function NameLine({ s }: { s: ControlSession }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(s.name);
  const [err, setErr] = useState("");
  if (!editing)
    return (
      <p class="dim">
        Name <b data-account-name>{s.name || "none yet"}</b>{" "}
        <button
          class="control-linkish"
          data-edit-name
          onClick={() => {
            setName(s.name);
            setEditing(true);
          }}
        >
          Change
        </button>
      </p>
    );
  return (
    <form
      class="control-code control-name"
      onSubmit={(e) => {
        e.preventDefault();
        s.setName(name).then(
          () => setEditing(false),
          (x: Error) => setErr(x.message),
        );
      }}
    >
      <input data-name-input maxLength={64} value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} aria-label="Name" autoFocus />
      <button type="submit" data-save-name disabled={!name.trim()}>
        Save
      </button>
      {err ? <p class="control-error">{err}</p> : null}
    </form>
  );
}

function RecoveryForm({ s, label = "Lost your other devices? Use a recovery code", open: startOpen = false }: { s: ControlSession; label?: string; open?: boolean }) {
  const [open, setOpen] = useState(startOpen);
  const [code, setCode] = useState("");
  const [err, setErr] = useState("");
  // A button that looks like one (#327): as a link it read as a sentence.
  if (!open)
    return (
      <button class="control-secondary" data-use-recovery onClick={() => setOpen(true)}>
        {label}
      </button>
    );
  return (
    <form
      class="control-code"
      onSubmit={(e) => {
        e.preventDefault();
        s.useRecoveryCode(code).catch((x: Error) => setErr(x.message));
      }}
    >
      <input placeholder="Recovery code" value={code} onInput={(e) => setCode((e.target as HTMLInputElement).value)} aria-label="Recovery code" data-recovery-input autoFocus={startOpen} />
      <button type="submit">Use it</button>
      {err ? <p class="control-error">{err}</p> : null}
    </form>
  );
}

/** Once, after the account's first device (or new codes): the codes to keep. */
function RecoveryCodes({ s }: { s: ControlSession }) {
  const all = s.recoveryCodes!.join("\n") + "\n";
  const [stored, setStored] = useState(false);
  return (
    <Modal>
      <h2>Your recovery codes</h2>
      <p>If you lose every device that can approve new ones, one of these lets a new browser in. Each works once. Keep them somewhere safe and offline: they're shown only now, and this service never had them.</p>
      {s.recoveryCodes!.map((c) => (
        <p key={c}>
          <CopyText text={c} data-recovery-code />
        </p>
      ))}
      <div class="prompt-buttons">
        <CopyButton text={all} label="Copy both" />
        <button data-download-codes onClick={() => download("arugula-recovery-codes.txt", all)}>
          Download .txt
        </button>
      </div>
      <label class="control-check">
        <input type="checkbox" data-stored-codes checked={stored} onChange={(e) => setStored((e.target as HTMLInputElement).checked)} />
        I've stored these somewhere safe
      </label>
      <PasskeyNudge s={s} />
      <div class="prompt-buttons">
        <button class="primary" data-saved-codes disabled={!stored} onClick={() => s.savedRecoveryCodes()}>
          Continue
        </button>
      </div>
    </Modal>
  );
}

/** Ready, but no machine to show yet: the account's menu (#97), any team
 * this account is waiting on (#103), and how to add a machine (#98). */
export function NoMachines({ s, newAgent }: { s: ControlSession; newAgent?: () => void }) {
  useCloudPanes();
  useSubscribe((fn) => chat.subscribe(fn));
  return (
    <div class="control-page">
      <header class="control-account" data-account-bar>
        <span class="control-who">Arugula</span>
        <AccountButton client={null} />
      </header>
      <MenuLayer />
      <main>
        {s.asked.map((a) => (
          <p key={a.team} class="control-asked" data-asked={a.team}>
            Waiting for {a.owners.length ? a.owners.join(" or ") : "an owner"} to add you to <b>{a.name}</b>. Their machines appear here when they do.
          </p>
        ))}
        {/* M123: chat is control's, and needs no machine. */}
        {chat.on && (
          <button data-open-chat onClick={() => (location.hash = "chat")}>
            Chat
          </button>
        )}
        {cloudPanes.enabled && newAgent && <button class="primary" onClick={newAgent}>New agent</button>}
        <button onClick={openPanel("cloud")}>Cloud agents</button>
        {cloudPanes.enabled && <button data-open-cloud-workspaces onClick={() => (location.hash = "cloud-ws")}>Cloud workspaces</button>}
        <h1>{s.asked.length ? "Add your own machine" : "Add a machine"}</h1>
        <AddMachine s={s} />
      </main>
    </div>
  );
}

const INSTALL = "curl -fsSL https://arugula.io/install.sh | sh";
const INSTALL_WINDOWS = "irm https://arugula.io/install.ps1 | iex";

function AddMachine({ s }: { s: ControlSession }) {
  return (
    <div class="control-add">
      <ol class="control-steps">
        <li>
          <b>Install Arugula on the machine.</b> On macOS or Linux:
          <CopyText text={INSTALL} data-install />
          On Windows, in PowerShell:
          <CopyText text={INSTALL_WINDOWS} data-install-windows />
          <span class="dim">
            Or with <a href="https://arugula.io/#install" target="_blank" rel="noopener">Homebrew, or from source</a>.
          </span>
        </li>
        <li>
          <b>Join it to this account:</b>
          <CopyText text={`~/.local/bin/arugulad join ${s.info.url}`} data-join-cmd />
          On Windows, in a new PowerShell window:
          <CopyText text={`arugulad join ${s.info.url}`} data-join-cmd-windows />
          {s.teams
            .filter((t) => !t.locked || t.role === "owner")
            .map((t) => (
              <TeamJoin key={t.team} s={s} t={t} />
            ))}
        </li>
        <li>
          <b>Approve it here.</b> It prints a link and a code: open the link here, or type the code below. Codes last 15 minutes.
          <JoinCodeForm s={s} />
        </li>
      </ol>
      <p class="dim">
        A <i>machine</i> runs your terminals; a <i>device</i> (this browser, your phone) reaches them.
      </p>
      {s.sandboxesOpen ? <HostedVm s={s} /> : null}
    </div>
  );
}

/** How a machine joins a team (#332: any member adds their own), in the
 * same words on Add a machine and in Teams. */
function TeamJoin({ s, t, here }: { s: ControlSession; t: Team; here?: boolean }) {
  return (
    <>
      <span class="dim">
        To add a machine to <b>{t.roster.name}</b>, run this on it, or move one you have with <i>Team …</i> in <i>Devices and machines…</i>:
      </span>
      <CopyText text={`arugulad join ${s.info.url} --team ${t.team}`} {...(here ? { "data-team-join": true } : { "data-team-join-step": t.team })} />
    </>
  );
}

/** A hosted VM (M20): no machine of your own needed. */
function HostedVm({ s }: { s: ControlSession }) {
  useControl(s);
  const [err, setErr] = useState("");
  const starting = s.starting ? s.sandboxes.find((x) => x.id === s.starting) : undefined;
  return (
    <div class="hosted-vm">
      <p>No machine to hand? Use a hosted VM: a fresh Linux machine, deleted when you close its last tab.</p>
      <button class="primary" data-start-vm disabled={!!starting && !starting.state.startsWith("failed")} onClick={() => s.startSandbox().catch((e: Error) => setErr(e.message))}>
        {starting && !starting.state.startsWith("failed") ? `Starting (${starting.state})…` : "Start a hosted VM"}
      </button>
      {starting?.state.startsWith("failed") ? <p class="control-error">{starting.state}</p> : null}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

function JoinCodeForm({ s }: { s: ControlSession }) {
  const [code, setCode] = useState("");
  return (
    <form
      class="control-code"
      onSubmit={(e) => {
        e.preventDefault();
        // The whole code, secret too (#760): it stays in this page.
        const j = splitJoinCode(code.trim());
        location.hash = `join=${j ? (j.secret ? `${j.code}.${j.secret}` : j.code) : code.trim()}`;
      }}
    >
      <input class="control-join-code" placeholder="XXXXX-XXXXX.XXXX-XXXX-XXXX-XXXX" value={code} onInput={(e) => setCode((e.target as HTMLInputElement).value)} aria-label="Join code" />
      <button type="submit" disabled={!code.trim() || s.phase !== "ready"}>
        Look up
      </button>
    </form>
  );
}

type Panel = "cloud" | "devices" | "add" | "add-device" | "teams" | "plan" | "account" | "shared-docs";

const openPanel = (p: Panel) => () => dispatchEvent(new CustomEvent("arugula:control-panel", { detail: p }));

/** Over the app: approval prompts, a daemon's join, the device list. */
export function ControlOverlay({ s }: { s: ControlSession }) {
  useControl(s);
  const [hash, setHash] = useState(location.hash);
  const [panel, setPanel] = useState<null | Panel>(null);
  useEffect(() => {
    const on = () => setHash(location.hash);
    const open = (e: Event) => setPanel((e as CustomEvent<Panel>).detail);
    const cloudMessage = (event: MessageEvent) => {
      if (event.data?.type === "open-cloud-run" && /^run_[0-9a-f]+$/.test(event.data.run)) location.hash = `cloud=${event.data.run}`;
      if (event.data?.type === "open-cloud-tab" && /^cw_[0-9a-f]+$/.test(event.data.cloud_workspace) && /^cwc_[0-9a-f]+$/.test(event.data.cloud_tab)) {
        location.hash = `cloud-ws=${event.data.cloud_workspace}/${event.data.cloud_tab}`;
      }
    };
    navigator.serviceWorker?.addEventListener("message", cloudMessage);
    addEventListener("hashchange", on);
    addEventListener("arugula:control-panel", open);
    // Says the panel event has a listener, so a test can wait for it
    // rather than send one into nothing.
    document.documentElement.dataset.controlPanels = "";
    // Mounted afresh (the page switches machine when one is approved, and
    // that happens before the prompt clears the hash): catch up with a
    // hashchange that fired before this listener was there.
    on();
    return () => {
      navigator.serviceWorker?.removeEventListener("message", cloudMessage);
      removeEventListener("hashchange", on);
      removeEventListener("arugula:control-panel", open);
      delete document.documentElement.dataset.controlPanels;
    };
  }, []);
  if (s.phase !== "ready") return null;
  if (s.recoveryCodes) return <RecoveryCodes s={s} />;
  const join = JOIN_HASH.exec(hash);
  if (join) return <JoinPrompt s={s} code={join[1]} device={join[2]} />;
  // #761: a waiting device's code, scanned with the camera or opened.
  const device = DEVICE_HASH.exec(hash)?.[1];
  if (device) return <DeviceLinkPrompt key={device} s={s} whole={device} />;
  // The app's device asking to be approved comes first: it's the next step
  // after allowing its sign-in.
  const appLogin = /^#app=([0-9a-f]{16,128})$/.exec(hash)?.[1];
  if (appLogin && !s.pending[0]) return <AppLoginPrompt s={s} id={appLogin} />;
  if (hash === "#app-done" && !s.pending[0]) return <AppLoginDone />;
  const invite = inviteInHash(hash) ?? inviteInHash(usedLink);
  if (invite)
    return invite.presigned ? (
      <PresignedPrompt s={s} team={invite.team} seed={invite.code} />
    ) : (
      <InvitePrompt s={s} team={invite.team} code={invite.code} old={invite.old} />
    );
  // Someone used an invite to a team I own: add them (sign the roster)?
  const req = s.teams.flatMap((t) => (t.role === "owner" ? t.requests.map((r) => ({ t, r })) : []))[0];
  if (req) return <AdmitPrompt s={s} team={req.t} req={req.r} />;
  const asking = s.pending[0];
  // #326: a device that came to approve a machine's join: both together.
  const asked = asking && s.pendingJoins.get(asking.device);
  if (asking && asked) return <JoinPrompt key={asked} s={s} code={asked} from={asking} />;
  if (asking) return <DevicePrompt key={asking.device} s={s} c={asking} />;
  const offer = s.offers[0];
  if (offer) return <ShareOfferPrompt s={s} o={offer} />;
  if (s.joined) return <Joined s={s} />;
  const notice = s.notices[0];
  if (notice) return <Notice s={s} n={notice} />;
  const cloudRun = /^#cloud=(run_[0-9a-f]+)$/.exec(hash)?.[1];
  if (panel === "cloud") {
    const close = () => setPanel(null);
    return <Modal close={close}><CloudPanel key={cloudRun ?? "catalog"} close={close}  notify={() => enableControlPush(s.info.vapid, (sub) => s.subscribePush(sub))} /></Modal>;
  }
  if (panel === "devices") return <Devices s={s} close={() => setPanel(null)} />;
  if (panel === "teams") return <Teams s={s} close={() => setPanel(null)} />;
  if (panel === "plan") return <Plan s={s} close={() => setPanel(null)} />;
  if (panel === "shared-docs")
    return (
      <Modal close={() => setPanel(null)}>
        <SharedDocs s={s} close={() => setPanel(null)} />
      </Modal>
    );
  if (panel === "account")
    return (
      <Modal close={() => setPanel(null)}>
        <AccountPanel s={s} close={() => setPanel(null)} />
      </Modal>
    );
  if (panel === "add-device") return <AddDevice s={s} close={() => setPanel(null)} />;
  if (panel === "add")
    return (
      <Modal close={() => setPanel(null)}>
        <h2>Add a machine</h2>
        <AddMachine s={s} />
      </Modal>
    );
  // M104: asked once, when nothing else is.
  if (s.askTelemetry())
    return (
      <TelemetryCard
        about="Send usage counts from your account on this page: which views you open and how often, and errors our code hits. Never what your terminals show, names, file names, commands or prompts."
        answer={(on) => void s.setTelemetry(on).catch(() => {})}
        later={() => s.telemetryNotNow()}
      />
    );
  return null;
}

/** A control dialog: as tall as the window allows, scrolling inside, with
 * its close button pinned where it can always be seen. */
function Modal({ children, close }: { children: preact.ComponentChildren; close?: () => void }) {
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && close?.()}>
      <div class="prompt control-prompt">
        {close ? (
          <button class="modal-close" aria-label="Close dialog" title="Close" onClick={close}>
            ×
          </button>
        ) : null}
        {children}
      </div>
    </div>
  );
}

function clearHash() {
  usedLink = "";
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
}

/** An invite link that was used: out of the address bar, so a reload
 * doesn't offer it again (#208), but its prompt stays up until closed,
 * through the overlay mounting afresh. */
let usedLink = "";

function dropHash() {
  if (location.hash) usedLink = location.hash;
  history.replaceState(null, "", location.pathname + location.search);
}

/** A machine's join, by its code: from its approval link, typed in, or
 * (#326) brought by a waiting device that came to approve it (`from`).
 * Devices waiting with this code are listed alongside, and one Approve
 * covers the machine and them.
 *
 * #760: a machine whose link carries a secret (`CODE.SECRET`) is approved
 * with a MAC under it, and there's nothing to compare: the code was
 * recomputed from its key, and the machine checks the account itself. One
 * that made a secret but reached this page without it (the code alone, or
 * brought by a waiting device) asks for the whole code. An older machine
 * makes none, and its code and the account's fingerprint are compared, as
 * before.
 *
 * #761: a device alongside that shows a code is approved with it, not by
 * its fingerprint: from the link (`device`, which a waiting browser adds
 * to the machine's link it hands on), or scanned or typed on its card. */
function JoinPrompt({ s, code: whole, from, device }: { s: ControlSession; code: string; from?: Cert; device?: string }) {
  const parts = splitJoinCode(whole);
  const code = parts?.code ?? whole;
  // #760: the rest of the code, typed when the page has only the first half.
  const [typed, setTyped] = useState<Rest>(NO_REST);
  const [j, setJ] = useState<JoinRequest | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  // Control refused an approval from here (#327): closing then isn't
  // turning the machine down. It keeps waiting for one that works.
  const [refused, setRefused] = useState(false);
  // "" is just me; else a team's id.
  const [to, setTo] = useState("");
  // Devices alongside that the person unticked.
  const [skip, setSkip] = useState<Set<string>>(new Set());
  // #761: codes typed or scanned for devices alongside, by id.
  const [typedCodes, setTypedCodes] = useState<Map<string, Rest>>(new Map());
  useEffect(() => {
    // Another code: nothing typed for the last one carries over, nor for
    // the devices that came with it (#761).
    setTyped({ rest: "", bad: null });
    setTypedCodes(new Map());
    // This browser's own trust, fresh, before it offers Approve (#327).
    void s.refresh();
    s.showJoin(code).then(
      (j) => {
        setJ(j);
        setTo(j.team?.team ?? "");
      },
      (e: Error) => setErr(e.message),
    );
  }, [code]);
  // The machine's code is gone (approved elsewhere, expired): the device
  // that brought it asks on its own.
  if (from && err && !j) return <DevicePrompt s={s} c={from} />;
  // This browser waited with this code (#326), and it's gone: most likely
  // approved with this browser, on the device that approved both.
  if (!from && err && !j && s.brought === code)
    return (
      <Modal close={clearHash}>
        <h2>Add a machine?</h2>
        <p data-join-done={code}>
          Code <b>{s.brought}</b> isn't waiting any more: the device that approved this browser approved the machine with it, or the code expired. The
          machine says which.
        </p>
        <div class="prompt-buttons">
          <button class="primary" onClick={clearHash}>
            Done
          </button>
        </div>
      </Modal>
    );
  // Teams I'm in (#332), and the one it asked for even if I can't add to it.
  const teams = s.addableTeams();
  const asked = j?.team && !teams.some((t) => t.team === j.team!.team) ? j.team : null;
  const team = teams.find((t) => t.team === to);
  const cant = !!to && !team;
  const locked = !!asked && !!s.teams.find((t) => t.team === asked.team)?.locked;
  // Devices waiting with this code (#326), approved with the machine
  // unless unticked.
  const alongside = j ? s.pending.filter((c) => s.pendingJoins.get(c.device) === j.code) : [];
  // #761: each one's code, from the link or its card, when it's its own.
  const linked = device ? splitDeviceCode(device) : null;
  const codes = new Map<string, DeviceCode>();
  for (const c of alongside) {
    const want = s.pendingCodes.get(c.device);
    const given = (want && linked?.code === want ? linked : null) ?? typedCode(want ?? "", typedCodes.get(c.device) ?? NO_REST);
    if (given) codes.set(c.device, given);
  }
  const ready = (c: Cert) => !s.sealed(c) || codes.has(c.device);
  const fromLink = (c: Cert) => !!linked && linked.code === s.pendingCodes.get(c.device);
  const also = alongside.filter((c) => !skip.has(c.device) && ready(c));
  const cancel = () => {
    if (j && !refused) void s.rejectJoin(j.code).then(() => s.refresh(), () => {});
    clearHash();
  };
  const failed = (e: unknown) => {
    setErr((e as Error).message);
    if (e instanceof RefusedError) setRefused(true);
    setBusy(false);
  };
  // #760: the secret from the link (or the whole code typed in Look up),
  // else the rest of the code typed here.
  const secret = parts?.secret ?? (typed.bad ? null : normalizeSecret(typed.rest));
  const sealed = !!j && (!!parts?.secret || takesSecret(j));
  // Named, so it reads apart from a device's field below it (#761).
  const askWhole =
    j && sealed && !parts?.secret ? (
      <RestOfCode
        code={j.code}
        rest={typed.rest}
        bad={typed.bad}
        set={setTyped}
        cli={j.cert.kind === "cli"}
        prompt={
          <>
            Enter the rest of <b>{j.cert.name}</b>'s code to approve.
          </>
        }
      />
    ) : null;
  // M49: the Arugula CLI on a machine, asking to be one of your devices.
  if (j?.cert.kind === "cli")
    return <CliJoin s={s} j={j} cancel={cancel} refused={refused} failed={failed} sealed={sealed} secret={secret} whole={askWhole} />;
  const machine = j ? (
    sealed ? (
      <p data-join-sealed>
        <b>{j.cert.name}</b> asks to join {j.team ? <>the team <b data-join-team={j.team.team}>{j.team.name}</b></> : "your account"}.
      </p>
    ) : (
      <>
        <p>
          <b>{j.cert.name}</b> asks to join {j.team ? <>the team <b data-join-team={j.team.team}>{j.team.name}</b></> : "your account"} with code{" "}
          <b data-join-code={j.code}>{j.code}</b>. Check it's the code the machine shows (in Getting started, or where you ran <code>arugulad join</code>).
        </p>
        <p class="dim">Its key: {fingerprint(j.cert.device)}</p>
      </>
    )
  ) : null;
  return (
    <Modal close={from ? undefined : clearHash}>
      <h2>{alongside.length ? "Add a machine and a device?" : "Add a machine?"}</h2>
      {err ? (
        <p class="control-error" data-join-error>
          {err}
        </p>
      ) : null}
      {j ? (
        <>
          {alongside.length ? (
            <div class="control-both" data-join-both>
              <div class="control-both-card" data-join-machine>
                <div class="control-both-kind">{j.cert.name} (machine)</div>
                {machine}
              </div>
              {alongside.map((c) => (
                <div key={c.device} class="control-both-card" data-join-alongside={c.device}>
                  <div class="control-both-kind">{c.name} (browser)</div>
                  {s.sealed(c) ? (
                    <>
                      <p data-pending={c.device} data-device-sealed>
                        Signed in to come here and approve this machine. Approved too, it reaches your machines.
                        {fromLink(c) ? " Its code came with the link." : " Its code goes below."}
                      </p>
                    </>
                  ) : (
                    <>
                      <p>Signed in to come here and approve this machine. Approved too, it reaches your machines. It shows this fingerprint:</p>
                      <p class="fingerprint" data-pending={c.device}>
                        {fingerprint(c.device)}
                      </p>
                    </>
                  )}
                  <label class="control-check">
                    <input
                      type="checkbox"
                      data-join-also={c.device}
                      disabled={!ready(c)}
                      checked={!skip.has(c.device) && ready(c)}
                      onChange={(e) => {
                        const next = new Set(skip);
                        if ((e.target as HTMLInputElement).checked) next.delete(c.device);
                        else next.add(c.device);
                        setSkip(next);
                      }}
                    />{" "}
                    {/* #761: its code, entered below, ticks it. */}
                    {ready(c) ? "Approve it too" : "Approve it too, once its code is in below"}
                  </label>
                </div>
              ))}
            </div>
          ) : (
            machine
          )}
          {askWhole}
          {/* #761: a device's code, at full width under the cards. */}
          {alongside
            .filter((c) => s.sealed(c) && !fromLink(c))
            .map((c) => (
              <DeviceRest
                key={c.device}
                code={s.pendingCodes.get(c.device) ?? ""}
                typed={typedCodes.get(c.device) ?? NO_REST}
                set={(t) => setTypedCodes(new Map(typedCodes).set(c.device, t))}
                prompt={
                  <>
                    Enter the rest of <b>{c.name}</b>'s code to approve it too.
                  </>
                }
              />
            ))}
          {s.enrollment && !sealed ? (
            <p>
              Your account:{" "}
              <b class="fingerprint" data-join-account={s.enrollment.root}>
                {fingerprint(s.enrollment.root)}
              </b>
              . Once you approve, the machine shows its account's fingerprint: check it's this one there.
            </p>
          ) : null}
          {teams.length || asked ? (
            <>
              {/* #778: someone already in a team, before they pick: what each
                  place means, and the way to work with several teams. */}
              <p class="dim" data-join-place>
                A machine is in one place: <b>Just me</b> (only your devices see its panes, and the sessions you share) or one team (everyone in it sees
                them, and the team's owners count as its owners too). To work with more than one team, pick Just me, then share a session with each team
                (<i>Share</i>, beside the session's name).
              </p>
              <p>
                <label>
                  Join to{" "}
                  <select class="control-select" data-join-to value={to} onChange={(e) => setTo((e.target as HTMLSelectElement).value)}>
                    <option value="">Just me</option>
                    {teams.map((t) => (
                      <option key={t.team} value={t.team}>
                        {t.roster.name}
                      </option>
                    ))}
                    {asked ? (
                      <option value={asked.team}>
                        {asked.name} ({locked ? "locked" : "you're not in it"})
                      </option>
                    ) : null}
                  </select>
                </label>
              </p>
            </>
          ) : null}
          {cant ? (
            <p class="control-error" data-join-not-member>
              {locked
                ? `${asked!.name} is locked: only its owners add machines to it. Ask one of them to approve it, or pick Just me.`
                : `Only ${asked!.name}'s members add machines to it. Ask one of them to approve it, or pick Just me.`}
            </p>
          ) : (
            <p class="dim" data-join-grants>
              {team
                ? `Everyone in ${team.roster.name} sees it and reaches it by their role: owners and editors drive its terminals, viewers watch. Its owners also see private panes, and can take it out of the team. It stays yours.`
                : "Only your devices reach it, and they can drive its terminals. You're its only owner."}{" "}
              Control relays the connection but can't read it.
            </p>
          )}
        </>
      ) : err ? null : (
        <p class="dim">Looking up {code}…</p>
      )}
      <div class="prompt-buttons">
        <button data-cancel-join onClick={cancel}>
          {refused ? "Close" : "Cancel"}
        </button>
        <button
          class="primary"
          data-approve-join
          disabled={!j || busy || cant || (sealed && !secret)}
          onClick={async () => {
            if (!j) return;
            setBusy(true);
            try {
              // The machine, then the devices that came with it.
              await s.approveJoin(j.code, j.cert, team?.team ?? null, also, secret, codes);
              clearHash();
            } catch (e) {
              // The machine is in: only the device is left to ask again.
              if (e instanceof AlongsideError) setJ(null);
              failed(e);
            }
          }}
        >
          {also.length > 1 ? "Approve them all" : also.length ? "Approve both" : "Approve"}
        </button>
      </div>
    </Modal>
  );
}

/** Letters people read for digits in a join code: its alphabet has no
 * I, L or O, so they can only be 1, 1 and 0 (as Crockford's base32). */
const LOOKALIKE: Record<string, string> = { I: "1", L: "1", O: "0" };

/** What's typed in the rest-of-the-code field, as it's shown: the 16
 * characters after the dot, uppercase, dashed every four. A whole code,
 * pasted or typed, is fine when it starts with this `code`, and that half
 * is stripped. Once there's a dot (or more than 16 characters) what comes
 * first is a claimed code: one that isn't this one stays in the field, as
 * typed, with the error, until it's fixed or cleared. */
function restOf(raw: string, code: string): { rest: string; bad: string | null } {
  let r = raw;
  const dot = r.indexOf(".");
  const squash = (t: string) => t.toUpperCase().replace(/[^A-Z0-9]/g, "");
  const claimed = dot >= 0 ? squash(r.slice(0, dot)) : squash(r).length > 16 ? squash(r).slice(0, -16) : null;
  if (dot >= 0) r = r.slice(dot + 1);
  else if (claimed) r = squash(r).slice(-16);
  if (claimed && normalizeCode(claimed) !== code) {
    // Kept as typed, but tidied: the claimed code in its own shape, one
    // dot, and what follows dashed as the rest is.
    const after = squash(r).match(/.{1,4}/g)?.join("-") ?? "";
    const head = normalizeCode(claimed) ?? claimed;
    return { rest: dot >= 0 ? `${head}.${after}` : `${head}${after}`, bad: `That code starts ${head}, not ${code}.` };
  }
  const chars = r
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, "")
    .replace(/[ILO]/g, (c) => LOOKALIKE[c]);
  const odd = chars.match(/[^ABCDEFGHJKMNPQRSTVWXYZ0-9]/)?.[0];
  const kept = chars.replace(/[^ABCDEFGHJKMNPQRSTVWXYZ0-9]/g, "").slice(0, 16);
  const rest = kept.match(/.{1,4}/g)?.join("-") ?? "";
  if (odd) return { rest, bad: `Codes have no ${odd}.` };
  return { rest, bad: null };
}

/** What's typed in a `RestOfCode` field. */
type Rest = { rest: string; bad: string | null };
const NO_REST: Rest = { rest: "", bad: null };

/** #760: a machine (or the CLI) made a join secret, but this page has only
 * its code (typed alone, or brought by a waiting browser, #326). The code
 * is shown as a fixed prefix, and only the rest is typed. #761: the same
 * for a waiting device's code (`what="device"`), whose first half is its
 * request's keys' code; there the rest can be scanned too. Holding
 * another code (`restOf` keeps it, its first half and all), the field
 * drops the fixed half and widens, so it shows that code once and whole. */
function RestOfCode({
  code,
  rest,
  bad,
  set,
  cli = false,
  what = "join",
  hint,
  prompt = "Enter the rest of its code to approve.",
}: {
  code: string;
  rest: string;
  bad: string | null;
  set: (r: Rest) => void;
  cli?: boolean;
  what?: "join" | "device";
  hint?: preact.ComponentChildren;
  prompt?: preact.ComponentChildren;
}) {
  const [scanning, setScanning] = useState(false);
  const tag = (t: string) => ({ [`data-${what}-${t}`]: "" });
  // More than the 16 characters of a rest: a code of its own, typed whole.
  const claims = rest.replace(/[^A-Za-z0-9]/g, "").length > 16;
  return (
    <div class="control-rest" {...tag("rest")}>
      <p>
        {prompt}
        {what === "device" && canScan() && !scanning ? (
          <>
            {" "}
            <button type="button" class="control-linkish" data-scan onClick={() => setScanning(true)}>
              Scan it
            </button>
          </>
        ) : null}
      </p>
      {scanning ? (
        <Scanner
          accept={(t) => {
            const d = splitDeviceCode(t);
            if (d) set(restOf(`${d.code}.${d.secret}`, code));
            return !!d;
          }}
          close={() => setScanning(false)}
        />
      ) : null}
      <div class={claims ? "control-rest-field claims" : "control-rest-field"}>
        {/* Hidden, not removed, so the input keeps its place and focus. */}
        <span class="control-rest-known" aria-hidden="true" hidden={claims}>
          {code}.
        </span>
        <input
          {...tag("whole")}
          aria-label={`The rest of the code, after ${code}.`}
          placeholder="XXXX-XXXX-XXXX-XXXX"
          value={rest}
          autoComplete="off"
          autoCapitalize="characters"
          spellcheck={false}
          onInput={(e) => {
            const el = e.target as HTMLInputElement;
            const r = restOf(el.value, code);
            el.value = r.rest;
            set(r);
          }}
        />
      </div>
      {bad ? (
        <p class="control-error" {...tag("rest-error")}>
          {bad}
        </p>
      ) : (
        <p class="dim">
          {hint ?? (
            <>
              It's after the dot, {cli ? "" : "in Getting started or "}where you ran <code>{cli ? "arugula login" : "arugulad join"}</code>.
            </>
          )}
        </p>
      )}
    </div>
  );
}

/** M49: the Arugula CLI on some machine asks to be one of this account's
 * devices (`arugula login`). Approved, it reaches the account's machines
 * and can approve devices and machines, as this browser can. `sealed`:
 * its link carries a secret (#760), so nothing is compared. */
function CliJoin({
  s,
  j,
  cancel,
  refused,
  failed,
  sealed,
  secret,
  whole,
}: {
  s: ControlSession;
  j: JoinRequest;
  cancel: () => void;
  refused: boolean;
  failed: (e: unknown) => void;
  sealed: boolean;
  secret: string | null;
  whole: preact.ComponentChildren;
}) {
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  // Its name is "Arugula CLI on <host>" unless `--name` said otherwise:
  // the machine, here, since the sentence says it's the command line.
  const host = j.cert.name.replace(/^(Arugula|illogical) CLI on /, "");
  return (
    <Modal close={clearHash}>
      <h2>Add a terminal?</h2>
      {err ? (
        <p class="control-error" data-join-error>
          {err}
        </p>
      ) : null}
      {sealed ? (
        <p data-join-cli={j.cert.name} data-join-sealed>
          The Arugula command line on <b>{host}</b> asks to be one of your devices.
        </p>
      ) : (
        <>
          <p data-join-cli={j.cert.name}>
            The Arugula command line on <b>{host}</b> asks to be one of your devices, with code <b data-join-code={j.code}>{j.code}</b>. Check it's the
            code it shows where you ran <code>arugula login</code>.
          </p>
          <p class="dim">Its key: {fingerprint(j.cert.device)}</p>
        </>
      )}
      {whole}
      {s.enrollment && !sealed ? (
        <p>
          Your account:{" "}
          <b class="fingerprint" data-join-account={s.enrollment.root}>
            {fingerprint(s.enrollment.root)}
          </b>
          . Once you approve, it shows its account's fingerprint: check it's this one there.
        </p>
      ) : null}
      <p class="dim">It reaches your machines (directly or through control's relay, end to end encrypted) and can approve devices, as this one can.</p>
      <div class="prompt-buttons">
        <button data-cancel-join onClick={cancel}>
          {refused ? "Close" : "Cancel"}
        </button>
        <button
          class="primary"
          data-approve-join
          disabled={busy || (sealed && !secret)}
          onClick={async () => {
            setBusy(true);
            try {
              await s.approveJoin(j.code, j.cert, null, [], secret);
              clearHash();
            } catch (e) {
              setErr((e as Error).message);
              setBusy(false);
              failed(e);
            }
          }}
        >
          Approve
        </button>
      </div>
    </Modal>
  );
}

/** M48: the desktop app asks to sign in as this account. Allowing hands a
 * grant to the app on this computer (its loopback port): an app elsewhere
 * that sent this link gets nothing, so there's nothing to compare (#758).
 * From a device, it also signs the app's certificate, so the app is a
 * device at once; otherwise its window asks to be approved afterwards (the
 * "New device?" prompt). An app from before #758 still shows a code. */
function AppLoginPrompt({ s, id }: { s: ControlSession; id: string }) {
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
            {s.signsApp(a) ? <span data-app-login-device> Allowing also makes it one of your devices, so its window reaches your machines.</span> : null}
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

/** Back from handing the app its grant (and, from a device, its
 * certificate). */
function AppLoginDone() {
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
          Signed in, and the app is one of your devices now: its window reaches your machines. Its own machine joins separately, with a code, and
          doesn't need this.
        </p>
      ) : (
        <p data-app-login-done>
          Signed in. Next the app's window asks to be approved as a device, so it reaches your machines: the prompt shows here in a moment. Its own
          machine joins separately, with a code, and doesn't need this.
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
function ShareOfferPrompt({ s, o }: { s: ControlSession; o: ShareOffer }) {
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

/** Another device asks to be one of this account's. #761: one that shows
 * a code is approved with it (scanned with the camera, typed, or `given`
 * by the link its QR code holds): its lookup must be the request's keys'
 * code, and the approval answers its secret, so there's no fingerprint to
 * compare. One from before codes shows its fingerprint, as before. */
function DevicePrompt({ s, c, given, done }: { s: ControlSession; c: Cert; given?: DeviceCode; done?: () => void }) {
  const [err, setErr] = useState("");
  const [typed, setTyped] = useState<Rest>(NO_REST);
  const act = (f: () => Promise<void>) => f().then(() => done?.(), (e: Error) => setErr(e.message));
  const sealed = s.sealed(c);
  const want = s.pendingCodes.get(c.device) ?? "";
  const code = (given?.code === want ? given : null) ?? typedCode(want, typed);
  return (
    <Modal>
      <h2>New device?</h2>
      {sealed ? (
        <p data-pending={c.device} data-device-sealed>
          <b>{c.name}</b> wants to reach your machines.{" "}
          {code && code === given ? "Approve it if it's the one that showed you this code." : null}
        </p>
      ) : (
        <>
          <p>
            <b>{c.name}</b> wants to reach your machines. Approve it only if it's yours and shows this fingerprint:
          </p>
          <p class="fingerprint" data-pending={c.device}>
            {fingerprint(c.device)}
          </p>
        </>
      )}
      {sealed && !(code && code === given) ? <DeviceRest code={want} typed={typed} set={setTyped} /> : null}
      {err ? <p class="control-error">{err}</p> : null}
      <p class="dim">If you didn't just sign in on it, turn it down.</p>
      <div class="prompt-buttons">
        <button data-reject onClick={() => act(() => s.reject(c))}>
          Turn down
        </button>
        <button class="primary" data-approve disabled={sealed && !code} onClick={() => act(() => s.approve(c, code))}>
          Approve
        </button>
      </div>
    </Modal>
  );
}

/** #761: the rest of a waiting device's code, after its lookup. */
function DeviceRest({ code, typed, set, prompt }: { code: string; typed: Rest; set: (r: Rest) => void; prompt?: preact.ComponentChildren }) {
  return <RestOfCode what="device" code={code} rest={typed.rest} bad={typed.bad} set={set} prompt={prompt} hint="It's after the dot, on the screen asking to be approved." />;
}

/** #761: the code typed after `lookup`, once it's whole. */
function typedCode(lookup: string, typed: Rest): DeviceCode | null {
  const secret = typed.bad ? null : normalizeSecret(typed.rest);
  return lookup && secret ? { code: lookup, secret } : null;
}

/** #761: here from a waiting device's QR code or link (`#device=`): the
 * request whose keys give that lookup, approved with it. */
function DeviceLinkPrompt({ s, whole }: { s: ControlSession; whole: string }) {
  const given = splitDeviceCode(whole);
  const [looked, setLooked] = useState(false);
  useEffect(() => {
    // The request may be newer than this page's last look.
    void s.refresh().finally(() => setLooked(true));
  }, []);
  const c = given ? s.pending.find((p) => s.pendingCodes.get(p.device) === given.code) : undefined;
  // Keyed, like the overlay's: another device's prompt starts empty.
  if (c && given) return <DevicePrompt key={c.device} s={s} c={c} given={given} done={clearHash} />;
  return (
    <Modal close={clearHash}>
      <h2>New device?</h2>
      {looked ? (
        <p data-device-gone>
          No device is waiting with code <b>{given?.code ?? whole}</b>: it was approved or turned down already, or the code isn't whole.
        </p>
      ) : (
        <p class="dim">Looking for it…</p>
      )}
      <div class="prompt-buttons">
        <button class="primary" onClick={clearHash}>
          Close
        </button>
      </div>
    </Modal>
  );
}

/** The control URL as a QR code, a link to copy, and what to do (#105).
 * When the new device asks, DevicePrompt comes up over this. */
function AddDevice({ s, close }: { s: ControlSession; close: () => void }) {
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
function PasskeyNudge({ s }: { s: ControlSession }) {
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

const since = (ms: number) => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  return s < 60 ? "just now" : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 86400 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86400)}d ago`;
};

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

/** The account's recovery codes and its name: Sign-in and account's, with
 * the passkeys (they were at the foot of Devices and machines). */
export function RecoveryAndName({ s }: { s: ControlSession }) {
  const [renewing, setRenewing] = useState(false);
  const [err, setErr] = useState("");
  const left = s.recoveryLeft;
  return (
    <>
      <h3 class="control-group">Recovery codes</h3>
      <p class="dim">
        <span data-recovery-left={left}>
          {left === 0 ? "No recovery codes left." : `${left} recovery code${left === 1 ? "" : "s"} left.`}
        </span>{" "}
        {renewing ? "New codes replace these: the old ones stop working. " : ""}
        <button
          class={renewing ? "control-linkish danger" : "control-linkish"}
          data-new-codes
          onClick={() => {
            if (!renewing) return setRenewing(true);
            setRenewing(false);
            s.newRecoveryCodes().catch((e: Error) => setErr(e.message));
          }}
        >
          {renewing ? "Make new ones" : "Make new codes"}
        </button>
      </p>
      <NameLine s={s} />
      {err ? <p class="control-error">{err}</p> : null}
    </>
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

/** When a device was added, to the minute: with the last use beside it,
 * what tells apart two devices of the same name (#759). */
const addedAt = (ms: number) =>
  new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", ...(new Date(ms).getFullYear() === new Date().getFullYear() ? {} : { year: "numeric" }), hour: "numeric", minute: "2-digit" });

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

function DeviceDetails({ s, c }: { s: ControlSession; c: Cert }) {
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

function Devices({ s, close }: { s: ControlSession; close: () => void }) {
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
        <button class="control-linkish" data-to-account onClick={panel("account")}>
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

const panel = openPanel;

/** For the host menu. */
export function controlMenuItems(s: ControlSession): MenuItem[] {
  const shown = s.daemons.find((d) => d.name === directory.current);
  return [
    "separator",
    ...(s.stale ? [{ header: "Control unreachable" } as MenuItem] : []),
    ...(s.sandboxesOpen ? [{ label: "New hosted VM", run: () => void s.startSandbox() }] : []),
    ...(shown?.sandbox ? [{ label: "Delete this VM", run: () => void s.deleteSandbox(shown.sandbox!) }] : []),
    { label: "Add a machine…", run: panel("add") },
  ];
}

/** The account's own items, in the account menu on every page (the bar's,
 * and the no-machines screen's, #97). Sign out is the menu's own, last. */
export function accountItems(s: ControlSession, docs = false): MenuItem[] {
  return [
    { label: "Teams…", run: panel("teams") },
    { label: "Devices and machines…", run: panel("devices") },
    // #1066: follows the `docs` flag of the machine shown.
    ...(docs ? [{ label: "Shared Docs…", run: panel("shared-docs") } as MenuItem] : []),
    { label: "Cloud agents…", run: openPanel("cloud") },
    { label: "Add a phone or browser…", run: openPanel("add-device") },
    { label: "Sign-in and account…", run: panel("account") },
    ...(s.billing?.billing ? [{ label: s.billing.relay.warning ? "Plan and usage… (over the free relay)" : "Plan and usage…", run: panel("plan") }] : []),
  ];
}

const mb = (b: number) => `${(b / 1e6).toFixed(b < 1e7 ? 1 : 0)} MB`;

function Plan({ s, close }: { s: ControlSession; close: () => void }) {
  const b = s.billing;
  const [err, setErr] = useState("");
  if (!b) return null;
  return (
    <Modal close={close}>
      <h2>Plan and usage</h2>
      <p>
        You're on <b>{b.plan === "paid" ? "a paid plan" : "the free plan"}</b>. This month: {mb(b.relay.bytes)} through the relay
        {b.plan === "paid" ? "" : ` of ${mb(b.relay.allowance)} free`}, {b.sandbox_minutes} hosted VM minutes.
      </p>
      {b.relay.warning ? (
        <p class="control-error" data-relay-warning>
          You're over the free relay allowance{b.relay.slowed ? ", so relayed traffic is slowed down" : ""}. Direct connections (your tailnet, your LAN) don't count. A plan lifts it.
        </p>
      ) : null}
      {b.teams.map((t) => (
        <p key={t.team} data-team-plan={t.team}>
          <b>{t.name}</b>: {t.plan === "team" ? `team plan, ${t.seats} seats` : "free"}; {t.sandbox_minutes} VM minutes this month.{" "}
          {t.owner && t.plan !== "team" ? (
            <button class="control-linkish" onClick={() => s.upgrade(t.team).catch((e: Error) => setErr(e.message))}>
              Upgrade ({t.seats} seat{t.seats === 1 ? "" : "s"})
            </button>
          ) : null}
        </p>
      ))}
      {b.plan !== "paid" ? (
        <p>
          <button class="control-linkish" onClick={() => s.upgrade().catch((e: Error) => setErr(e.message))}>
            Add a payment method for hosted VMs (by the minute)
          </button>
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button onClick={close}>Done</button>
      </div>
    </Modal>
  );
}

/** An owner said yes to this account's request (#103). */
function Joined({ s }: { s: ControlSession }) {
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
function Notice({ s, n }: { s: ControlSession; n: ControlSession["notices"][number] }) {
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
function InvitePrompt({ s, team, code, old }: { s: ControlSession; team: string; code: string; old: boolean }) {
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
function PresignedPrompt({ s, team, seed }: { s: ControlSession; team: string; seed: string }) {
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
function AdmitPrompt({ s, team, req }: { s: ControlSession; team: Team; req: Team["requests"][number] }) {
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

/** The flags control may set: guest ssh and the agent catalog are each
 * machine's alone (`local_only`), set in its own Developer settings. */
const PUSHED = FLAGS.filter((f) => !f.local_only);

/** Labs flags set on control (#347), for an account's machines or a team's:
 * each unset (a machine follows its default), on or off. A machine's own
 * Developer settings win over these, and the account's over the team's. */
function LabsFlagList({ scope, what, flags, edit, set }: { scope: string; what: string; flags: Record<string, boolean>; edit: boolean; set: (name: string, on: boolean | null) => void }) {
  const named = PUSHED.filter((f) => flags[f.name] !== undefined).length;
  return (
    <details class="labs-flags" data-labs-flags={scope}>
      <summary>
        Labs flags <span class="dim">· {named ? `${named} set` : "none set"}</span>
      </summary>
      <p class="dim">{what}</p>
      <ul class="control-devices">
        {PUSHED.map((f) => {
          const v = flags[f.name];
          const value = v === undefined ? "" : v ? "on" : "off";
          return (
            <li key={f.name} data-labs-flag={f.name}>
              <span>
                {f.title}
                <span class="control-device-label dim">{f.about}</span>
              </span>
              {edit ? (
                <select
                  class="control-select"
                  value={value}
                  aria-label={`${f.title} for ${scope}`}
                  onChange={(e) => {
                    const x = (e.target as HTMLSelectElement).value;
                    set(f.name, x === "" ? null : x === "on");
                  }}
                >
                  <option value="">Not set</option>
                  <option value="on">On</option>
                  <option value="off">Off</option>
                </select>
              ) : (
                <span class="dim">{v === undefined ? "Not set" : v ? "On" : "Off"}</span>
              )}
            </li>
          );
        })}
      </ul>
    </details>
  );
}

function Teams({ s, close }: { s: ControlSession; close: () => void }) {
  const [name, setName] = useState("");
  const [err, setErr] = useState("");
  const act = (f: () => Promise<unknown>) => f().catch((e: Error) => setErr(e.message));
  // Labs flags (#347): the account's and each team's; null until loaded,
  // left out by a control from before them, and for someone with no
  // machine that has Developer settings (strangers never see Labs).
  const [labs, setLabs] = useState<LabsFlags | null>(null);
  useEffect(() => {
    let live = true;
    s.labsFlags().then(
      (l) => live && setLabs(l.dev ? l : null),
      () => live && setLabs(null),
    );
    return () => {
      live = false;
    };
  }, [s]);
  const setFlag = (team: string | undefined) => (flag: string, on: boolean | null) =>
    act(async () => {
      const { flags } = await s.setLabsFlag(flag, on, team);
      setLabs((l) => l && (team ? { ...l, teams: { ...l.teams, [team]: { ...l.teams[team], flags } } } : { ...l, account: flags }));
    });
  return (
    <Modal close={close}>
      <h2>Teams</h2>
      <p class="dim" data-teams-intro>
        A team shares its machines with its members. Each member:
      </p>
      <ul class="control-roles">
        {ROLE_HELP.map(([r, what]) => (
          <li key={r}>
            <b>{roleLabel(r)}</b>: {what}
          </li>
        ))}
      </ul>
      <p class="dim">
        An invite link lets one person in right away (with Ask me first, an owner says yes to each). Any member can add their own machines to a team; its owners can take them out.{" "}
        <a href="https://docs.arugula.io/teams/" target="_blank" rel="noreferrer">
          More about teams
        </a>
      </p>
      {s.teams.map((t) => (
        <TeamSection key={t.team} s={s} t={t} act={act} labs={labs?.teams[t.team]} setFlag={setFlag(t.team)} />
      ))}
      {labs ? (
        <section class="team" data-account-labs>
          <h3>Your machines</h3>
          <LabsFlagList
            scope="your machines"
            what="For every machine of yours. A machine's own Developer settings win over these, and these over a team's."
            flags={labs.account}
            edit
            set={setFlag(undefined)}
          />
        </section>
      ) : null}
      <form
        class="control-code"
        onSubmit={(e) => {
          e.preventDefault();
          void act(() => s.createTeam(name)).then(() => setName(""));
        }}
      >
        <input placeholder="New team's name" value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} aria-label="Team name" />
        <button type="submit" disabled={!name.trim()}>
          Make a team
        </button>
      </form>
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button onClick={close}>Done</button>
      </div>
    </Modal>
  );
}

function TeamSection({
  s,
  t,
  act,
  labs,
  setFlag,
}: {
  s: ControlSession;
  t: Team;
  act: (f: () => Promise<unknown>) => void;
  labs?: { flags: Record<string, boolean>; owner: boolean };
  setFlag: (name: string, on: boolean | null) => void;
}) {
  const [role, setRole] = useState<TeamRole>("editor");
  const [link, setLink] = useState<string | null>(null);
  // An invite that waits for an owner's yes (the old way), not presigned.
  const [askFirst, setAskFirst] = useState(false);
  const [linkAsks, setLinkAsks] = useState(false);
  const [linkWhy, setLinkWhy] = useState("");
  // Which button waits for a second click: "lock".
  const [confirming, setConfirming] = useState<string | null>(null);
  // The member whose Remove was clicked: a dialog asks.
  const [removing, setRemoving] = useState<string | null>(null);
  const owner = t.role === "owner";
  // A member's machine an owner is taking out of the team (#332).
  const [takingOut, setTakingOut] = useState<string | null>(null);
  const machines = s.daemons.filter((d) => d.team === t.team);
  // One-click links not used yet (#134), each with Cancel.
  const [unused, setUnused] = useState<PresignedInvite[]>([]);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    if (!owner) return setUnused([]);
    let live = true;
    s.presignedInvites(t.team).then(
      (l) => live && setUnused(l),
      () => live && setUnused([]),
    );
    return () => {
      live = false;
    };
  }, [s, t.team, owner, t.locked, t.roster.version, link, reload]);
  return (
    <section class="team" data-team={t.team}>
      <h3>
        {t.roster.name} {t.locked ? <span class="control-error">· locked</span> : null}
      </h3>
      {t.locked && !owner ? (
        <p class="dim" data-team-add-locked>
          It's locked: only its owners add machines to it until it's unlocked.
        </p>
      ) : (
        <>
          <p class="dim">
            Team id <CopyText inline text={t.team} data-team-id />.
          </p>
          <TeamJoin s={s} t={t} here />
        </>
      )}
      <h4 class="control-subgroup" data-team-members-head>
        Members <span class="dim">· {t.roster.members.length}</span>
      </h4>
      <ul class="control-devices" data-team-members={t.team}>
        {t.roster.members.map((m) => (
          <li key={m.account} data-member={m.account}>
            <span data-member-name>
              {t.names?.[m.account] ?? m.name}
              {m.account === s.account ? " (you)" : ""}
            </span>
            {owner && m.account !== s.account ? (
              <select
                class="control-select"
                value={m.role}
                aria-label={`${t.names?.[m.account] ?? m.name}'s role`}
                onChange={(e) => {
                  const r = (e.target as HTMLSelectElement).value as TeamRole;
                  act(() => s.changeTeam(t.team, (ms) => ms.map((x) => (x.account === m.account ? { ...x, role: r } : x))));
                }}
              >
                <RoleOptions />
              </select>
            ) : (
              <span class="dim">{roleLabel(m.role)}</span>
            )}
            {owner && m.account !== s.account ? (
              <button class="control-revoke quiet-danger" data-remove-member={m.account} title="They lose access to the team's machines at once" onClick={() => setRemoving(m.account)}>
                Remove
              </button>
            ) : (
              <span />
            )}
          </li>
        ))}
      </ul>
      {removing ? (
        <ConfirmRemove
          title={`Remove ${t.names?.[removing] ?? t.roster.members.find((m) => m.account === removing)?.name ?? "them"} from ${t.roster.name}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            setRemoving(null);
            act(() => s.changeTeam(t.team, (ms) => ms.filter((x) => x.account !== removing)));
          }}
        >
          <p>They lose the team's machines at once. To come back, they need a new invite.</p>
        </ConfirmRemove>
      ) : null}
      <h4 class="control-subgroup" data-team-machines-head>
        Machines <span class="dim">· {machines.length}</span>
      </h4>
      {machines.length ? (
        <ul class="control-devices" data-team-machines={t.team}>
          {machines.map((d) => {
            const mine = !d.account || d.account === s.account;
            return (
              <li key={d.id} data-team-machine={d.id}>
                <span>
                  {d.name}
                  <span class="dim">{mine ? " · yours" : ` · ${d.owner_name || "a member"}'s`}</span>
                </span>
                <span class="dim" data-status>
                  {machineState(d.name)}
                </span>
                {owner && !mine ? (
                  <button class="control-revoke quiet-danger" data-take-out={d.id} title="Take it out of the team; it stays its owner's" onClick={() => setTakingOut(d.id)}>
                    Take out
                  </button>
                ) : (
                  <span />
                )}
                {takingOut === d.id ? (
                  <p class="dim control-explain" data-take-out-explain>
                    {t.roster.name}'s members lose {d.name} at once. It stays {d.owner_name || "its owner"}'s, and they can add it again.{" "}
                    <button data-take-out-cancel onClick={() => setTakingOut(null)}>
                      Cancel
                    </button>{" "}
                    <button
                      class="danger"
                      data-take-out-go
                      onClick={() => {
                        setTakingOut(null);
                        act(() => s.moveDaemon(d.id, null));
                      }}
                    >
                      Take it out
                    </button>
                  </p>
                ) : null}
              </li>
            );
          })}
        </ul>
      ) : (
        <p class="dim">None yet.</p>
      )}
      {labs ? (
        <LabsFlagList
          scope={t.roster.name}
          what={
            labs.owner
              ? "For the team's machines. A machine's own Developer settings win over these, and so do its owner's flags for their machines."
              : "For the team's machines, set by its owners. A machine's own Developer settings win over these."
          }
          flags={labs.flags}
          edit={labs.owner}
          set={setFlag}
        />
      ) : null}
      {owner ? (
        <>
          <div class="control-code control-invite">
            <label>
              Invite{" "}
              <select class="control-select" value={role} aria-label="Invite as" data-invite-role={t.team} onChange={(e) => setRole((e.target as HTMLSelectElement).value as TeamRole)}>
                {ROLE_HELP.map(([r]) => (
                  <option key={r} value={r}>
                    {roleAs(r).replace(/^as /, "")}
                  </option>
                ))}
              </select>
            </label>
            <label class="control-check" title="Each person waits for an owner's yes, as before">
              <input type="checkbox" data-invite-ask-first checked={askFirst || role === "owner"} disabled={role === "owner"} onChange={(e) => setAskFirst((e.target as HTMLInputElement).checked)} />{" "}
              Ask me first
            </label>
            <button
              data-invite={t.team}
              disabled={t.locked}
              onClick={() =>
                act(async () => {
                  const l = await s.makeInvite(t.team, role, askFirst);
                  setLinkAsks(l.asks);
                  setLinkWhy(l.why ?? "");
                  setLink(l.link);
                })
              }
            >
              Make a link
            </button>
          </div>
          {link && linkWhy ? (
            <p class="dim" data-invite-why>
              {linkWhy}, so this link asks you first. Rerun the install command on them to update.
            </p>
          ) : null}
          {link ? (
            <p>
              {linkAsks
                ? "Anyone with this link can ask to join (for a week); you approve each:"
                : "One person can join with this link, within a day, and you're told when they do. Send it only to them:"}
              <CopyText text={link} share data-invite-link />
            </p>
          ) : null}
          {unused.length ? (
            <>
              <p class="dim">One-click links nobody has used yet:</p>
              <ul class="control-devices" data-presigned-list>
                {unused.map((i) => (
                  <li key={i.key} data-presigned={i.key}>
                    <span>
                      {roleAs(i.role).replace(/^as /, "for ")}
                      {i.by === s.account ? "" : `, from ${i.by_name}`}
                    </span>
                    <span class="dim">{expiresIn(i.expires)}</span>
                    <button
                      class="control-revoke"
                      data-cancel-presigned={i.key}
                      title="Nobody can join with this link any more"
                      onClick={() =>
                        act(async () => {
                          await s.cancelPresigned(t.team, i.key);
                          setReload((n) => n + 1);
                        })
                      }
                    >
                      Cancel
                    </button>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
          <div class="team-lock">
            <p class={confirming === "lock" ? "control-error" : "dim"} data-lock-warning={confirming === "lock" ? "" : undefined}>
              {t.locked
                ? "Locked: only owners reach the team's machines and add to it."
                : "Locking lets only owners reach the team's machines; open invites and requests are dropped."}
            </p>
            {confirming === "lock" ? <button onClick={() => setConfirming(null)}>Cancel</button> : null}
            <button
              class={t.locked ? "control-revoke" : confirming === "lock" ? "control-revoke danger" : "control-revoke quiet-danger"}
              data-lock={t.team}
              onClick={() => {
                if (!t.locked && confirming !== "lock") return setConfirming("lock");
                setConfirming(null);
                setLink(null);
                act(() => s.lockTeam(t.team, !t.locked));
              }}
            >
              {t.locked ? "Unlock" : confirming === "lock" ? "Lock it" : "Lock team…"}
            </button>
          </div>
        </>
      ) : null}
    </section>
  );
}

/** How long a link has left, roughly. */
function expiresIn(at: number): string {
  const min = Math.max(1, Math.round((at - Date.now()) / 60_000));
  return min < 90 ? `expires in ${min} min` : `expires in ${Math.round(min / 60)} h`;
}

function RoleOptions() {
  return (
    <>
      {ROLE_HELP.map(([r]) => (
        <option key={r} value={r}>
          {roleLabel(r)}
        </option>
      ))}
    </>
  );
}
