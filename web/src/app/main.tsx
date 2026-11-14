// The app's entry (Arugula #989): one app on app.arugula.io, served through
// the hub, rooted at the project. It puts the platform's pages (projects,
// boards, Docs, chat) and, as the inversion lands, the shell's machine page
// and Swarm in one frame. app/ is the only folder that may import both
// platform/ and shell/ (web/scripts/check-imports.mjs). The machines are
// its own places (./machines), reached through the hub with machine tokens
// (./session), and open on control's page while the hub can't from here.

import { render } from "preact";
import "../shared/fonts.css";
import "../shared/style.css";
import "../shared/frame.css";
import "../platform/platform.css";
// The lists in the swarm's language, over platform.css's.
import "../platform/swarm.css";
// /settings: its sections, the agents' matrices and the switches.
import "../shared/ui/settings-section.css";
import "../platform/settings.css";
import "../platform/finish.css";
// The one button set (UI audit #158), last: what it draws, it draws everywhere.
import "../shared/buttons.css";
import { apply as applyAppearance } from "../shared/appearance";
import { chat } from "../shared/chat-store";
import { followChatBadge, setupDesktop } from "../shared/desktop";
import { setProtoOrigin } from "../shared/ui/prototype-frame";
import "../shared/ui/prototype-storage";
import { PlatformApiContext } from "../shared/platform-api";
import { AppearanceLayer } from "../shared/ui/appearance";
import { MenuLayer, PromptLayer } from "../shared/ui/menu";
import { platformApi } from "../platform/api";
import { names } from "../platform/links";
import { go, pathOf } from "../platform/router";
import { controlOrigin, session } from "../platform/session";
import { fromControl } from "../shared/machine-paths";
import type { PlatformApi } from "../shared/platform-api";
import { appMachineDocs, appMachines, appOverview, appPanes } from "./machines";
import { appLive } from "./live";
import { AppSession } from "./session";
import { signedInOf } from "./signed-in";
import { setSignedIn } from "../shell/signed-in";
import { setPlatformHere } from "../shell/platform-links";
import { followPlatformLinks } from "./platform-here";
import { accountBackend, setSeenBackend } from "../shared/seen";
import { arriveHere } from "../platform/chat-nav";
import { PlatformPage } from "../platform/page";

// This device's palette and finish, before anything draws: on
// app.arugula.io, the one picked on control's page too (the cookie,
// shared/appearance-cookie.ts).
applyAppearance();
// In the desktop app: macOS's buttons over the bar's left end, Linux's
// own buttons in it (the bar's CSS reads this).
setupDesktop();
// Chat's unread on the app's Dock badge (#498).
followChatBadge(chat);
// A Doc's prototype can't run by srcdoc under this page's CSP: until the
// hub says where its proto page is (session.ts), it shows a note (#506).
setProtoOrigin(null);
// A room or thread opened cold gets the levels above it, so Back climbs (#817).
arriveHere();
// Who's signed in, for the shell's blocks in a machine's pane (a board,
// a native issue, team chat): the platform's session, as control's page
// gives its own.
setSignedIn(signedInOf(session));
// The platform is this page: the shell's links to it (a card, a board's
// row, an issue) go to its places here, in place, not to a tab of their own.
setPlatformHere((path) => go(path));
followPlatformLinks((path) => go(path), "[data-platform-machines]");
// The machines through the hub, made once someone is signed in: its
// links back to control's page need the hub's address, which the
// platform's first answer says.
let machines: AppSession | null = null;
const appSession = () => (machines ??= new AppSession(() => (session.me ? { account: session.me.account, login: session.me.login } : null), controlOrigin()));
/** Whether the machines open here: not once the hub has said it can't. */
const here = () => appSession().reachable !== false;
const api: PlatformApi = {
  ...platformApi,
  Machines: appMachines(appSession),
  panes: appPanes(appSession),
  Overview: appOverview(appSession),
  machineDocs: appMachineDocs(appSession),
  // A board card's session, drawn in its drawer with the app's own clients.
  ...appLive(appSession),
  goToPane: (machine, pane, quote) =>
    here() ? go(pathOf({ page: "find", what: "machines", at: { at: "machine", machine, pane: Number(pane) } }, names)) : platformApi.goToPane(machine, pane, quote),
  goToSession: (machine) => (here() ? go(pathOf({ page: "find", what: "machines", at: { at: "machine", machine } }, names)) : platformApi.goToSession(machine)),
  // Control's page's addresses, as the app's own places where it has them.
  controlHref: (hash) => (here() && fromControl(`#${hash}`)) || `${controlOrigin()}/#${hash}`,
};
// Your prefs (where you were, per project; hints seen) are your account's
// once you're signed in here: the hub's `/api/me/prefs`, through the app's
// host. Until the hub answers there, they're this browser's (shared/seen.ts).
let prefsFor: string | null = null;
session.subscribe(() => {
  const account = session.state === "ready" ? (session.me?.account ?? null) : null;
  if (!account || account === prefsFor) return;
  prefsFor = account;
  setSeenBackend(accountBackend(() => appSession().prefs(), (changes) => appSession().setPrefs(changes)));
});
render(
  <PlatformApiContext.Provider value={api}>
    <PlatformPage />
    {/* Menus and the questions they ask (a new channel's name…). */}
    <MenuLayer />
    <PromptLayer />
    {/* Appearance…, from the account menu and Settings: the shell's dialog. */}
    <AppearanceLayer />
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
