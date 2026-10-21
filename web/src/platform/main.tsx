// The platform page's entry (M150): projects, chat and Cloud workspaces at
// real paths, on an origin of its own (app.arugula.io, served through the
// hub, M151) that never loads the shell's key code ("The platform page is
// on app.arugula.io, not on the keys' origin"; web/scripts/check-imports.mjs).

import { render } from "preact";
import "../shared/style.css";
import "./platform.css";
import { apply as applyAppearance } from "../shared/appearance";
import { cloudRouteChanged, setCloudRoutes } from "../shared/cloud";
import { PlatformApiContext } from "../shared/platform-api";
import { MenuLayer, PromptLayer } from "../shared/ui/menu";
import { platformApi } from "./api";
import { PlatformPage } from "./page";
import { pathRoutes } from "./cloud-routes";
import { onRoute } from "./router";

applyAppearance();
// Cloud's views open paths here, not the shell's fragments.
setCloudRoutes(pathRoutes);
onRoute(cloudRouteChanged);
render(
  <PlatformApiContext.Provider value={platformApi}>
    <PlatformPage />
    {/* Menus and the questions they ask (a new channel's name…). */}
    <MenuLayer />
    <PromptLayer />
  </PlatformApiContext.Provider>,
  document.getElementById("root")!,
);
