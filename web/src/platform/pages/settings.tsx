// `/settings`: the account's platform settings (M150): plan and usage
// (`/api/billing`, the platform's), the way to cloud agents and secrets,
// and which machines' agents read and post in conversations (M197). Sign-in, passkeys, devices and teams are trust, signed by a
// device, so they stay on control's page and this links there.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { Plan, type Billing } from "../settings/plan";
import { AgentGrants } from "../settings/agents";
import { follow } from "../router";
import { controlOrigin } from "../session";

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

export function SettingsPage() {
  const [billing, setBilling] = useState<Billing | null>(null);
  const [plan, setPlan] = useState(false);
  useEffect(() => {
    call<Billing>("/api/billing").then(setBilling, () => setBilling(null));
  }, []);
  const upgrade = async (team?: string) => {
    location.href = (await call<{ url: string }>("/api/billing/checkout", { team })).url;
  };
  return (
    <section class="platform-section" data-platform-settings>
      <h1>Settings</h1>
      <ul class="platform-list">
        {billing?.billing && (
          <li>
            <button class="control-linkish" data-open-plan onClick={() => setPlan(true)}>
              Plan and usage{billing.relay.warning ? " (over the free relay)" : ""}
            </button>
          </li>
        )}
        <li>
          <a href="/agents" onClick={(e) => follow(e, "/agents")}>
            Cloud agents and secrets
          </a>
        </li>
        <li>
          <a href={controlOrigin()} data-settings-control>
            Sign-in, passkeys, devices, teams and usage reports
          </a>{" "}
          <span class="dim">are on control's page, where your devices sign them.</span>
        </li>
      </ul>
      <AgentGrants />
      {plan && <Plan billing={billing} upgrade={upgrade} close={() => setPlan(false)} />}
    </section>
  );
}
