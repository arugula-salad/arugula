// A person's picture in chat: their GitHub avatar as control serves it
// (never github's own host), or their initials on their color while it
// loads and when they have none. An agent shows its owner's, with a small
// machine badge. A dot says who's around (#110).

import { useState } from "preact/hooks";
import { useSubscribe } from "../ui/hooks";
import { chat } from "../chat-store";
import { presence } from "./presence";
import { LiveFace } from "../live/face";

/** A steady colour per person. */
export function colorOf(who: string): string {
  let h = 0;
  for (const c of who) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 70% 68%)`;
}

/** Where control serves an account's picture (404 for one the reader
 * doesn't share a team with, or that has none). 128 px, so it stays sharp
 * at 3x on the biggest avatar. */
export function avatarUrl(account: string): string {
  return `/api/chat/avatar/${encodeURIComponent(account)}`;
}

/** Accounts control had no picture for, so a list of them doesn't ask again. */
const none = new Set<string>();

function initialsOf(name: string): string {
  return name
    .split(/[\s@]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}

/** The dot on an avatar: a person's state, or an agent's machine's. */
function Dot({ account, machine }: { account?: string; machine?: string }) {
  useSubscribe((fn) => presence.subscribe(fn));
  const state = machine ? (presence.machine(machine) ? "online" : "offline") : account ? presence.of(account) : null;
  if (!state) return null;
  return <span class={`presence-dot presence-${state}`} data-presence={state} title={state} role="img" aria-label={state} />;
}

/** A message's picture: the account's (`account`; none for a deleted one),
 * or their initials on their color. `agent` is the machine of an agent's
 * message: the owner's picture, a machine badge, and the machine's dot.
 * `dot` shows presence (a person's; an agent's machine's). */
export function MsgAvatar({
  who,
  name,
  account,
  agent,
  dot,
}: {
  who: string;
  name: string;
  /** Whose picture. None: letters only. */
  account?: string;
  agent?: string;
  dot?: boolean;
}) {
  // The picture sits over the letters and shows once it has loaded, so the
  // letters are what shows while it loads and when control has none.
  const [shown, setShown] = useState(false);
  const [failed, setFailed] = useState(() => !!account && none.has(account));
  const face = (
    <span class="msg-avatar" style={{ background: colorOf(who) }} aria-hidden="true">
      {initialsOf(name)}
      {account && !failed && (
        <img
          class={shown ? "loaded" : ""}
          src={avatarUrl(account)}
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          onLoad={() => setShown(true)}
          onError={() => {
            // Not asked for again this session: control said it has none.
            if (account) none.add(account);
            setFailed(true);
          }}
        />
      )}
      {/* #344: their live face over the picture while they're live (a person's; an agent isn't them). */}
      {account && !agent && <LiveFace account={account} />}
    </span>
  );
  if (!dot && !agent) return face;
  return (
    <span class={agent ? "avatar-wrap by-agent" : "avatar-wrap"}>
      {face}
      {agent && (
        <span class="avatar-badge" title="An agent" aria-hidden="true">
          ▣
        </span>
      )}
      {dot && <Dot account={agent ? undefined : account} machine={agent} />}
    </span>
  );
}

/** Whose picture last loaded on the account button, in this browser: shown
 * while chat is still starting, so the button doesn't go from the picture
 * to its letter and back from one load to the next (UI audit #162). */
const SEEN = "arugula.account-picture";

function seen(): string | null {
  try {
    return localStorage.getItem(SEEN);
  } catch {
    return null;
  }
}

/** The signed-in person's own picture on the account button, over its
 * letter: while chat is on (control serves pictures to chat's people), or
 * while it's starting where it loaded before, and not when it has none. */
export function AccountPicture() {
  useSubscribe((fn) => chat.subscribe(fn));
  const [failed, setFailed] = useState(false);
  const who = chat.on === true ? chat.me : chat.on === null ? seen() : null;
  if (!who || failed || none.has(who)) return null;
  return (
    <img
      class="account-picture"
      src={avatarUrl(who)}
      alt=""
      decoding="async"
      referrerpolicy="no-referrer"
      onLoad={() => {
        try {
          localStorage.setItem(SEEN, who);
        } catch {
          // A private window keeps nothing: the letter shows until chat starts.
        }
      }}
      onError={() => {
        none.add(who);
        try {
          if (seen() === who) localStorage.removeItem(SEEN);
        } catch {
          // As above.
        }
        setFailed(true);
      }}
    />
  );
}
