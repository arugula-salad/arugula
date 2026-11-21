// Sign-in and account (#173): where you're signed in (sign one out, or
// everywhere), your passkeys and GitHub link (#208; never the last way
// in), and deleting the account, after typing its login. Its usage reports switch is shared
// (../../shared/ui/account-telemetry.tsx), handed the account's answer.

import { useEffect, useRef, useState } from "preact/hooks";
import { UsageReports } from "../../shared/ui/account-telemetry";
import { api, passkeyRemoval, passkeyRemove, shownName, type ControlSession, type PasskeyRemoval } from "../control";
import { desktopApp } from "../../shared/desktop";
import { BrowserStep, browserButton } from "../../shared/ui/browser-step";
import { currentHand, lending, setLending } from "../hand";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Time } from "../../shared/ui/time";
import { EmailSection, shellCall } from "./email";
import { describeAgent, LINKED, passkeyName, type Passkeys, type SessionRow } from "../../shared/account";
import { DeleteAccountSteps } from "../../shared/ui/delete-account";
import { PasskeyResetOffer } from "./reset";

/** A sign-in's name: its device's, once its page said which it is, so
 * the same browser has one name in both lists. */
export function sessionName(s: ControlSession, x: SessionRow): string {
  const c = x.device ? s.trusted.get(x.device) : undefined;
  return c ? shownName(c.name) : describeAgent(x.agent);
}

/** Why the only passkey's Remove waits (DK10, #393). */
const ONLY_PASSKEY = "Removing a passkey takes another of yours: add another passkey first, or, if you've lost it, start a passkey reset.";

/** Where linking GitHub comes back to: `#github-link=<what happened>`. */
export const GITHUB_LINK_HASH = /^#github-link=([a-z-]+)$/;

/** The page in a browser that opens Sign-in and account: where the desktop
 * app sends Link GitHub, as its window can't link (#235). */
export const ACCOUNT_HASH = "#account";

/** The account actions the desktop app's window can't do (control refuses
 * them from it): each opens the browser step instead (#251). */
type Away = "end-all" | "remove-passkey" | "add-passkey" | "link-github" | "unlink-github" | "delete";

/** What the browser can change here, as one string: the passkeys and the
 * GitHub link. Not the sign-ins: signing that browser in adds one, and
 * signing out everywhere or deleting ends this window's too. */
const fingerprint = (keys: Passkeys | null) => JSON.stringify([keys?.passkeys.map((k) => k.id), keys?.github ?? null]);

/** What other people see (#102), changed in place: Sign-in and account's
 * own row under its title (UI audit #174: it sat under Recovery codes). */
export function NameLine({ s }: { s: ControlSession }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(s.name);
  const [err, setErr] = useState("");
  if (!editing)
    return (
      <p class="dim">
        Name <b data-account-name>{s.name || "none yet"}</b>{" "}
        <button
          class="link"
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
      <button type="submit" class="primary" data-save-name disabled={!name.trim()}>
        Save
      </button>
      {err ? <p class="control-error">{err}</p> : null}
    </form>
  );
}

