// A join, by its code: a machine's, added in one click by a browser with no
// device key (#1003) or approved with this browser's key; the Arugula
// CLI's (M49) or the phone app's (aru-131), which only a device approves
// (`arugula login --browser` signs the CLI in with no key). A code with a
// secret (#760) is typed rather than compared; one from before codes
// shows a fingerprint to check. No browser asks to be approved any more
// (aru-141, #1004).

import { useEffect, useState } from "preact/hooks";
import { CopyText } from "../../shared/ui/copy";
import { Modal } from "../../shared/ui/modal";
import { RefusedError, takesSecret, type ControlSession, type JoinRequest } from "../control";
import { fingerprint, normalizeCode, normalizeSecret, splitJoinCode } from "../e2e/cert.ts";
import { addsInOneClick, cantAdd } from "../../shared/team-adds.ts";
import { WhosePicker } from "./whose";
import { clearHash } from "./common";

/** A machine's join, by its code: from its approval link, or typed in.
 *
 * #1003: a browser that isn't one of the account's devices adds a machine
 * in one click (aru-141): nothing to compare, since control decides who
 * reaches the machine. A machine whose link carries a secret takes the Add
 * only with a MAC under it, so it's added from the whole link (or the
 * whole code typed in), not the code alone.
 *
 * #760: from a device, a machine whose link carries a secret
 * (`CODE.SECRET`) is approved with a MAC under it, and there's nothing to
 * compare: the code was recomputed from its key, and the machine checks the
 * account itself. One that made a secret but reached this page without it
 * (the code alone) asks for the rest of the code. An older machine makes
 * none, and its code and the account's fingerprint are compared, as
 * before. */
