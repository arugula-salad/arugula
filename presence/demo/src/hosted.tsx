// arugula.stream/hosted: the hosted service, written for the person who'd
// buy it (a founder or PM whose shared product still feels empty), not
// for their developers: the feeling first, privacy as the reason it's
// welcome, what they don't have to run, then the price and the honest
// answers. Copy worked out with Codex (buyer profiles, objections, ranked
// selling points). Early access isn't open, so nothing is collected.

import { FakeRoom } from "./hero";
import { Nav, SOURCE } from "./landing";
import { newRoom } from "./room";

const MOMENTS = [
  {
    title: "A character with a person behind it",
    text: "They nod, and their character nods. They laugh, look away, lean in. It moves with them. Or they show up as a live line drawing of their face.",
  },
  {
    title: "A hello that starts with a wave",
    text: "Wave at someone and your character waves on their screen. A small way to say: hey, I'm here too. They wave back.",
  },
  {
    title: "A reaction with a little lift",
    text: "Send an emoji and it floats up over your tile. A little applause. A shared laugh. A moment of “yes, exactly.”",
  },
];

const USES = [
  ["Docs, design tools and whiteboards", "Let collaborators notice the person beside the work. A nod or a wave turns a shared file into a shared moment."],
  ["Coworking, focus and remote-team spaces", "Quiet work can still have company. People can see who's around without anyone broadcasting their camera."],
  ["Classes, cohorts and communities", "Classmates can see each other react. A wave hello, an emoji for a small win, a room that feels like a room."],
  ["Customer workspaces", "When your team and a customer are in the app together, make that visible, and give them an easy way to say hi."],
];

const PLANS = [
  { name: "Starter", price: "Free", per: "", fit: "Try presence in your product with a small group." },
  { name: "Team", price: "$29", per: "/ month, about", fit: "For a team adding presence to its product, priced by the people who use it.", best: true },
  { name: "Enterprise", price: "Let's talk", per: "", fit: "Your security review, your regions, your support needs." },
];

const FAQ: [string, string][] = [
  [
    "Will people actually use this?",
    "We're early, so we won't promise you a retention curve. Put it where people already want company: a shared doc, a cohort, a focus session. Then watch whether they wave. That's the whole test, and it's a cheap one.",
  ],
  [
    "Does everyone need a webcam?",
    "To animate their character, yes: the camera drives it. What's different is what gets shared. Others see the character or the line drawing, never the camera picture. Anyone without a camera still shows up, as themselves, just not moving.",
  ],
  [
    "Isn't this another way to feel watched?",
    "It's the opposite of a video grid. Nobody sees your face, your room or your bad-hair day; they see a little character that nods when you nod. It's opt-in per person, and it fits best where company is welcome.",
  ],
  [
    "What actually leaves someone's computer?",
    "About twenty bytes of pose data per frame: which way the head is turned, whether the eyes are open, how wide the smile is. Face and pose tracking run in the browser. The webcam picture never leaves it.",
  ],
  [
    "Do we need to build anything on our side?",
    "No servers. Your developers add a few lines to your frontend and connect the sign-in you already use (Clerk, Auth0, Firebase, Supabase). We run everything that has to be live.",
  ],
  [
    "How predictable is the price?",
    "You pay for monthly active users, the people who actually show up. You'll know your number before you start, and you can cap it. The figures here are a preview; we'll confirm the details with the first teams.",
  ],
  [
    "What if we want to leave?",
    "The client, the protocol and the relay are open source. You can run the whole thing yourself, and your product's code stays the same.",
  ],
  [
    "How established is arugula.stream?",
    "Brand new, and honest about it: early access isn't open yet and there are no customer logos to show you. What there is: a working product you can try right now, in a room, with a friend.",
  ],
];

