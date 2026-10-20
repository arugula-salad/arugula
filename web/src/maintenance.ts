// Control's maintenance windows (M141, #1158): for a few announced minutes
// the platform's routes (chat, Cloud, grants and billing) answer 503 with a
// `Retry-After` in seconds and `{"error": …, "reason": "maintenance"}`,
// while terminals and the relay keep working. The page waits them out:
// `platformFetch` tries again in the background, and `maintenance` says a
// window is on, so the chat page and Cloud workspaces show a calm note
// (ui/maintenance-note.tsx) rather than an error.
//
// The hub's gateway answers that 503 before it forwards anything, so a
// request refused with `maintenance` wasn't processed: it's safe to send
// again whatever its method (a message posted included). Any other 503
// may have come after the work was done, so only a GET or HEAD is sent
// again on one. The waits are control-wire's `maintenance::Waits`, which
// the daemon and the CLI use: what `Retry-After` asks, between 1s and 30s,
// else 2s, 4s, 8s… up to 30s, for five minutes in all.

/** The `reason` control's 503 gives during a maintenance window. */
export const MAINTENANCE = "maintenance";
/** What the page says while one is on. */
export const MAINTENANCE_SAID = "Chat and Cloud are briefly down for maintenance, back in a minute.";

const FLOOR = 1_000;
const PER_WAIT = 30_000;
const TOTAL = 300_000;

/** How long to wait before the next try, `asked` being `Retry-After` in
 * ms (null: it asked nothing), after `tries` tries that waited `waited` ms
 * in all: null once five minutes of waits are spent. */
export function nextWait(asked: number | null, tries: number, waited: number): number | null {
  const left = TOTAL - waited;
  if (left <= 0) return null;
  const wait = asked ?? 2 ** (tries + 1) * 1_000;
  return Math.min(Math.max(wait, FLOOR), PER_WAIT, left);
}

/** `Retry-After` in ms. An HTTP date (control never sends one) is null. */
export function retryAfter(header: string | null): number | null {
  const s = header?.trim() ?? "";
  return /^\d+$/.test(s) ? Number(s) * 1_000 : null;
}

let down = false;
const listeners = new Set<() => void>();

/** Whether control's platform is in a maintenance window, as the page's
 * last call to it heard. */
export const maintenance = {
  get down() {
    return down;
  },
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
};

function heard(now: boolean) {
  if (now === down) return;
  down = now;
  for (const fn of [...listeners]) fn();
}

async function isMaintenance(res: Response): Promise<boolean> {
  if (res.status !== 503) return false;
  const j = (await res.clone().json().catch(() => null)) as { reason?: string } | null;
  return j?.reason === MAINTENANCE;
}

/** `fetch` for control's platform routes (`/api/chat…`, `/api/cloud/…`,
 * `/api/billing…`): a maintenance window is waited out for up to five
 * minutes, then its 503 is the answer (`maintenanceOf` says so). A network
 * failure throws, as `fetch` does. `init.body` must be sendable twice (a
 * string, as every caller's is). */
export async function platformFetch(input: string, init: RequestInit = {}): Promise<Response> {
  const method = (init.method ?? "GET").toUpperCase();
  let tries = 0;
  let waited = 0;
  for (;;) {
    const res = await fetch(input, init);
    const window = await isMaintenance(res);
    heard(window);
    if (res.status !== 503 || !(window || method === "GET" || method === "HEAD")) return res;
    const wait = nextWait(retryAfter(res.headers.get("Retry-After")), tries, waited);
    if (wait === null) return res;
    tries += 1;
    waited += wait;
    await new Promise((r) => setTimeout(r, wait));
  }
}

/** What to say of a refused answer's body: the calm words for a
 * maintenance window, else control's own `error`, else `fallback`. */
export function refusalOf(status: number, body: { error?: string; reason?: string }, fallback = `Request failed (${status})`): string {
  if (status === 503 && body.reason === MAINTENANCE) return MAINTENANCE_SAID;
  return body.error ?? fallback;
}
