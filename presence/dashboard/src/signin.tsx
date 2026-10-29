// The dashboard's front door: sign in by an emailed link or Google (P3),
// coming back to the page you asked for. There's no separate sign-up: the
// first link you follow makes your account.

import { useState } from "preact/hooks";
import * as api from "./api";
import type { Providers } from "./api";
import { value } from "./ui";

export function SignInPage({ ways, returnTo }: { ways: Providers; returnTo: string }) {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async () => {
    const e = email.trim();
    if (!e || busy) return;
    setBusy(true);
    try {
      await api.emailLink(e, returnTo);
      setSent(e);
      setErr("");
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <main class="signin-page">
      <div class="intro">
        <h1 class="headline">
          Your apps, <span>your keys.</span>
        </h1>
        <p class="lede">Make an app, copy its key into your page, and tell us how your people sign in. No server of yours needed. New here? Signing in makes your account.</p>
      </div>
      <section class="card panel" style={{ "--c": "var(--k-server)" }}>
        <h2>Sign in</h2>
        {!ways.email && !ways.google ? (
          <p class="sub">Signing in isn't set up on this server yet.</p>
        ) : sent ? (
          <p class="sent" role="status">
            Check your email: a sign-in link is on its way to <b>{sent}</b>. It works once, for 15 minutes.
          </p>
        ) : (
          <div class="signin">
            {ways.email && (
              <form
                class="row"
                onSubmit={(e) => {
                  e.preventDefault();
                  void send();
                }}
              >
                <input type="email" placeholder="you@example.com" aria-label="Your email" autocomplete="email" value={email} onInput={(e) => setEmail(value(e))} />
                <button class="btn primary" type="submit" disabled={!email.trim() || busy}>
                  Email me a link
                </button>
              </form>
            )}
            {ways.google && (
              <a class="btn google" href={api.googleUrl(returnTo)}>
                Sign in with Google
              </a>
            )}
            {err && <span class="error">{err}</span>}
          </div>
        )}
      </section>
    </main>
  );
}
