// Plan and usage (M22), the platform's: the account's plan, what it used
// of the relay and hosted VMs this month, each team's plan, and the way to
// upgrade. Drawn in control's overlay from what it's handed, so it never
// sees the control session (M149).

import { useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";

const mb = (b: number) => `${(b / 1e6).toFixed(b < 1e7 ? 1 : 0)} MB`;

/** The account's plan and usage, as control's `/api/billing` gives it
 * (the shell's `ControlSession["billing"]` is this). */
export interface Billing {
  billing: boolean;
  plan: string;
  relay: { bytes: number; allowance: number; warning: boolean; slowed: boolean };
  sandbox_minutes: number;
  teams: { team: string; name: string; owner: boolean; seats: number; plan: string; sandbox_minutes: number }[];
}

/** How a plan is put in a sentence: the account's, or a team's (`team`). */
export function planWords(b: Billing, team?: string): string {
  if (!team) return b.plan === "paid" ? "a paid plan" : "the free plan";
  const t = b.teams.find((x) => x.team === team);
  return t?.plan === "team" ? `team plan, ${t.seats} seats` : "free";
}

/** `upgrade` goes off to checkout, for a team or (with none) the account.
 * `finishing` is the desktop app's step while checkout is open in the
 * browser (#236). */
export function Plan({ billing: b, upgrade, close, finishing }: { billing: Billing | null; upgrade: (team?: string) => Promise<void>; close: () => void; finishing?: preact.ComponentChildren }) {
  const [err, setErr] = useState("");
  if (!b) return null;
  return (
    <Modal close={close}>
      <h2>Plan and usage</h2>
      <p>
        You're on <b>{planWords(b)}</b>. This month: {mb(b.relay.bytes)} through the relay
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
            <button class="link" onClick={() => upgrade(t.team).catch((e: Error) => setErr(e.message))}>
              Upgrade ({t.seats} seat{t.seats === 1 ? "" : "s"})
            </button>
          ) : null}
        </p>
      ))}
      {b.plan !== "paid" ? (
        <p>
          <button class="link" onClick={() => upgrade().catch((e: Error) => setErr(e.message))}>
            Add a payment method for hosted VMs (by the minute)
          </button>
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
      {finishing}
      <div class="prompt-buttons">
        <button class="secondary" onClick={close}>Done</button>
      </div>
    </Modal>
  );
}