export function JoinPrompt({ s, code: whole }: { s: ControlSession; code: string }) {
  const parts = splitJoinCode(whole);
  const code = parts?.code ?? whole;
  // #760: the rest of the code, typed when the page has only the first half.
  const [typed, setTyped] = useState<Rest>(NO_REST);
  const [j, setJ] = useState<JoinRequest | null>(null);
  const [err, setErr] = useState("");
  // Control doesn't know the code (UI audit #179): mistyped, or past its
  // 15 minutes.
  const [unknown, setUnknown] = useState(false);
  const [busy, setBusy] = useState(false);
  // Control refused an approval from here (#327): closing then isn't
  // turning the machine down. It keeps waiting for one that works.
  const [refused, setRefused] = useState(false);
  // "" is just me; else a team's id.
  const [to, setTo] = useState("");
  // Into a team: the team's own machine, not mine also used for it
  // (Arugula #1182). Mine unless picked.
  const [teamBox, setTeamBox] = useState(false);
  useEffect(() => {
    // Another code: nothing typed for the last one carries over.
    setTyped({ rest: "", bad: null });
    setUnknown(false);
    // This browser's own trust, fresh, before it offers Approve (#327).
    void s.refresh();
    s.showJoin(code).then(
      (j) => {
        setJ(j);
        setTo(j.team?.team ?? "");
      },
      (e: Error) => {
        setErr(e.message);
        setUnknown(true);
      },
    );
  }, [code]);
  // A code control doesn't know says so, in words, with nothing to approve
  // (UI audit #179: it showed "no such code (expired?)" over Approve).
  if (unknown && !j)
    return (
      <Modal close={clearHash}>
        <h2>Add a machine?</h2>
        <p data-join-unknown={code}>
          No machine is waiting with code <b>{code}</b>. Codes last 15 minutes: run <code>arugulad join</code> on the machine again for a new one.
        </p>
        {/no such code/i.test(err) ? null : <p class="dim">{err}</p>}
        <div class="prompt-buttons">
          <button class="primary" onClick={clearHash}>
            Close
          </button>
        </div>
      </Modal>
    );
  // #1003: no device key here: Add, as this signed-in person.
  const oneClick = !!j && j.cert.kind === "daemon" && !s.enrollment;
  // Teams I can add it to (#332, #1184), and the one it asked for even if
  // I can't, with why not (#1183).
  // In one click, control lets a team's owners and editors add machines,
  // and only its owners while it's locked.
  const teams = oneClick ? s.teams.filter(addsInOneClick) : s.addableTeams();
  const asked = j?.team && !teams.some((t) => t.team === j.team!.team) ? j.team : null;
  const team = teams.find((t) => t.team === to);
  const cant = !!to && !team;
  const why = asked ? cantAdd(asked.name, s.teams.find((t) => t.team === asked.team) ?? null) : null;
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
  // In one click too: the machine checks the Add's MAC under its secret.
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
  // A native app (aru-131), which pairs the CLI's way.
  if (j?.cert.kind === "browser") return <AppJoin s={s} j={j} cancel={cancel} refused={refused} failed={failed} sealed={sealed} secret={secret} whole={askWhole} />;
  // M49: the Arugula CLI on a machine, asking to be one of your devices.
  if (j?.cert.kind === "cli")
    return <CliJoin s={s} j={j} cancel={cancel} refused={refused} failed={failed} sealed={sealed} secret={secret} whole={askWhole} />;
  const machine = j ? (
    sealed || oneClick ? (
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
    <Modal close={clearHash}>
      <h2>Add a machine?</h2>
      {err ? (
        <p class="control-error" data-join-error>
          {err}
        </p>
      ) : null}
      {j ? (
        <>
          {machine}
          {askWhole}
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
                A machine is in one place: <b>Just me</b> (only you see its panes, and the sessions you share) or one team (everyone in it watches
                its sessions in the team's projects, or uses all of it if it's the team's own machine, and the team's owners count as its owners
                too). To work with more than one team, pick Just me, then share a session with each team (<i>Share</i>, beside the session's name).
              </p>
              <p>
                <label>
                  Join to{" "}
                  <select class="field" data-join-to value={to} onChange={(e) => setTo((e.target as HTMLSelectElement).value)}>
                    <option value="">Just me</option>
                    {teams.map((t) => (
                      <option key={t.team} value={t.team}>
                        {t.roster.name}
                      </option>
                    ))}
                    {asked ? (
                      <option value={asked.team}>
                        {asked.name} ({why!.note})
                      </option>
                    ) : null}
                  </select>
                </label>
              </p>
              {/* Arugula #1182: a machine into a team is its owner's or the
                  team's. */}
              {team && j.cert.kind === "daemon" ? (
                <p>
                  <WhosePicker hook="join" teamBox={teamBox} onPick={setTeamBox} />
                </p>
              ) : null}
            </>
          ) : null}
          {cant ? (
            <p class="control-error" data-join-not-member>
              {why?.error}
            </p>
          ) : (
            <p class="dim" data-join-grants>
              {team && teamBox
                ? `It's ${team.roster.name}'s: its sessions go in ${team.roster.name}'s project, and everyone in ${team.roster.name} uses it by their role: editors drive its terminals, viewers watch. Its owners reach all of it, private panes too, and can take it out of the team.`
                : team
                  ? `Everyone in ${team.roster.name} sees its sessions in ${team.roster.name}'s projects and watches them: they drive a terminal only when you share its session or trust them with it. Its owners reach all of it, private panes too, and can take it out of the team. Sessions in your own projects stay yours alone. It stays yours.`
                  : "Only you reach it, and you can drive its terminals. You're its only owner."}{" "}
              Arugula cloud decides who reaches it and relays the connection: it can read what it relays, and keeps none of it.
            </p>
          )}
        </>
      ) : err ? null : (
        <p class="dim">Looking up {code}…</p>
      )}
      <div class="prompt-buttons">
        <button class="secondary" data-cancel-join onClick={cancel}>
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
              if (oneClick) await s.addJoin(j.code, j.cert, team?.team ?? null, teamBox, sealed ? secret : null);
              else await s.approveJoin(j.code, j.cert, team?.team ?? null, secret, teamBox);
              clearHash();
            } catch (e) {
              failed(e);
            }
          }}
        >
          {oneClick ? "Add" : "Approve"}
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
 * its code (typed alone). The code is shown as a fixed prefix, and only the
 * rest is typed. Holding another code (`restOf` keeps it, its first half
 * and all), the field drops the fixed half and widens, so it shows that
 * code once and whole. */
