// Settings › You › Sign-in and security (Arugula #1311): control's Sign-in
// and account dialog (shell/trust/account.tsx), drawn in place through the
// app's host. Your passkeys (a list: adding or removing one is a ceremony
// control takes only on its own page, aru-078, so that's a ↗ row there
// until the single origin, aru-141), GitHub (unlinked here; linking it
// needs the hub's cookie, so ↗ too) and your email, where you're signed in
// (sign one out, or everywhere), and deleting the account.

import { useEffect, useState } from "preact/hooks";
import { describeAgent, LINKED, passkeyName, type AccountCall, type Passkeys, type SessionRow } from "../../shared/account";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { EmailSection } from "../../shared/ui/account-email";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { DeleteAccountSteps } from "../../shared/ui/delete-account";
import { ExternalIcon } from "../../shared/ui/icons";
import { Section } from "../../shared/ui/settings-section";
import { Time } from "../../shared/ui/time";
import { controlOrigin } from "../session";
import { ControlRow } from "../settings/owner-machines";
import { joined, plural } from "../settings/summary";
import { signOut } from "../signin";

/** `/api/me/*` through the app's host. */
const appCall: AccountCall = async <T,>(method: "GET" | "POST" | "DELETE", path: string, body?: unknown): Promise<T> => {
  const send = method === "POST";
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: send ? { "Content-Type": "application/json" } : {},
    body: send ? JSON.stringify(body ?? {}) : undefined,
  });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
};

/** Control's dialog, at the top level: where what this page can't do is done. */
const accountHref = () => `${controlOrigin()}/#account`;

/** Out of this page, and back to signed out: after signing out everywhere
 * or deleting the account, this page's session is gone too. */
const leave = () => void signOut().then(() => location.reload());

function Away({ label, hook }: { label: string; hook: string }) {
  return (
    <a href={accountHref()} data-security-away={hook}>
      {label} <ExternalIcon size="xs" />
    </a>
  );
}

