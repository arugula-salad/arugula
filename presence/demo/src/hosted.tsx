// arugula.stream/hosted: the pitch for running it on our relay, sold by
// usage: frontend code only (a publishable key, and the auth your users
// already have), presence now and calls next. A preview: sign-ups aren't
// open, so the page collects nothing.

import { Code, Tabs } from "./code";
import { FakeRoom } from "./hero";
import { Nav, SOURCE } from "./landing";
import { newRoom } from "./room";
import { SDK } from "./snippets";

const embed = `<script type="module" src="${SDK}"></script>

<arugula-room key="pk_live_7Hc2…" room="design-team" auth="clerk">
  <arugula-camera></arugula-camera>
  <arugula-people></arugula-people>
</arugula-room>`;

const auth = {
  Clerk: `<arugula-room key="pk_live_…" room="design-team" auth="clerk">
<!-- The element asks Clerk for the signed-in user's session token;
     we verify it against your Clerk instance (set in the dashboard). -->`,
  Auth0: `const room = await connect({
  key: "pk_live_…",
  room: "design-team",
  auth: () => auth0.getTokenSilently(),   // verified against your Auth0 tenant
});`,
  Firebase: `const room = await connect({
  key: "pk_live_…",
  room: "design-team",
  auth: () => getAuth().currentUser.getIdToken(),   // verified against your project
});`,
  Supabase: `const room = await connect({
  key: "pk_live_…",
  room: "design-team",
  auth: async () => (await supabase.auth.getSession()).data.session.access_token,
});`,
  "Any OIDC": `// Dashboard: issuer https://login.example.com, audience my-app.
// We fetch its JWKS and check every token's signature, expiry and audience.
const room = await connect({ key: "pk_live_…", room, auth: () => myAuth.idToken() });`,
  Guests: `<!-- No sign-in: a name, your allowed origins, and the room rules you set. -->
<arugula-room key="pk_live_…" room="open-house" guest name="Visitor">
  <arugula-people></arugula-people>
</arugula-room>`,
} as Record<string, string>;

const PLANS = [
  {
    name: "Hobby",
    price: "Free",
    per: "",
    lines: ["100 monthly active users", "Avatars, outlines, waves, reactions", "Demo-grade limits, one app", "Community support"],
  },
  {
    name: "Team",
    price: "$29",
    per: "/ month",
    lines: ["2,500 monthly active users, then 1¢ each", "Your auth provider, guests, room rules", "Webhooks and a usage dashboard", "Email support"],
    best: true,
  },
  {
    name: "Calls",
    price: "Add-on",
    per: "coming next",
    lines: ["A wave becomes a call, in the same room", "Voice ¼¢, video ½¢ per participant-minute", "Group calls on our SFU, worldwide", "Billed with your plan"],
  },
  {
    name: "Enterprise",
    price: "Let's talk",
    per: "",
    lines: ["SSO and SCIM for the dashboard", "Regions and data residency", "Support for running the relay yourself", "SLA and DPA"],
  },
];

const FAQ: [string, string][] = [
  [
    "Do I need a server?",
    "No. Your page loads the script with a publishable key, and passes in the token your users already get from your auth provider. We verify it against the issuer you configure, so who's signed in is your auth's call, not a token you mint. Guest rooms need no sign-in at all.",
  ],
  [
    "What counts as a monthly active user?",
    "Anyone who joined at least one of your rooms in the calendar month, once, however many rooms, tabs or devices. Watching counts; it doesn't have to be with a camera on.",
  ],
  [
    "Is anyone's camera picture sent to you?",
    "Never. Tracking runs in the browser; what reaches the relay is a rig of about twenty bytes, or line outlines. We relay it and keep only each person's newest frame in memory, for someone joining.",
  ],
  [
    "When do calls arrive?",
    "Next. Waving already asks someone to talk; calls will pick up from the wave in the same room, voice first, then video, on a hosted SFU.",
  ],
  [
    "Can I leave?",
    "Yes. The client and the protocol are open source, and so is the relay: run it yourself with server-signed tokens, and the same frontend code works against it.",
  ],
];

