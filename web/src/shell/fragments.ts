// Every fragment the shell's address carries (M154, "Secrets stay in
// fragments"), in one place: what it is, why it stays a fragment, and
// where it's read. A fragment never reaches a server, a log or a Referer
// header, which is why secrets ride in them; the shell's own places stay
// fragments because the shell is one page the daemon serves (aru-026).
//
// Only the platform's routes left: `#chat…` is the platform page's paths
// now ("Platform URLs are ids with cosmetic slugs"), and the shell sends an
// old link to its new place. **Keep those redirects forever**: they're in
// old notifications, bookmarks, and messages from old daemons. (`#cloud=…`
// and `#cloud-ws…` went with Managoat's Cloud workspaces, which are gone.)
//
// `scripts/fragments.test.mjs` checks that every file listening for
// `hashchange` is named here, and the redirects.

export interface Fragment {
  /** The fragment, as it starts. */
  route: string;
  /** Why it stays a fragment. */
  why: string;
  /** Where it's read, under web/src. */
  where: string[];
}

/** The fragments that stay, the secrets first. */
export const FRAGMENTS: Fragment[] = [
  // ---- secrets: never a path
  { route: "#link=", why: "A share link's machine, key and token (S16): whoever has it gets in, so no server may see it.", where: ["shell/main.tsx"] },
  { route: "#join=", why: "A machine's join code with its secret (#760): the secret proves the approval, and control never sees it.", where: ["shell/control.ts", "shell/trust/overlay.tsx"] },
  { route: "#device=", why: "A waiting device's code with its secret (#761), the same way.", where: ["shell/control.ts", "shell/trust/overlay.tsx"] },
  { route: "#pinvite=", why: "A team invite's one-time seed (aru-060): its key admits whoever holds it.", where: ["shell/control.ts", "shell/trust/overlay.tsx"] },
  { route: "#askinvite=", why: "An invite that asks an owner first (#762), with its seed.", where: ["shell/control.ts", "shell/trust/overlay.tsx"] },
  { route: "#invite=", why: "An invite from before #762, with its code.", where: ["shell/control.ts", "shell/trust/overlay.tsx"] },
  { route: "#k=", why: "A Shared Doc's key (#1066), on the docs viewer: control holds the Doc as ciphertext only.", where: ["shell/doc-link.ts", "shell/doc-view.tsx"] },
  { route: "#app-redeem=", why: "The desktop app's sign-in ticket, grant and verifier (M48), handed from the app to the browser.", where: ["shell/control.ts"] },
  { route: "#app=", why: "A desktop app asking to sign in as this account (M48): the request's id, for this page to allow.", where: ["shell/trust/overlay.tsx", "shell/control.ts"] },
  { route: "#platform-signin=", why: "The platform page's sign-in resumed after signing in here (M152): its return address, PKCE challenge and state.", where: ["shell/control.ts"] },
  { route: "#issuer-signin=", why: "A product's sign-in through the hub's issuer resumed after signing in here (M228): the /oauth/authorize or /oauth/hud request it came in on.", where: ["shell/control.ts"] },
  // ---- the shell's own places: one page, served by the daemon
  { route: "#swarm", why: "The swarm (M41): the shell's.", where: ["shell/swarm/route.ts", "shell/main.tsx"] },
  { route: "#overview", why: "A machine's sessions at a glance (M111): the shell's.", where: ["shell/ui/places.tsx", "shell/ui/shell/overview.tsx"] },
  { route: "#agents", why: "The machine's agents (#403): the shell's.", where: ["shell/ui/agents-route.ts", "shell/ui/agents-page.tsx", "shell/ui/shell/rail.tsx"] },
  { route: "#pane=", why: "A pane to show, from a notification or the swarm.", where: ["shell/main.tsx"] },
  { route: "#machine=", why: "A machine to show (M184: a hosted workspace's, from the platform page), and with `&agent` Start an agent on it.", where: ["shell/main.tsx"] },
  { route: "#getting-started=", why: "The getting-started guide at a step.", where: ["shell/main.tsx"] },
  { route: "#developer", why: "The developer settings dialog.", where: ["shell/ui/developer.tsx"] },
  { route: "#lend=", why: "Lending this device to an agent on or off (S33).", where: ["shell/hand.ts"] },
  { route: "#app-done", why: "The desktop app's sign-in finished (M48).", where: ["shell/trust/overlay.tsx"] },
  { route: "#add-passkey", why: "Back from signing in with GitHub again to add a passkey to an account with none: the page offers it.", where: ["shell/trust/overlay.tsx", "shell/control.ts"] },
];

/** The listeners that aren't a route of their own: they redraw on any
 * route, or note it. */
export const HASH_LISTENERS: { where: string; why: string }[] = [
  { where: "shared/telemetry.ts", why: "Notes which view is open (M103), never the fragment itself." },
  { where: "shell/main.tsx", why: "Reads `#pane=`, `#machine=` and `#link=`, and redirects the platform's old routes." },
  { where: "shell/swarm/route.ts", why: "`#swarm`." },
  { where: "shell/trust/gate.tsx", why: "The sign-in screens follow `#join=`, `#device=` and invites." },
  { where: "shell/trust/overlay.tsx", why: "The overlay's prompts follow the secret fragments above." },
  { where: "shell/ui/agents-page.tsx", why: "`#agents`." },
  { where: "shell/ui/app.tsx", why: "Redraws on the shell's places." },
  { where: "shell/ui/places.tsx", why: "The places bar follows `#swarm`, `#overview`, `#agents`." },
  { where: "shell/ui/shell/overview.tsx", why: "`#overview`." },
  { where: "shell/ui/shell/rail.tsx", why: "The rail follows the shell's places." },
];

const ID = "[a-z]{2,3}_[0-9a-f]{4,64}";

/** Where an old link of the platform's lands on the platform page, as a
 * path there, or null when `hash` isn't one: `#chat` (and anything else of
 * chat's, like a machine's thread from before M125), `#chat=<room>` with
 * `&msg=N`, `#chat=activity`, `#chat=search/<words>`. */
export function platformPath(hash: string): string | null {
  let m: RegExpExecArray | null;
  if ((m = new RegExp(`^#chat=(${ID})(?:&msg=(-?\\d+))?$`).exec(hash))) return `/chat/${m[1]}${m[2] !== undefined ? `?msg=${m[2]}` : ""}`;
  if (hash === "#chat=activity") return "/chat/activity";
  if ((m = /^#chat=search\/(.*)$/.exec(hash))) {
    let q = m[1];
    try {
      q = decodeURIComponent(q);
    } catch {
      // as it came
    }
    return `/chat/search${q ? `?q=${encodeURIComponent(q)}` : ""}`;
  }
  if (/^#chat(?:=|$)/.test(hash)) return "/chat";
  return null;
}
