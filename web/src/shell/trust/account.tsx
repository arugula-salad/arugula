// Sign-in and account (#173): where you're signed in (sign one out, or
// everywhere), your passkeys and GitHub link (#208; never the last way
// in), and deleting the account, after typing its login. Its usage reports switch is shared
// (../../shared/ui/account-telemetry.tsx), handed the account's answer.

import { useEffect, useRef, useState } from "preact/hooks";
import { UsageReports } from "../../shared/ui/account-telemetry";
import { agentName, api, passkeyRemoval, passkeyRemove, shownName, type ControlSession, type PasskeyRemoval } from "../control";
import { desktopApp } from "../../shared/desktop";
import { BrowserStep, browserButton } from "../../shared/ui/browser-step";
import { currentHand, lending, setLending } from "../hand";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { Time } from "../../shared/ui/time";
import { NameLine, RecoveryLeft } from "./recovery";
import { EmailSection } from "./email";

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


/** What a sign-in or passkey came from, in the words Devices uses for a
 * browser ("Safari on iPhone"). Sign-ins from before control kept the
 * browser have none. */
export function describeAgent(ua: string): string {
  if (!ua) return "An earlier sign-in";
  const name = agentName(ua);
  // UI audit #195: a sign-in from no browser, named by what it is. The CLI
  // keeps none (it signs each call with its device key), and the desktop
  // app's window names its device once its page loads (sessionName), so
  // what's left signed in without a browser is a headless device (the
  // tests' and dev-control's) or a script: named by its runtime's word.
  const runtime = name === "A program" ? /\b(node|undici|curl|reqwest)\b/i.exec(ua)?.[1] : null;
  return runtime ? `A headless device (${runtime.toLowerCase()})` : name;
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

/** Why the only passkey's Remove waits (DK10, #393). */
const ONLY_PASSKEY = "Removing a passkey takes another of yours: add another passkey first.";

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

/** What linking GitHub came back with, for the dialog to say. */
const LINKED: Record<string, string> = {
  linked: "GitHub is linked: you can sign in with it too.",
  taken: "That GitHub account already signs in to another Arugula account. Nothing changed.",
  "has-other": "This account has another GitHub account linked: unlink it first.",
  elsewhere: "That link was started in another browser or sign-in: press Link GitHub again here.",
  expired: "That link took too long or was used already: press Link GitHub again.",
  declined: "GitHub didn't link: it was declined on GitHub.",
  failed: "Linking GitHub failed: try again.",
};

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
      <RecoveryLeft s={s} />
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
        {others ? `${others} other session${others === 1 ? "" : "s"}${unnamed ? `, ${unnamed} of them from before control kept which browser` : ""}. ` : "Nowhere else. "}
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
                  added <Time at={k.created} on />
                  {k.used ? (
                    <>
                      , last used <Time at={k.used} on />
                    </>
                  ) : (
                    ""
                  )}
                </span>
                <button
                  class="secondary danger small"
                  data-remove-passkey={k.id}
                  disabled={waiting || onlyPasskey || (keys?.ways ?? 0) <= 1}
                  {...browserButton("Remove")}
                  title={onlyPasskey ? ONLY_PASSKEY : (keys?.ways ?? 0) <= 1 ? "Your only way to sign in" : app ? "Opens in your browser" : undefined}
                  onClick={() => (app ? toBrowser("remove-passkey") : askToRemove(k.id, passkeyName(k, i)))}
                />
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
        Removes your account from this control, with your devices, machines, sessions and passkeys.{" "}
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
