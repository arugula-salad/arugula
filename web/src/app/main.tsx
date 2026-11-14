// The app's entry (Arugula #989): one app on app.arugula.io, served through
// the hub, rooted at the project. It puts the platform's pages (projects,
// boards, Docs, chat) and, as the inversion lands, the shell's machine page
// and Swarm in one frame. app/ is the only folder that may import both
// platform/ and shell/ (web/scripts/check-imports.mjs); for now it draws the
// platform's pages alone.

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
render(
  <PlatformApiContext.Provider value={platformApi}>
    <PlatformPage />
    {/* Menus and the questions they ask (a new channel's name…). */}
    <MenuLayer />
    <PromptLayer />
    {/* Appearance…, from the account menu and Settings: the shell's dialog. */}
    <AppearanceLayer />
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
