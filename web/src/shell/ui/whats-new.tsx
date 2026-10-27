// What's new (Discovery 2): a small panel with the headlines of the
// releases since the last one shown to this person (shared/whats-new),
// each with *Try it* when it has one, and a release's *Full notes*. Not a
// modal: it sits under the bar, and the work goes on beside it.
//
// It opens by itself once, when this build is newer than the pref
// `whats-new` (shared/seen: per account on control's page, else per
// browser), so after an update: the daemon's page reloads onto the new
// build, control's page is deployed, and the desktop app's window shows
// one of the two. It's seen when shown. The update chip's *What's new*
// and the account menu's open it again. Items this person can't use are
// left out (their `needs`).

import { useEffect, useState } from "preact/hooks";
import type { Client, FlagName } from "../client";
import { pref, seenReady, setPref, subscribeSeen } from "../../shared/seen";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { advances, notesUrl, PREF, unshown, upTo, usable, type Need, type WhatsNewItem, type WhatsNewRelease } from "../../shared/whats-new";
import { platformOrigin, openPlatform } from "../platform-links";
import { COMMANDS } from "./commands";
import { getControlSession } from "./people";

/** This build's version (vite.config.ts, from Cargo.toml), and the
 * release notes' headlines (scripts/whats-new.mjs), newest first. */
const BUILT = (import.meta.env.VITE_ARUGULA_VERSION as string | undefined) ?? null;
const RELEASES = (import.meta.env.VITE_WHATS_NEW as WhatsNewRelease[] | undefined) ?? [];

/** A newer release than this build, from the update chip: its notes
 * aren't built in, so the panel links them. */
export interface Newer {
  latest: string;
  url: string;
}

let open: { client: Client; releases: WhatsNewRelease[]; newer?: Newer } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
const subscribe = (fn: () => void) => (listeners.add(fn), () => void listeners.delete(fn));

/** Whether the platform's `GET /api/projects` says this reader is on the
 * hosted workspaces' list: asked once a page, and only when an item
 * needs it, on control's page. */
let hosted: Promise<boolean> | null = null;
function onHostedList(): Promise<boolean> {
  hosted ??= platformFetch("/api/projects", { credentials: "same-origin" })
    .then((r) => (r.ok ? (r.json() as Promise<{ hosted?: boolean }>) : null))
    .then((v) => !!v?.hosted)
    .catch(() => false);
  return hosted;
}

/** Leave out what this person can't use, with what's known on this page. */
async function forThem(client: Client, releases: WhatsNewRelease[]): Promise<WhatsNewRelease[]> {
  const control = getControlSession();
  const signedIn = control?.phase === "ready" && !!control.account;
  const needsHosted = signedIn && releases.some((r) => r.items.some((it) => it.needs?.includes("hosted")));
  const isHosted = needsHosted && (await onHostedList());
  const can = (n: Need) =>
    n === "control" ? signedIn : n === "team" ? signedIn && control!.teams.length > 0 : n === "hosted" ? isHosted : client.flag(n.slice(5) as FlagName);
  return usable(releases, can);
}

/** Open it with this build's newest releases (the update chip's
 * *What's new*, the account menu's). */
export async function openWhatsNew(client: Client, newer?: Newer) {
  open = { client, releases: await forThem(client, upTo(RELEASES, BUILT)), newer };
  changed();
}

export function closeWhatsNew() {
  open = null;
  changed();
}

/** Opens it once when this build is newer than the last version shown;
 * draws it when open. Once, in the workspace. */
export function WhatsNewLayer({ client }: { client: Client }) {
  useSubscribe(subscribe);
  const [ready, setReady] = useState(seenReady);
  useEffect(() => subscribeSeen(() => setReady(seenReady())), []);
  const has = !!client.state;
  useEffect(() => {
    // Wait for the store (it says which account) and the machine (its flags).
    // A browser driven by automation (the e2e specs, which each start as a
    // first run) doesn't get it by itself.
    if (!ready || !has || navigator.webdriver) return;
    const last = pref(PREF);
    if (!advances(BUILT, last)) return;
    // Seen when shown, as hints are: moved on first, so a second tab or
    // device doesn't open it too.
    setPref(PREF, BUILT);
    void forThem(client, unshown(RELEASES, BUILT, last)).then((releases) => {
      if (!releases.length) return;
      open = { client, releases };
      changed();
    });
  }, [ready, has, client]);
  return open ? <Panel {...open} /> : null;
}

function Panel({ client, releases, newer }: { client: Client; releases: WhatsNewRelease[]; newer?: Newer }) {
  return (
    <section class="whats-new" aria-label="What's new" data-whats-new>
      <header class="whats-new-head">
        <h2>What's new</h2>
        <button class="whats-new-close" aria-label="Close What's new" data-whats-new-close onClick={closeWhatsNew}>
          ×
        </button>
      </header>
      {newer && (
        <p class="whats-new-newer">
          Arugula {newer.latest} is out.{" "}
          <a href={newer.url} target="_blank" rel="noreferrer">
            Its notes ↗
          </a>
        </p>
      )}
      {releases.map((r) => (
        <div class="whats-new-release" key={r.version} data-whats-new-release={r.version}>
          <div class="whats-new-version">
            <b>Arugula {r.version}</b>
            <a href={notesUrl(r.version)} target="_blank" rel="noreferrer" data-whats-new-notes>
              Full notes ↗
            </a>
          </div>
          <ul>
            {r.items.map((it) => (
              <Item key={it.id} client={client} item={it} />
            ))}
          </ul>
        </div>
      ))}
      {!releases.length && <p class="whats-new-none">Nothing new here for you in this release.</p>}
    </section>
  );
}

/** What *Try it* does, or null when this page can't (an unknown command,
 * a platform path with no platform page). */
function tryIt(client: Client, t: WhatsNewItem["try"]): (() => void) | null {
  if (!t) return null;
  if ("command" in t) {
    const run = COMMANDS[t.command];
    return run ? () => run(client, {}) : null;
  }
  if (t.route.startsWith("#")) return () => void (location.hash = t.route);
  return platformOrigin() ? () => openPlatform(t.route) : null;
}

function Item({ client, item }: { client: Client; item: WhatsNewItem }) {
  const run = tryIt(client, item.try);
  return (
    <li data-whats-new-item={item.id}>
      <span class="whats-new-copy">
        <b>{item.title}</b> {item.line}
      </span>
      {run && (
        <button
          class="whats-new-try"
          data-whats-new-try
          onClick={() => {
            closeWhatsNew();
            run();
          }}
        >
          Try it
        </button>
      )}
    </li>
  );
}