function RestOfCode({
  code,
  rest,
  bad,
  set,
  cli = false,
  hint,
  prompt = "Enter the rest of its code to approve.",
}: {
  code: string;
  rest: string;
  bad: string | null;
  set: (r: Rest) => void;
  cli?: boolean;
  hint?: preact.ComponentChildren;
  prompt?: preact.ComponentChildren;
}) {
  const tag = (t: string) => ({ [`data-join-${t}`]: "" });
  // More than the 16 characters of a rest: a code of its own, typed whole.
  const claims = rest.replace(/[^A-Za-z0-9]/g, "").length > 16;
  return (
    <div class="control-rest" {...tag("rest")}>
      <p>{prompt}</p>
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
type DeviceJoin = {
  s: ControlSession;
  j: JoinRequest;
  cancel: () => void;
  refused: boolean;
  failed: (e: unknown) => void;
  sealed: boolean;
  secret: string | null;
  whole: preact.ComponentChildren;
};

/** Arugula's native app on a phone, asking to be one of your devices. */
function AppJoin({ s, j, cancel, refused, failed, sealed, secret, whole }: DeviceJoin) {
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Modal close={clearHash}>
      <h2>Add this app?</h2>
      {err ? (
        <p class="control-error" data-join-error>
          {err}
        </p>
      ) : null}
      {sealed ? (
        <p data-join-app={j.cert.name} data-join-sealed>
          <b>{j.cert.name}</b> asks to be one of your devices.
        </p>
      ) : (
        <>
          <p data-join-app={j.cert.name}>
            <b>{j.cert.name}</b> asks to be one of your devices, with code <b data-join-code={j.code}>{j.code}</b>. Check it's the code the app shows.
          </p>
          <p class="dim">Its key: {fingerprint(j.cert.device)}</p>
        </>
      )}
      {whole}
      <p class="dim">It reaches your machines (directly or through Arugula cloud's relay), your chat, projects, issues and boards.</p>
      {s.enrollment ? null : (
        <p class="control-error" data-join-needs-device>
          The phone app still joins as one of your devices, and only a device approves it. This browser isn't one: signing in is all it took
          here. Approve the app from a browser or phone that still holds a device key, or turn it down.
        </p>
      )}
      <div class="prompt-buttons">
        <button class="secondary" data-cancel-join onClick={cancel}>
          {refused ? "Close" : "Cancel"}
        </button>
        <button
          class="primary"
          data-approve-join
          disabled={busy || (sealed && !secret) || !s.enrollment}
          onClick={async () => {
            setBusy(true);
            try {
              await s.approveJoin(j.code, j.cert, null, secret);
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

/** A CLI from before #1151 (plain `arugula login`) or a newer one with
 * `--device` asks to be one of the account's devices. A browser with no
 * device key can't approve that (aru-141, #1004): it says how to sign the
 * CLI in from a browser instead, `arugula login --browser` (which both an
 * older CLI and a newer one take), and turning this one down ends its
 * wait. */
function CliFromBrowser({ s, j, cancel, refused }: DeviceJoin) {
  const host = j.cert.name.replace(/^(Arugula|illogical) CLI on /, "");
  // The hosted control is the CLI's default; another is named.
  const cmd = `arugula login --browser${s.info.url.replace(/\/$/, "") === HOSTED_URL ? "" : ` ${s.info.url}`}`;
  return (
    <Modal close={clearHash}>
      <h2>Sign in the command line?</h2>
      <p data-join-cli={j.cert.name} data-join-cli-browser>
        The Arugula command line on <b>{host}</b> asks to be one of your devices. Signing in is all it takes now, so it doesn't need a device: stop{" "}
        <code>arugula login</code> there (Ctrl-C), and run this instead. It prints a link to open in this browser, where you press Allow.
      </p>
      <CopyText text={cmd} data-join-cli-command />
      <p class="dim">Turn it down to end its wait.</p>
      <div class="prompt-buttons">
        <button class="secondary" data-cancel-join onClick={cancel}>
          {refused ? "Close" : "Turn it down"}
        </button>
        <button class="primary" data-join-cli-done onClick={clearHash}>
          Done
        </button>
      </div>
    </Modal>
  );
}

/** The hosted control, the CLI's default (`ssh::CONTROL`). */
const HOSTED_URL = "https://control.arugula.io";

function CliJoin(p: DeviceJoin) {
  const { s, j, cancel, refused, failed, sealed, secret, whole } = p;
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  if (!s.enrollment) return <CliFromBrowser {...p} />;
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
      {!sealed ? (
        <p>
          Your account:{" "}
          <b class="fingerprint" data-join-account={s.enrollment.root}>
            {fingerprint(s.enrollment.root)}
          </b>
          . Once you approve, it shows its account's fingerprint: check it's this one there.
        </p>
      ) : null}
      <p class="dim">It reaches your machines (directly or through Arugula cloud's relay). To sign it in with no key instead, run <code>arugula login --browser</code> there.</p>
      <div class="prompt-buttons">
        <button class="secondary" data-cancel-join onClick={cancel}>
          {refused ? "Close" : "Cancel"}
        </button>
        <button
          class="primary"
          data-approve-join
          disabled={busy || (sealed && !secret)}
          onClick={async () => {
            setBusy(true);
            try {
              await s.approveJoin(j.code, j.cert, null, secret);
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
