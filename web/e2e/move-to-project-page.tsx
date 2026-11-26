// The fixture page for "Move to project…" (Arugula #971): the app's own
// pages (the platform's router, rail and pages), with every platform answer
// the spec's (move-to-project.spec.ts routes them), and one machine the page
// reaches, `m_mac`, whose Doc moves are kept on `window.__movedDocs` for the
// spec to read, and its Docs shared by link (docs-shared.spec.ts). Built
// with Vite and served at /p/… on the daemon's origin.

import { render } from "preact";
import "../src/shared/fonts.css";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/platform/settings.css";
import "../src/shared/fields.css";
import "../src/shared/buttons.css";
import { PlatformApiContext } from "../src/shared/platform-api";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { FeedbackLayer } from "../src/shared/ui/feedback";
import { platformApi } from "../src/platform/api";
import { PlatformPage } from "../src/platform/page";
import { localBackend, setSeenBackend } from "../src/shared/seen";

const moved: [string, string, string][] = [];
(window as unknown as { __movedDocs: typeof moved }).__movedDocs = moved;
// Docs' Shared by you (Arugula #1315): `m_mac` answers and titles its
// links; `m_off` is offline. What's asked of them is on `window.__links`.
const links: string[] = [];
(window as unknown as { __links: typeof links }).__links = links;

setSeenBackend(localBackend());

const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  <PlatformApiContext.Provider
    value={{
      ...platformApi,
      docLinks: {
        watch: () => () => {},
        here: (machine) => machine === "m_mac",
        name: (machine) => ({ m_mac: "mac", m_off: "laptop" })[machine],
        titles: async (machine) => new Map(machine === "m_mac" ? [["lnk_mac", "Launch plan"]] : []),
        url: async (_, link) => `https://share.example/d/${link}#key`,
        open: async (machine, link) => (links.push(`open ${machine} ${link}`), 7),
        unshare: async (machine, link) => (links.push(`unshare ${machine} ${link}`), false),
      },
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
    <FeedbackLayer />
  </PlatformApiContext.Provider>,
  root,
);