export function Hosted() {
  return (
    <>
      <Nav />
      <main class="landing hosted">
        <section class="hero">
          <div>
            <p class="eyebrow">arugula.stream hosted · early access</p>
            <h1 class="headline">Presence and calls on our servers. Frontend code only.</h1>
            <p class="lede">
              A script tag, a publishable key, and the sign-in your users already have. We run the rooms, the relay at the edge and, next, the calls.
              You pay for the people who use it.
            </p>
            <div class="start">
              <a class="btn primary" href="#plans">
                See the plans
              </a>
              <a class="btn" href={`/r/${newRoom()}`}>
                Try a room
              </a>
            </div>
          </div>
          <FakeRoom />
        </section>

        <section class="dev">
          <h2>All of it, in your frontend</h2>
          <p class="sub">No token endpoint, no relay to run. The key is publishable: it only works from the origins you allow.</p>
          <Code code={embed} label="The hosted embed" />
        </section>

        <section class="dev">
          <h2>Bring the sign-in you have</h2>
          <p class="sub">
            Point the dashboard at your auth provider and we verify your users' own tokens: signature, expiry, audience. Who may join which room is a
            rule you set there, by claim, or a guest room for anyone on your site.
          </p>
          <Tabs tabs={auth} label="Who's who" />
        </section>

        <section class="dev how">
          <h2>What we run</h2>
          <div class="flow">
            <div class="box">
              <b>Your page</b>
              <span>the script, your key, your user's token; tracking in the browser</span>
            </div>
            <i aria-hidden="true">→</i>
            <div class="box hi">
              <b>arugula.stream</b>
              <span>verifies the token, rooms, presence, waves, reactions; relays ~20 B frames at the edge</span>
            </div>
            <i aria-hidden="true">→</i>
            <div class="box">
              <b>Calls (next)</b>
              <span>a wave becomes voice or video, on our SFU, in the same room</span>
            </div>
          </div>
          <div class="points">
            <div class="point">
              <b>Edge relay</b>
              <span>Rooms near your people, Phoenix underneath, presence that's right to the second.</span>
            </div>
            <div class="point">
              <b>Room rules</b>
              <span>Who may join, by a claim in your users' tokens, the room's name pattern, or a guest list.</span>
            </div>
            <div class="point">
              <b>Webhooks</b>
              <span>joined, left, waved, reacted: into your backend, if you have one.</span>
            </div>
            <div class="point">
              <b>Usage you can read</b>
              <span>Active users, rooms and call minutes per day, and a cap so a launch day can't surprise you.</span>
            </div>
          </div>
        </section>

        <section class="dev" id="plans">
          <h2>Plans</h2>
          <p class="sub">A preview: we're setting prices with the first teams, so these may change before sign-ups open.</p>
          <div class="plans">
            {PLANS.map((p) => (
              <div class={p.best ? "plan best" : "plan"}>
                <b class="plan-name">{p.name}</b>
                <span class="plan-price">
                  {p.price} <small>{p.per}</small>
                </span>
                <ul>
                  {p.lines.map((l) => (
                    <li>{l}</li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </section>

        <section class="dev faq">
          <h2>Questions</h2>
          {FAQ.map(([q, a]) => (
            <details>
              <summary>{q}</summary>
              <p>{a}</p>
            </details>
          ))}
        </section>

        <section class="try">
          <h2>Early access opens soon</h2>
          <p class="sub">
            Until then, everything here runs free on demo rooms, and the open-source relay with server tokens is yours to host. The frontend code
            won't change when you move.
          </p>
          <div class="start">
            <a class="btn primary" href="/docs#quickstart">
              Read the quickstart
            </a>
            <a class="btn" href={`/r/${newRoom()}`}>
              Try a room
            </a>
          </div>
        </section>
      </main>
      <footer class="foot">
        <a href="/">Home</a> · <a href="/docs">Docs</a> · <a href={`${SOURCE}/PROTOCOL.md`}>Protocol</a> · <a href={SOURCE}>Source</a>
      </footer>
    </>
  );
}