export function Hosted() {
  return (
    <>
      <Nav />
      <main class="landing hosted">
        <section class="hero">
          <div>
            <p class="eyebrow">Live presence for your product · early access</p>
            <h1 class="headline">
              Make your app feel like <span>someone's here.</span>
            </h1>
            <p class="lede">
              Live characters that nod, blink and wave with the people behind them. Your users notice each other and say hello, and their webcam
              pictures never leave their browsers.
            </p>
            <div class="start">
              <a class="btn primary" href="#feel">
                See how it feels
              </a>
              <a class="btn" href={`/r/${newRoom()}`}>
                Try it with a friend
              </a>
            </div>
          </div>
          <div class="stage">
            <FakeRoom />
          </div>
          <p class="demo-note">A demo, drawn live in your browser. These people are made up.</p>
        </section>

        <section class="story">
          <h2>A shared workspace can still feel empty.</h2>
          <p>The document is open. The room has names in it. But is anyone actually here?</p>
          <p>A green dot tells you someone is connected. It can't nod, smile or wave back.</p>
          <p>And “can everyone turn their camera on?” is the fastest way to make a quiet room quieter.</p>
          <p class="turn">Give your users a little more of each other.</p>
        </section>

        <section class="dev" id="feel">
          <h2>Small signs of life, right where people already are.</h2>
          <div class="moments">
            {MOMENTS.map((m, i) => (
              <div class="moment card" style={{ "--c": ["var(--k-agent)", "var(--k-server)", "var(--k-build)"][i] }}>
                <b>{m.title}</b>
                <span>{m.text}</span>
              </div>
            ))}
          </div>
        </section>

        <section class="dev privacy">
          <h2>Share the expression. Keep the camera picture.</h2>
          <div class="split">
            <div>
              <p>The webcam picture never leaves the browser.</p>
              <p>
                arugula.stream reads the face and pose right there, then sends about twenty bytes a frame: enough to make a character nod, not enough
                to show a single pixel of anyone's room.
              </p>
              <p>Others see the character, or a line drawing of a face. That's it. That's the feature.</p>
            </div>
            <div class="bytes" aria-hidden="true">
              <span class="cam card">
                <small>your browser</small>camera picture
              </span>
              <span class="arrow">stays here ⟂</span>
              <span class="pose card">
                <small>the wire · ≈ 20 bytes / frame</small>
                {"{ yaw: 0.2, blink: 0, smile: 0.8 … }"}
              </span>
              <span class="arrow">↓</span>
              <span class="char card">
                <small>their screen</small>a character that smiles
              </span>
            </div>
          </div>
        </section>

        <section class="dev">
          <h2>You build the place. We run the live part.</h2>
          <p class="sub wide">
            No realtime servers to stand up, scale or wake up for at 3 a.m. Your product keeps its sign-in; your users get a new way to notice each
            other; we keep the rooms running.
          </p>
          <p class="devline">
            <b>For your developers:</b> a few lines of frontend code and the sign-in you already have. <a href="/docs">The docs</a> have the rest.
          </p>
        </section>

        <section class="dev next">
          <p class="eyebrow">Next · not available yet</p>
          <h2>Start with a wave. Stay for a conversation.</h2>
          <p class="sub wide">
            Sometimes a hello is enough. Sometimes there's more to say. Next, a wave will turn into a voice or video call, in the same room, with the
            person who waved back. No meeting link, no calendar invite.
          </p>
        </section>

        <section class="dev">
          <h2>Wherever people have a reason to be together.</h2>
          <div class="uses">
            {USES.map(([t, d], i) => (
              <div class="point card" style={{ "--c": ["var(--k-agent)", "var(--k-server)", "var(--k-test)", "var(--k-editor)"][i] }}>
                <b>{t}</b>
                <span>{d}</span>
              </div>
            ))}
          </div>
        </section>

        <section class="dev" id="plans">
          <h2>Priced by the people who show up.</h2>
          <p class="sub wide">
            You pay for monthly active users: one person in your rooms this month, however many rooms or tabs. Easy to plan for, easy to explain to
            finance, and you can set a cap.
          </p>
          <div class="plans three">
            {PLANS.map((p) => (
              <div class={p.best ? "plan card best" : "plan card"}>
                <b class="plan-name">{p.name}</b>
                <span class="plan-price">
                  {p.price} <small>{p.per}</small>
                </span>
                <span class="plan-fit">{p.fit}</span>
              </div>
            ))}
          </div>
          <p class="note-line">A pricing preview, for when early access opens. Calls, when they arrive, are billed by the minute.</p>
        </section>

        <section class="dev faq">
          <h2>Fair questions.</h2>
          {FAQ.map(([q, a]) => (
            <details>
              <summary>{q}</summary>
              <p>{a}</p>
            </details>
          ))}
        </section>

        <section class="try">
          <h2>Give “I'm here” a little life.</h2>
          <p class="sub">A nod. A wave. A small reaction that makes a shared space feel shared. Early access opens soon; until then, try it in a room.</p>
          <div class="start">
            <a class="btn primary" href={`/r/${newRoom()}`}>
              Try it with a friend
            </a>
            <a class="btn" href="/docs">
              Show my developers
            </a>
          </div>
        </section>
      </main>
      <footer class="foot">
        <a href="/">Home</a> · <a href="/docs">Docs</a> · <a href={SOURCE}>Open source</a>
      </footer>
    </>
  );
}
