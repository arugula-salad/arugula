// The fixture page for the design reference (design-fields.spec.ts): the
// /design page alone, with the app's stylesheets in the app's order
// (app/main.tsx), so each piece is drawn as it is in the app.

import { render } from "preact";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/shared/switch.css";
import "../src/platform/settings.css";
import "../src/shared/fields.css";
import "../src/shared/buttons.css";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { DesignPage } from "../src/platform/pages/design";

const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  <>
    <DesignPage />
    <PromptLayer />
    <MenuLayer />
  </>,
  root,
);
