// Approving with this device's key: a machine's join (or the Arugula CLI's,
// M49) by its code, with the devices that came alongside it (#326), and
// another device asking to be the account's (#761). A code with a secret
// (#760, #761) is typed or scanned rather than compared; one from before
// codes shows a fingerprint to check.

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import { AlongsideError, RefusedError, splitDeviceCode, takesSecret, type ControlSession, type DeviceCode, type JoinRequest } from "../control";
import { fingerprint, normalizeCode, normalizeSecret, splitJoinCode, type Cert } from "../e2e/cert.ts";
import { canScan, Scanner } from "../ui/scan";
import { clearHash } from "./common";

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
export function JoinPrompt({ s, code: whole, from, device }: { s: ControlSession; code: string; from?: Cert; device?: string }) {
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

/** Another device asks to be one of this account's. #761: one that shows
 * a code is approved with it (scanned with the camera, typed, or `given`
 * by the link its QR code holds): its lookup must be the request's keys'
 * code, and the approval answers its secret, so there's no fingerprint to
 * compare. One from before codes shows its fingerprint, as before. */
export function DevicePrompt({ s, c, given, done }: { s: ControlSession; c: Cert; given?: DeviceCode; done?: () => void }) {
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
export function DeviceLinkPrompt({ s, whole }: { s: ControlSession; whole: string }) {
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
