// Plan and usage (M22), the platform's: the account's plan, what it used
// of the relay and hosted VMs this month, each team's plan, and the way to
// upgrade. Drawn in control's overlay from what it's handed, so it never
// sees the control session (M149). It's a section of each owner's settings
// (Arugula #897): Personal's is the account's plan, a team's is the team's.

import { useEffect, useState } from "preact/hooks";
import { desktopApp } from "../../shared/desktop";
import { platformFetch } from "../../shared/maintenance";
import { BrowserStep } from "../../shared/ui/browser-step";
import { Modal } from "../../shared/ui/modal";
import { Section } from "../../shared/ui/settings-section";

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

async function call<T>(path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method: body === undefined ? "GET" : "POST",
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

/** An owner's Plan and billing section: Personal's (no `team`) or a
 * team's, with Plan and usage… to see it all and upgrade. Shown once
 * billing is on. */
export function PlanAndBilling({ team }: { team?: string }) {
  const [billing, setBilling] = useState<Billing | null>(null);
  const [plan, setPlan] = useState(false);
  useEffect(() => {
    call<Billing>("/api/billing").then(setBilling, () => setBilling(null));
  }, []);
  // The desktop app opens checkout in the browser and waits here for the
  // plan to change (#236); `words` is the plan it started from.
  const [finishing, setFinishing] = useState<{ team?: string; words: string } | null>(null);
  const [now, setNow] = useState("");
  if (!billing?.billing) return null;
  const upgrade = async (t?: string) => {
    if (desktopApp()) {
      setNow("");
      setFinishing({ team: t, words: planWords(billing, t) });
      return;
    }
    location.href = (await call<{ url: string }>("/api/billing/checkout", { team: t })).url;
  };
  const mine = team ? billing.teams.find((t) => t.team === team) : undefined;
  if (team && !mine) return null;
  return (
    <Section id="plan" title="Plan and billing" lede={mine ? `${mine.name}: ${planWords(billing, team)}.` : `You're on ${planWords(billing)}.`} hooks={{ "data-owner-plan": team ?? "personal" }}>
      <ul class="platform-list settings-rows">
        <li>
          <button class="settings-open" data-open-plan onClick={() => setPlan(true)}>
            <span class="settings-open-name">Plan and usage{!team && billing.relay.warning ? " (over the free relay)" : ""}</span>
            <span class="dim">{mine ? `${mine.sandbox_minutes} VM minutes this month.` : "Your plan, and what you used this month."}</span>
          </button>
        </li>
      </ul>
      {plan && (
        <Plan
          billing={billing}
          upgrade={upgrade}
          close={() => setPlan(false)}
          finishing={
            finishing ? (
              <BrowserStep
                url={() => call<{ url: string }>("/api/billing/checkout", { team: finishing.team }).then((v) => v.url)}
                label="Continue to checkout"
                cancel={() => setFinishing(null)}
                check={async () => {
                  const b = await call<Billing>("/api/billing");
                  if (planWords(b, finishing.team) === finishing.words) return false;
                  setBilling(b);
                  setNow(planWords(b, finishing.team));
                  return true;
                }}
                onDone={() => setFinishing(null)}
              />
            ) : now ? (
              <p data-plan-now>You're on {now}.</p>
            ) : null
          }
        />
      )}
    </Section>
  );
}
