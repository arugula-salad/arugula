// A snippet to paste, with this app's own key: a script tag, React, or
// the JS API, for the sign-in provider it uses (the docs' own snippets,
// ../../demo/src/snippets.ts, with the key filled in), or for guests.
// Off arugula.stream (staging, a relay of your own) they name this
// relay's SDK and socket.

import { useState } from "preact/hooks";
import { SDK as HOSTED_SDK, install, providers } from "../../demo/src/snippets";
import type { App, Issuer, Provider } from "./api";
import { PRESETS } from "./provider";
import { Section, Tabs } from "./ui";

const TAB: Record<Provider, string> = { clerk: "Clerk", auth0: "Auth0", firebase: "Firebase", supabase: "Supabase", oidc: "OIDC" };

/** This relay's SDK and socket, when it isn't arugula.stream. */
function relay(): { sdk: string; socket: string | null } {
  if (location.hostname === "arugula.stream") return { sdk: HOSTED_SDK, socket: null };
  const ws = location.protocol === "https:" ? "wss:" : "ws:";
  return { sdk: `${location.origin}/sdk/v0/presence.js`, socket: `${ws}//${location.host}/socket` };
}

/** The docs' snippet for a provider, without its "issuer: …" comment
 * (the dashboard has set that), with `key` and this relay. */
function fill(code: string, key: string, sdk: string, socket: string | null): string {
  const lines = code.split("\n");
  let i = 0;
  if (lines[0]?.startsWith("// issuer:")) {
    i = 1;
    while (lines[i]?.startsWith("//  ")) i++;
  }
  let out = lines.slice(i).join("\n").replaceAll("pk_live_…", key).replaceAll(HOSTED_SDK, sdk);
  if (socket) {
    out = out
      .replace(/(\n\s*)key: "/, `$1relay: "${socket}",$1key: "`)
      .replace(/<arugula-room room="design-team"/, `<arugula-room room="design-team" relay="${socket}"`)
      .replace(/<PresenceRoom room="design-team"/, `<PresenceRoom room="design-team" relay="${socket}"`);
  }
  return out;
}

export function snippets(app: App, issuer: Issuer | null, key: string): Record<string, string> {
  const { sdk, socket } = relay();
  const script = `<script type="module" src="${sdk}"></script>\n\n`;
  const react = `// ${install.React}\nimport { PresenceRoom, People } from "@arugula/presence-react";\n\n`;
  const js = `import { connect } from "${sdk}";\n\n`;
  const guest = {
    HTML: `<arugula-room room="design-team" key="pk_live_…" name="Maya">\n  <arugula-camera></arugula-camera>\n  <arugula-people></arugula-people>\n</arugula-room>`,
    React: `<PresenceRoom room="design-team" publishableKey="pk_live_…" guest="Maya">\n  <People />\n</PresenceRoom>`,
    JS: `const room = await connect({\n  room: "design-team",\n  key: "pk_live_…",\n  guest: "Maya",                  // their name, for everyone to see\n});\nroom.mount(document.querySelector("#people"));`,
  };
  const f = (code: string) => fill(code, key, sdk, socket);
  const tabs: Record<string, string> = issuer
    ? {
        "Script tag": script + f(providers.HTML!),
        React: react + f(providers.React!),
        JavaScript: js + f(providers[TAB[issuer.provider]]!),
      }
    : {
        "Script tag": script + f(guest.HTML),
        React: react + f(guest.React),
        JavaScript: js + f(guest.JS),
      };
  if (issuer && app.guests) tabs["Guests"] = js + f(guest.JS);
  return tabs;
}

export function Quickstart({ app, issuer }: { app: App; issuer: Issuer | null }) {
  const live = app.keys.find((k) => k.mode === "live");
  const test = app.keys.find((k) => k.mode === "test");
  const [mode, setMode] = useState<"live" | "test">(test ? "test" : "live");
  const key = (mode === "test" ? test : live) ?? test ?? live;
  return (
    <Section
      id="quickstart"
      title="Quickstart"
      kind="--k-server"
      sub={
        issuer ? (
          <>
            Paste one into your page. <code>auth</code> hands over the person's {PRESETS[issuer.provider].label} token; everything else is ours.
          </>
        ) : app.guests ? (
          "Paste one into your page. Guests are on, so this works as it is; connect your sign-in below so people come in as themselves."
        ) : (
          "Paste one into your page. To connect, set your sign-in provider below, or turn guests on."
        )
      }
    >
      {!key ? (
        <p class="empty">Make a key first.</p>
      ) : (
        <>
          {live && test && (
            <div class="seg" role="radiogroup" aria-label="Which key">
              <button role="radio" aria-checked={key.mode === "test"} class={key.mode === "test" ? "btn small on" : "btn small"} onClick={() => setMode("test")}>
                Test key
              </button>
              <button role="radio" aria-checked={key.mode === "live"} class={key.mode === "live" ? "btn small on" : "btn small"} onClick={() => setMode("live")}>
                Live key
              </button>
            </div>
          )}
          <Tabs tabs={snippets(app, issuer, key.key)} label="Quickstart" />
          {issuer?.room_rule && (
            <p class="hint">
              Your room rule is on: name rooms so it lets people in{issuer.room_rule.match === "prefix" ? ", as in acme:design-team" : ""}.
            </p>
          )}
          <p class="hint">
            More in <a href="/docs#sign-in">the docs</a>: elements, events, the chat and calls.
          </p>
        </>
      )}
    </Section>
  );
}
