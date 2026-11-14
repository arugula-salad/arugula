// The fixture page for "Move to project…" (Arugula #971): the app's own
// pages (the platform's router, rail and pages), with every platform answer
// the spec's (move-to-project.spec.ts routes them), and one machine the page
// reaches, `m_mac`, whose Doc moves are kept on `window.__movedDocs` for the
// spec to read. Built with Vite and served at /p/… on the daemon's origin.

import { render } from "preact";
import "../src/shared/fonts.css";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/platform/settings.css";
import "../src/platform/finish.css";
import "../src/shared/buttons.css";
import { PlatformApiContext } from "../src/shared/platform-api";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { platformApi } from "../src/platform/api";
import { PlatformPage } from "../src/platform/page";
import { localBackend, setSeenBackend } from "../src/shared/seen";

const moved: [string, string, string][] = [];
(window as unknown as { __movedDocs: typeof moved }).__movedDocs = moved;

setSeenBackend(localBackend());

const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  <PlatformApiContext.Provider
    value={{
      ...platformApi,
      machineDocs: {
        watch: () => () => {},
        here: (machine) => machine === "m_mac",
        move: async (machine, doc, project) => {
          moved.push([machine, doc, project]);
          return null;
        },
      },
    }}
  >
    <PlatformPage />
    <MenuLayer />
    <PromptLayer />
  </PlatformApiContext.Provider>,
  root,
);