export function AccountPanel({ s, close, linked, emailBack }: { s: ControlSession; close: () => void; linked?: string; emailBack?: string }) {
  const [sessions, setSessions] = useState<SessionRow[] | null>(null);
  const [keys, setKeys] = useState<Passkeys | null>(null);
  const [err, setErr] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  // The passkey whose Remove was clicked: a dialog asks, and another of
  // the account's passkeys agrees (DK10, #393): what it signs, fetched as
  // the dialog opens, or why not.
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null);
  const [removal, setRemoval] = useState<{ id: string; options?: PasskeyRemoval; err?: string } | null>(null);
  const [unlinking, setUnlinking] = useState(false);
  const [deleting, setDeleting] = useState(false);
  // The desktop app's window can't link GitHub or change passkeys, sign-ins
  // or the account (control refuses it): the browser does, and the window
  // asks again when the person is back (#235, #251). Which button sent them,
  // and what the panel showed then.
  const [away, setAway] = useState<Away | null>(null);
  const before = useRef({ print: "", github: null as string | null });
  const seen = useRef<string | null>(null);
  const stepRef = useRef<HTMLDivElement>(null);
  const app = !!desktopApp();
  // Loaded, so what the browser changes is told from what was here.
  const waiting = away !== null || (app && !keys);
  const toBrowser = (a: Away) => {
    before.current = { print: fingerprint(keys), github: keys?.github ?? null };
    setAway(a);
  };
  // The step is one, below the panel's buttons: bring it into view.
  useEffect(() => void (away && stepRef.current?.scrollIntoView?.({ block: "nearest" })), [away]);
  const [browserLinked, setBrowserLinked] = useState(false);
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
  // DK10: removing a passkey takes another, so the only one stays.
  const onlyPasskey = keys?.passkeys.length === 1;
  const askToRemove = (id: string, name: string) => {
    setRemoving({ id, name });
    setRemoval({ id });
    passkeyRemoval(id).then(
      (options) => setRemoval((r) => (r?.id === id ? { id, options } : r)),
      (e: Error) => setRemoval((r) => (r?.id === id ? { id, err: e.message } : r)),
    );
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
      {app ? (
        <p class="dim" data-account-in-browser>
          Passkeys, GitHub, signing out everywhere and deleting the account are changed in a browser or on a phone: these open it there.
        </p>
      ) : null}
      <p class="dim">
        Signed in as <b>{s.login}</b>
        {keys?.github ? ` (GitHub: ${keys.github})` : ""}.
      </p>
      <NameLine s={s} />
      <h3 class="control-group">Where you're signed in</h3>
      <ul class="control-devices control-rows" data-sessions>
        {(sessions ?? []).filter((x) => !earlier(x)).map((x) => (
          <li key={x.id} data-session={x.id}>
            <span>
              {sessionName(s, x)}
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
                onClick={() => (confirming === x.id ? act(() => api(`/api/me/sessions/${x.id}/end`, {})) : setConfirming(x.id))}
              >
                {confirming === x.id ? "Really sign out?" : "Sign out"}
              </button>
            )}
          </li>
        ))}
      </ul>
      <p class="dim">
        {others ? `${others} other session${others === 1 ? "" : "s"}${unnamed ? `, ${unnamed} of them from before Arugula kept which browser` : ""}. ` : "Nowhere else. "}
        <button
          class={confirming === "all" ? "link danger" : "link"}
          data-end-all
          disabled={waiting}
          {...browserButton(confirming === "all" ? "Sign out everywhere, here too?" : "Sign out everywhere")}
          onClick={() => {
            if (app) return toBrowser("end-all");
            if (confirming !== "all") return setConfirming("all");
            api("/api/me/sessions/end-all", {}).then(
              () => void s.signOut(false),
              (e: Error) => setErr(e.message),
            );
          }}
        />
      </p>
      {s.info.passkeys || keys?.passkeys.length ? (
        <>
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
                  ) : (
                    ""
                  )}
                </span>
                <span class="control-row-actions">
                <button
                  class="secondary danger small"
                  data-remove-passkey={k.id}
                  disabled={waiting || onlyPasskey || (keys?.ways ?? 0) <= 1}
                  {...browserButton("Remove")}
                  title={onlyPasskey ? ONLY_PASSKEY : (keys?.ways ?? 0) <= 1 ? "Your only way to sign in" : app ? "Opens in your browser" : undefined}
                  onClick={() => (app ? toBrowser("remove-passkey") : askToRemove(k.id, passkeyName(k, i)))}
                />
                </span>
              </li>
            ))}
          </ul>
          {onlyPasskey ? (
            <p class="dim" data-only-passkey>
              {ONLY_PASSKEY}
            </p>
          ) : null}
          {removing ? (
            <ConfirmRemove
              title={`Remove ${removing.name}?`}
              busy={!(removal?.id === removing.id && removal.options)}
              cancel={() => {
                setRemoving(null);
                setRemoval(null);
              }}
              go={() => {
                const id = removing.id;
                const options = removal?.id === id ? removal.options : undefined;
                if (!options) return;
                setRemoving(null);
                setRemoval(null);
                act(async () => {
                  await passkeyRemove(id, options);
                  await s.refreshMe();
                });
              }}
            >
              <p>It stops signing you in at once. To use it again, add it as a new passkey.</p>
              <p class="dim" data-remove-passkey-with>
                Confirm with another of your passkeys: your browser asks for one when you press Remove.
              </p>
              {removal?.id === removing.id && removal.err ? (
                <p class="control-error" data-remove-passkey-error>
                  {removal.err}
                </p>
              ) : null}
            </ConfirmRemove>
          ) : null}
          {s.info.passkeys ? (
            <button
              class="secondary"
              data-add-passkey
              disabled={waiting}
              {...browserButton("Add a passkey")}
              onClick={() => (app ? toBrowser("add-passkey") : act(() => s.addPasskey()))}
            />
          ) : null}
          {/* DK10: the way back from lost passkeys; it takes GitHub. */}
          {!app && keys?.github ? <PasskeyResetOffer done={load} back={ACCOUNT_HASH} /> : null}
        </>
      ) : null}
      <EmailSection back={emailBack} />
      {s.info.github || keys?.github ? (
        <>
          <h3 class="control-group">GitHub</h3>
          {browserLinked ? (
            <p class="dim" data-github-link="linked">
              {LINKED.linked}
            </p>
          ) : linked ? (
            <p class={linked === "linked" ? "dim" : "control-error"} data-github-link={linked}>
              {LINKED[linked] ?? LINKED.failed}
            </p>
          ) : null}
          <ul class="control-devices control-rows" data-github>
            <li>
              <span data-github-login>{keys?.github ?? "Not linked"}</span>
              <span class="dim">{keys?.github ? "signs you in" : "sign in with GitHub too, and use its repos"}</span>
              {!keys ? (
                <span />
              ) : keys.github ? (
                <button
                  class="secondary danger small"
                  data-unlink-github
                  disabled={waiting || keys.ways <= 1}
                  {...browserButton("Unlink")}
                  title={keys.ways <= 1 ? "Your only way to sign in" : app ? "Opens in your browser" : undefined}
                  onClick={() => (app ? toBrowser("unlink-github") : setUnlinking(true))}
                />
              ) : (
                <button
                  class="secondary small"
                  data-link-github
                  disabled={waiting}
                  {...browserButton("Link GitHub")}
                  onClick={() =>
                    app
                      ? toBrowser("link-github")
                      : location.assign(`/auth/github?link=1&next=${encodeURIComponent(location.pathname + location.search)}`)
                  }
                />
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
                act(async () => {
                  await api("/api/me/github/unlink", {});
                  await s.refreshMe();
                });
              }}
            >
              <p>It stops signing you in at once, and features that use your GitHub account stop until you link it again.</p>
            </ConfirmRemove>
          ) : null}
        </>
      ) : null}
      <LendDevice s={s} />
      <UsageReports telemetry={s.telemetry} setTelemetry={(on) => s.setTelemetry(on)} />
      <h3 class="control-group">Delete your account</h3>
      <p class="dim">
        Removes your account from this server, with your machines, sessions and passkeys.{" "}
        <button
          class="link danger"
          data-delete-account
          disabled={waiting}
          {...browserButton("Delete account…")}
          onClick={() => (app ? toBrowser("delete") : setDeleting(true))}
        />
      </p>
      {away ? (
        <div ref={stepRef}>
          <BrowserStep
            url={`${location.origin}/${ACCOUNT_HASH}`}
            label="Open Sign-in and account in your browser"
            // Done when the browser changed the passkeys or the GitHub link. A
            // 401 (the browser signed this window out) throws: the directory
            // poll then takes the window to signed-out.
            check={async () => {
              const ks = await api<Passkeys>("/api/me/passkeys");
              seen.current = ks.github;
              return fingerprint(ks) !== before.current.print;
            }}
            onDone={() => {
              if (!before.current.github && seen.current) setBrowserLinked(true);
              setAway(null);
              load();
              void s.refreshMe();
            }}
            cancel={() => setAway(null)}
          />
        </div>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={close}>Done</button>
      </div>
    </>
  );
}

function DeleteAccount({ s, back }: { s: ControlSession; back: () => void }) {
  return (
    <>
      <h2>Delete your account</h2>
      <DeleteAccountSteps call={shellCall} back={back} deleted={() => void s.signOut(true)} />
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
          role="switch"
          class="sw-switch"
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
