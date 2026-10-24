// `/settings`: the account's platform settings (M150): plan and usage
// (`/api/billing`, the platform's), and which machines' agents read and
// post in conversations (M197), agents with no machine (M230), the
// products linked to the account (M229), and which apps may use Claude through
// Arugula for the account, with what was spent on its own key (M231), and
// this device's Appearance…, the shell's dialog. Sign-in, passkeys, devices and teams are
// trust, signed by a device, so they stay on control's page and this links
// there. Each part is a section (../settings/section): a heading, one
// line, the rest behind More; on a wide screen an index of them sits
// beside the column (../settings.css).

import type { RefObject } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { openAppearance } from "../../shared/ui/appearance";
import { Plan, type Billing } from "../settings/plan";
import { AgentGrants } from "../settings/agents";
import { AgentTokens } from "../settings/agent-tokens";
import { LinkedProducts } from "../settings/linked";
import { ClaudeSettings } from "../settings/claude";
import { Section } from "../settings/section";
import { controlOrigin, session } from "../session";

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
  const body = useRef<HTMLDivElement>(null);
  return (
    <section class="platform-section sw-page settings-page" data-platform-settings>
      <div class="settings-layout">
        <Index body={body} />
        <div class="settings-body" ref={body}>
          <h1>Settings</h1>
          <Section id="account" title="Appearance & account" lede="How this device looks, your plan, and where the rest of your account is.">
            <ul class="platform-list settings-rows">
              <li>
                <button class="control-linkish" data-open-appearance onClick={openAppearance}>
                  Appearance…
                </button>
                <span class="dim">This device's palette, finish and corners.</span>
              </li>
              {billing?.billing && (
                <li>
                  <button class="control-linkish" data-open-plan onClick={() => setPlan(true)}>
                    Plan and usage{billing.relay.warning ? " (over the free relay)" : ""}
                  </button>
                  <span class="dim">Your plan, and what you used this month.</span>
                </li>
              )}
              <li>
                <a href={controlOrigin()} data-settings-control>
                  Sign-in, passkeys, devices, teams and usage reports
                </a>
                <span class="dim">On control's page, where your devices sign them.</span>
              </li>
            </ul>
          </Section>
          <LinkedProducts />
          <AgentGrants />
          <AgentTokens />
          <ClaudeSettings me={session.me?.account} />
        </div>
      </div>
      {plan && <Plan billing={billing} upgrade={upgrade} close={() => setPlan(false)} />}
    </section>
  );
}

/** The section index, beside the column on a wide screen: the sections
 * the page shows, read off it as each one loads (Claude's shows only when
 * there's something in it). */
function Index({ body }: { body: RefObject<HTMLDivElement | null> }) {
  const [secs, setSecs] = useState<{ id: string; title: string }[]>([]);
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const read = () => {
      const next = [...el.querySelectorAll<HTMLElement>(".settings-sec")].map((s) => ({ id: s.id, title: s.querySelector("h2")?.textContent ?? s.id }));
      setSecs((prev) => (prev.map((s) => s.id).join() === next.map((s) => s.id).join() ? prev : next));
    };
    read();
    const watch = new MutationObserver(read);
    watch.observe(el, { childList: true, subtree: true });
    return () => watch.disconnect();
  }, []);
  // To the section's heading, which takes the focus for a keyboard.
  const go = (e: MouseEvent, id: string) => {
    const h = document.getElementById(`${id}-h`);
    if (!h) return;
    e.preventDefault();
    h.scrollIntoView({ block: "start" });
    h.tabIndex = -1;
    h.focus({ preventScroll: true });
  };
  return (
    <nav class="settings-index" aria-label="Settings sections">
      <ul>
        {secs.map((s) => (
          <li key={s.id}>
            <a href={`#${s.id}`} onClick={(e) => go(e, s.id)}>
              {s.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
