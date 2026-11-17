// Everything on control's page before the app (M17): signing in (GitHub or
// a passkey), and why you're here when a link brought you. Signing in is
// the whole flow (aru-141, #1004): no screen waits for another device to
// approve this browser.

import { useEffect, useState } from "preact/hooks";
import { inviteInHash, passkeyRegister, passkeySignIn, previewInvite, signInNext, type ControlSession } from "../control";
import { Center, useControl } from "./common";
import { Mark } from "../../shared/ui/icons";

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

/** Everything before the app: connecting, signing in, or what went wrong. */
export function ControlGate({ s }: { s: ControlSession }) {
  useControl(s);
  if (s.phase === "loading") return <Center>Connecting to Arugula cloud…</Center>;
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
        <h1 class="control-brand">
          <Mark />
          Arugula
        </h1>
        <WhyHere />
        <p>Your terminals, on every machine, from any device. This service decides who reaches your machines and relays for them: it can read what it relays, and keeps none of it.</p>
        <div class="control-signins">
          {s.info.github ? (
            <a class="primary control-signin" href={`/auth/github?next=${next}`} data-signin="github">
              Sign in with GitHub
            </a>
          ) : null}
          {s.info.passkeys ? <PasskeyButtons /> : null}
        </div>
        {!s.info.github && !s.info.passkeys ? <p class="control-error">No sign-in is configured on this server.</p> : null}
        <HostedTerms />
      </Center>
    );
  }
  return null;
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
        Sign in to add this machine.
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
        <button class="secondary control-signin" data-signup="passkey" onClick={() => setNaming(true)}>
          New here? Make an account with a passkey
        </button>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