export function SignInAndSecurity() {
  const [sessions, setSessions] = useState<SessionRow[] | null>(null);
  const [keys, setKeys] = useState<Passkeys | null>(null);
  const [err, setErr] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  const [unlinking, setUnlinking] = useState(false);
  const [deleting, setDeleting] = useState(false);
  // What linking GitHub on control's page came back with, when it came back here.
  const [linked] = useState(() => /^#github-link=([a-z-]+)$/.exec(location.hash)?.[1]);
  const failed = (e: Error) => setErr(e.message);
  const load = () => {
    appCall<{ sessions: SessionRow[] }>("GET", "/api/me/sessions").then((r) => setSessions(r.sessions), failed);
    appCall<Passkeys>("GET", "/api/me/passkeys").then(setKeys, failed);
  };
  useEffect(load, []);
  const act = (f: () => Promise<unknown>) => {
    setErr("");
    setConfirming(null);
    f().then(load, failed);
  };
  const lede = "How you sign in, and where you're signed in.";
  // Sign-ins from before control kept the browser all read "An earlier
  // sign-in": folded into a count (UI audit #174).
  const earlier = (x: SessionRow) => !x.current && !x.agent;
  const others = (sessions ?? []).filter((x) => !x.current).length;
  const unnamed = (sessions ?? []).filter(earlier).length;
  const ways = keys?.ways ?? 0;
  const summary =
    keys && sessions ? joined(plural(keys.passkeys.length, "passkey"), keys.github ? "GitHub linked" : null, plural(sessions.length, "sign-in")) : err ? "Couldn't load" : undefined;
  return (
    <Section id="security" title="Sign-in and security" summary={summary} lede={lede} hooks={{ "data-settings-security": "" }}>
      <h3 class="control-group">Passkeys</h3>
      {keys && keys.passkeys.length === 0 ? <p class="dim">None.</p> : null}
      <ul class="control-devices control-rows control-passkeys" data-passkeys>
        {(keys?.passkeys ?? []).map((k, i) => (
          <li key={k.id} data-passkey={k.id}>
            <span data-passkey-name>{passkeyName(k, i)}</span>
            <span class="dim">
              added <Time at={k.created} on />
              {k.used ? (
                <>
                  , last used <Time at={k.used} on />
                </>
              ) : null}
            </span>
            <span />
          </li>
        ))}
      </ul>
      <ul class="platform-list settings-rows">
        <ControlRow href={accountHref()} label="Add or remove a passkey" why="It asks your browser, on your machines page" hook="passkeys" />
      </ul>

      <h3 class="control-group">Linked</h3>
      {linked ? (
        <p class={linked === "linked" ? "dim" : "control-error"} data-github-link={linked}>
          {LINKED[linked] ?? LINKED.failed}
        </p>
      ) : null}
      <ul class="control-devices control-rows" data-github>
        <li>
          <span data-github-login>{keys?.github ? `GitHub: ${keys.github}` : "GitHub: not linked"}</span>
          <span class="dim">{keys?.github ? "signs you in" : "sign in with GitHub too, and use its repos"}</span>
          {!keys ? (
            <span />
          ) : keys.github ? (
            <button
              class="secondary danger small"
              data-unlink-github
              disabled={ways <= 1}
              title={ways <= 1 ? "Your only way to sign in" : undefined}
              onClick={() => setUnlinking(true)}
            >
              Unlink
            </button>
          ) : (
            <span class="control-row-actions">
              <Away label="Link GitHub" hook="link-github" />
            </span>
          )}
        </li>
      </ul>
      {unlinking ? (
        <ConfirmRemove
          title={`Unlink GitHub (${keys?.github ?? ""})?`}
          label="Unlink"
          cancel={() => setUnlinking(false)}
          go={() => {
            setUnlinking(false);
            act(() => appCall("POST", "/api/me/github/unlink"));
          }}
        >
          <p>It stops signing you in at once, and features that use your GitHub account stop until you link it again.</p>
        </ConfirmRemove>
      ) : null}
      <EmailSection call={appCall} heading={false} />

      <h3 class="control-group">Where you're signed in</h3>
      <ul class="control-devices control-rows" data-sessions>
        {(sessions ?? [])
          .filter((x) => !earlier(x))
          .map((x) => (
            <li key={x.id} data-session={x.id}>
              <span>
                {describeAgent(x.agent)}
                {x.current ? " (this browser)" : ""}
              </span>
              <span class="dim">
                signed in <Time at={x.created} on />
              </span>
              {x.current ? (
                <span />
              ) : (
                <button
                  class={confirming === x.id ? "primary danger small" : "secondary danger small"}
                  data-end-session={x.id}
                  onClick={() => (confirming === x.id ? act(() => appCall("POST", `/api/me/sessions/${encodeURIComponent(x.id)}/end`)) : setConfirming(x.id))}
                >
                  {confirming === x.id ? "Really sign out?" : "Sign out"}
                </button>
              )}
            </li>
          ))}
      </ul>
      {sessions ? (
        <p class="dim">
          {others ? `${plural(others, "other session")}${unnamed ? `, ${unnamed} of them from before Arugula kept which browser` : ""}. ` : "Nowhere else. "}
          <button
            class={confirming === "all" ? "link danger" : "link"}
            data-end-all
            onClick={() => {
              if (confirming !== "all") return setConfirming("all");
              setErr("");
              setConfirming(null);
              appCall("POST", "/api/me/sessions/end-all").then(leave, failed);
            }}
          >
            {confirming === "all" ? "Sign out everywhere, here too?" : "Sign out everywhere"}
          </button>
        </p>
      ) : null}

      <h3 class="control-group">Delete your account</h3>
      {deleting ? (
        <div data-delete-account-steps>
          <DeleteAccountSteps call={appCall} back={() => setDeleting(false)} deleted={leave} />
        </div>
      ) : (
        <p class="dim">
          Removes your account from this server, with your machines, sessions and passkeys.{" "}
          <button class="link danger" data-delete-account onClick={() => setDeleting(true)}>
            Delete account…
          </button>
        </p>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </Section>
  );
}
