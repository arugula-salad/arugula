// An app's plan (#318 P11's /billing): Starter, Team or Enterprise, what
// this month costs so far (the relay's own arithmetic on the usage
// numbers, never a Stripe invoice), Upgrade (Stripe Checkout) and Manage
// (Stripe's Customer Portal). Billing is in Stripe's test mode until we
// go live, and the card says so; a relay without Stripe says billing
// isn't set up.

import { useEffect, useState } from "preact/hooks";
import * as api from "./api";
import type { App, Billing } from "./api";
import { Section } from "./ui";

const fmt = (n: number) => n.toLocaleString();
const usd = (s: string) => `$${s}`;
const day = (iso: string) => new Date(iso).toLocaleDateString([], { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" });
const monthLabel = (m: string) => new Date(`${m}-15T12:00:00Z`).toLocaleDateString([], { month: "long", year: "numeric", timeZone: "UTC" });

const STATUS: Record<string, string> = {
  active: "Paid up",
  trialing: "On a trial",
  past_due: "A payment failed",
  unpaid: "Unpaid",
  paused: "Paused",
  canceled: "Cancelled",
  incomplete: "Waiting for the first payment",
  incomplete_expired: "The first payment didn't go through",
};

/** Where Upgrade and Manage go: Stripe's own pages, then back here. */
function GoButton({ label, primary, to, name }: { label: string; primary?: boolean; to: () => Promise<string>; name?: string }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  return (
    <>
      <button
        class={primary ? "btn primary" : "btn"}
        disabled={busy}
        aria-label={name}
        onClick={async () => {
          setBusy(true);
          setErr("");
          try {
            location.assign(await to());
          } catch (e) {
            setErr((e as Error).message);
            setBusy(false);
          }
        }}
      >
        {busy ? "Opening Stripe…" : label}
      </button>
      {err && (
        <span class="error" role="alert">
          {err}
        </span>
      )}
    </>
  );
}

function Estimate({ b }: { b: Billing }) {
  const e = b.estimate;
  if (b.plan !== "team")
    return (
      <div class="tile">
        <span class="label">{monthLabel(e.month)} so far</span>
        <b class="big">{usd(e.total)}</b>
        <span class="hint">Starter is free, up to {fmt(b.plans.starter.active_users_cap)} active users a month.</span>
      </div>
    );
  return (
    <div class="tile">
      <span class="label">{monthLabel(e.month)} so far</span>
      <b class="big">{usd(e.total)}</b>
      <table class="numbers bill" aria-label="This month's cost so far">
        <tbody>
          <tr>
            <td>Team, a month</td>
            <td>{usd(e.base)}</td>
          </tr>
          <tr>
            <td>
              {fmt(e.extra_active_users)} active users past the {fmt(e.included_active_users ?? 0)} included, at {usd(b.plans.team.per_extra_active_user)}
            </td>
            <td>{usd(e.extra_active_users_cost)}</td>
          </tr>
          <tr>
            <td>
              {fmt(e.call_minutes)} call minutes, at {usd(b.plans.team.per_call_minute)}
            </td>
            <td>{usd(e.call_minutes_cost)}</td>
          </tr>
        </tbody>
      </table>
      <span class="hint">An estimate from the usage above, before tax. Your first month's base is prorated from the day you upgraded.</span>
    </div>
  );
}

function PlanTile({ b }: { b: Billing }) {
  const s = b.subscription;
  const name = b.plan === "team" ? "Team" : "Starter";
  return (
    <div class="tile">
      <span class="label">Plan</span>
      <b class="big">{name}</b>
      {s?.status && <span class="hint">{STATUS[s.status] ?? s.status}</span>}
      {s?.grace_until && (
        <span class="state warn-line" role="status">
          ⚠ Team's limits stay until {day(s.grace_until)}; then this app falls back to Starter's cap of {fmt(b.plans.starter.active_users_cap)} active users. Update the card under Manage.
        </span>
      )}
      {s?.cancel_at_period_end && s.current_period_end && <span class="hint">Ends on {day(s.current_period_end)}.</span>}
      <span class="hint">{b.cap.active_users === null ? "No cap on active users." : `Capped at ${fmt(b.cap.active_users)} active users a month.`}</span>
    </div>
  );
}

export function BillingSection({ app }: { app: App }) {
  // undefined: loading; null: this relay has no billing yet.
  const [b, setB] = useState<Billing | null | undefined>(undefined);
  const [err, setErr] = useState("");
  const back = new URLSearchParams(location.search).get("billing");
  useEffect(() => {
    api.billing(app.id).then(setB, (e: Error) => setErr(e.message));
  }, [app.id]);
  return (
    <Section id="billing" title="Plan and billing" kind="--k-agent" sub="Each app has its own plan. Starter is free with a cap; Team is a monthly price that includes active users, then you pay for the people and call minutes you use.">
      {err ? (
        <p class="error">{err}</p>
      ) : b === undefined ? (
        <p class="empty" aria-busy="true">
          Loading…
        </p>
      ) : b === null || !b.enabled ? (
        <p class="empty calm">Billing isn't set up on this relay yet: every app is free, with only the caps you set.</p>
      ) : (
        <>
          {b.test_mode && (
            <p class="test-mode" role="note">
              <b>Test mode.</b> Billing runs on Stripe's test mode: no real card is charged. Use the test card 4242 4242 4242 4242, any future date and any CVC.
            </p>
          )}
          {back === "done" && (
            <p class="ok" role="status">
              Thanks: Stripe has your subscription. Team's limits apply as soon as Stripe tells us, usually within seconds.
            </p>
          )}
          <div class="tiles">
            <PlanTile b={b} />
            <Estimate b={b} />
          </div>
          <div class="row">
            {b.can_upgrade && <GoButton primary label={`Upgrade to Team, ${usd(b.plans.team.base)} a month`} to={() => api.checkout(app.id)} />}
            {b.can_manage && <GoButton label="Manage billing" name="Manage billing in Stripe" to={() => api.portal(app.id)} />}
            <a class="btn" href={b.plans.enterprise.contact}>
              Enterprise: talk to us
            </a>
          </div>
          <p class="hint">
            Team: {usd(b.plans.team.base)} a month including {fmt(b.plans.team.included_active_users)} active users, then {usd(b.plans.team.per_extra_active_user)} for each one more and {usd(b.plans.team.per_call_minute)} a call
            minute, billed at the end of each month.{b.managed_payments ? " Payments are processed by Stripe; receipts come from Link." : " Payments are processed by Stripe."}
          </p>
        </>
      )}
    </Section>
  );
}
