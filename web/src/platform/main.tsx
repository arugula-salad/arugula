// The platform page's entry (M150): projects, chat and hosted workspaces at
// real paths, on an origin of its own (app.arugula.io, served through the
// hub, M151) that never loads the shell's key code ("The platform page is
// on app.arugula.io, not on the keys' origin"; web/scripts/check-imports.mjs).

import { render } from "preact";
import "../shared/style.css";
import "../shared/frame.css";
import "./platform.css";
// The lists in the swarm's language, over platform.css's.
import "./swarm.css";
import "./finish.css";
import { apply as applyAppearance } from "../shared/appearance";
import { setupDesktop } from "../shared/desktop";
import { PlatformApiContext } from "../shared/platform-api";
import { AppearanceLayer } from "../shared/ui/appearance";
import { MenuLayer, PromptLayer } from "../shared/ui/menu";
import { platformApi } from "./api";
import { PlatformPage } from "./page";

// This device's palette and finish, before anything draws: on
// app.arugula.io, the one picked on control's page too (the cookie,
// shared/appearance-cookie.ts).
applyAppearance();
// In the desktop app: macOS's buttons over the bar's left end, Linux's
// own buttons in it (the bar's CSS reads this).
setupDesktop();
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
