// Deploys waiting for you, on control's phone view (#468): a push may not
// reach the phone (no subscription, no Home Screen app, a pause), so a
// deploy you can approve is a row under the bar, one tap from opening
// Arugula. It opens the deploy on the platform page, where it's approved;
// two or more open the inbox, which lists them first. Read from the
// platform's `GET /api/deploys/waiting` (on control's site, behind its
// gateway) when the page comes up, comes back into view, hears a tapped
// notice, and now and then while it's open.

import { useEffect, useState } from "preact/hooks";
import { deployTitle } from "../../shared/deploys";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { openPlatform, platformOrigin } from "../platform-links";
import { getControlSession } from "./people";

/** What the row needs of a waiting deploy (the platform's `deploy_json`). */
interface Waiting {
  id: string;
  project: { name: string };
  environment: { name: string; driver: string };
  source: { ref: string };
  subject?: string | null;
  expected_version?: string | null;
  may_approve: boolean;
}

/** How often to look again while the page is open and in view. */
const EVERY_MS = 120_000;

async function read(): Promise<Waiting[] | null> {
  try {
    const r = await platformFetch("/api/deploys/waiting", { credentials: "same-origin" });
    if (!r.ok) return null;
    return ((await r.json()) as { deploys: Waiting[] }).deploys.filter((d) => d.may_approve);
  } catch {
    return null;
  }
}

/** The row's words: the one deploy's title, or how many wait. */
export function waitingSaid(list: { project: { name: string }; environment: { name: string; driver: string }; source: { ref: string }; subject?: string | null; expected_version?: string | null }[]): string {
  if (list.length === 1) return `${list[0].project.name}: ${deployTitle(list[0])} waits for your approval`;
  return `${list.length} deploys wait for your approval`;
}

export function DeploysWaitingNote() {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  const ready = !!session && session.phase === "ready" && !!platformOrigin();
  const [list, setList] = useState<Waiting[]>([]);
  useEffect(() => {
    if (!ready) return;
    let live = true;
    const load = () =>
      void read().then((l) => {
        if (live && l) setList(l);
      });
    load();
    const back = () => document.visibilityState === "visible" && load();
    const timer = setInterval(() => document.visibilityState === "visible" && load(), EVERY_MS);
    document.addEventListener("visibilitychange", back);
    addEventListener("arugula:control-refresh", load);
    return () => {
      live = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", back);
      removeEventListener("arugula:control-refresh", load);
    };
  }, [ready]);
  if (!ready || list.length === 0) return null;
  const to = list.length === 1 ? `/deploys/${list[0].id}` : "/inbox";
  return (
    <button type="button" class="ws-stale deploys-waiting-note" data-deploys-waiting={list.length} onClick={() => openPlatform(to)}>
      <span class="deploys-waiting-dot" aria-hidden="true" />
      <span class="deploys-waiting-said">{waitingSaid(list)}</span>
      <span class="deploys-waiting-go">{list.length === 1 ? "Open" : "Inbox"} ›</span>
    </button>
  );
}
