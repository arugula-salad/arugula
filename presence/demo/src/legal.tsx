// arugula.stream/privacy and /terms: short and plain, true to what the
// relay keeps (accounts, messages, meet's rooms, apps, usage) and what it
// never does (store frames, send a camera picture, run ads or analytics).
// Google's consent screen links both.

import type { ComponentChildren } from "preact";
import { Nav, SOURCE } from "./landing";

const UPDATED = "October 29, 2026";
const CONTACT = "hello@arugula.stream";

function Page(props: { title: string; children: ComponentChildren }) {
  return (
    <>
      <Nav />
      <main class="legal">
        <h1>{props.title}</h1>
        <p class="updated">Last updated {UPDATED}</p>
        {props.children}
      </main>
      <footer class="foot">
        <a href="/">Home</a> · <a href="/privacy">Privacy</a> · <a href="/terms">Terms</a> · <a href={SOURCE}>Open source</a>
      </footer>
    </>
  );
}

export function Privacy() {
  return (
    <Page title="Privacy">
      <p class="lede-small">
        The short version: presence never sends your camera picture. Face and pose tracking run in your browser, and only a few bytes of pose (or
        the lines of an outline) leave it. Calls are different: a voice or video call sends real audio and video to the people in it.
      </p>

      <h2>What we keep</h2>
      <ul>
        <li>
          <b>Your account</b>, if you sign in: your email address and name. With Google, also your Google account id. Sign-in links are stored only
          as a hash and stop working after 15 minutes or one use.
        </li>
        <li>
          <b>Text messages</b> you send in a room, with your name and the time, so people who join later can scroll back.
        </li>
        <li>
          <b>Rooms on meet.arugula.stream</b> you make, and the ones you've been in lately, so you can find them again.
        </li>
        <li>
          <b>Apps</b>, if you're a developer: your app's settings, keys, allowed sites and sign-in provider settings.
        </li>
        <li>
          <b>Usage</b>: how many people joined each app's rooms each month and how many call minutes they used, to run limits and, later, billing.
          For guests that means a random id kept in their browser, not who they are.
        </li>
        <li>
          <b>Request logs</b>: our servers and hosting provider keep basic logs (like IP address and time) for a short while, for security and
          fixing problems.
        </li>
      </ul>

      <h2>What we don't keep</h2>
      <ul>
        <li>Camera pictures. Presence frames (pose or outline) are passed along live; the newest one is held in memory for a few seconds so someone joining sees you right away, and never stored.</li>
        <li>Call audio or video. Calls go through our calls provider live and aren't recorded.</li>
        <li>Waves and emoji reactions. They're passed along and gone.</li>
      </ul>

      <h2>Cookies and your browser</h2>
      <p>
        One cookie keeps you signed in. Your browser also remembers your choices (your name, character and camera mode) in its own storage. No ad
        or tracking cookies, and no analytics.
      </p>

      <h2>Who helps us run it</h2>
      <ul>
        <li>Fly.io hosts the servers and the database, in the United States.</li>
        <li>LiveKit carries voice and video calls.</li>
        <li>Resend sends sign-in emails.</li>
        <li>Google, if you choose to sign in with Google.</li>
      </ul>
      <p>We don't sell your data, and we don't show ads.</p>

      <h2>Inside someone else's app</h2>
      <p>
        If you meet arugula.stream inside another company's product, that company decides who's in which room and what name you show up with.
        Their privacy policy covers what they do; this one covers what we do for them.
      </p>

      <h2>Keeping and deleting</h2>
      <p>
        We keep your account and messages until you ask us to delete them, and usage totals as long as we need them for limits and billing. To see
        or delete your data, email <a href={`mailto:${CONTACT}`}>{CONTACT}</a>.
      </p>

      <h2>Changes</h2>
      <p>If this changes, we'll update this page and the date at the top.</p>
    </Page>
  );
}

export function Terms() {
  return (
    <Page title="Terms">
      <p class="lede-small">
        These terms cover arugula.stream: this site, meet.arugula.stream and the hosted presence service. By using them, you agree to these terms.
      </p>

      <h2>Early access</h2>
      <p>
        arugula.stream is new. Features may change, break or go away, and it's free while early access lasts. Before we charge for anything,
        we'll say what it costs and give you notice.
      </p>

      <h2>Your account and keys</h2>
      <p>
        Keep your email account secure; signing in to it signs you in here. If you build on arugula.stream, you're responsible for what's done with
        your app and its keys, and your server secret must stay secret.
      </p>

      <h2>Be decent</h2>
      <p>Don't use arugula.stream to:</p>
      <ul>
        <li>break the law, or help someone else break it;</li>
        <li>harass, threaten or impersonate anyone;</li>
        <li>send spam or malware;</li>
        <li>record or show people without the consent the law requires;</li>
        <li>attack, overload or get around the limits of the service.</li>
      </ul>

      <h2>Your content</h2>
      <p>
        What you send (your messages, your name, your look) is yours. You let us carry and store it as needed to run the service, and nothing more.
      </p>

      <h2>If you build an app on it</h2>
      <p>
        You're responsible for your own users, for telling them how you use arugula.stream in your own privacy policy, and for staying within your
        plan's limits.
      </p>

      <h2>Open source</h2>
      <p>
        The client, the protocol and the relay are <a href={SOURCE}>open source</a> under their own licenses. These terms are about the hosted
        service we run.
      </p>

      <h2>Stopping</h2>
      <p>You can stop using arugula.stream at any time. We may suspend accounts or apps that break these terms.</p>

      <h2>No guarantees</h2>
      <p>
        arugula.stream is provided as is, without warranties. As far as the law allows, we're not liable for indirect or consequential losses,
        and our total liability is limited to what you paid us in the 12 months before the claim.
      </p>

      <h2>Changes and questions</h2>
      <p>
        If these terms change, we'll update this page and the date at the top. Questions: <a href={`mailto:${CONTACT}`}>{CONTACT}</a>. See also
        our <a href="/privacy">privacy page</a>.
      </p>
    </Page>
  );
}
