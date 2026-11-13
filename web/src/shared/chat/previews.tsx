// Link previews under a message (#108): a link to one of our own objects
// (an issue, a PR the team mirrors, a run, a deploy, a workspace, a board)
// is a small live card, a board's the board at a glance; any other link is its Open Graph card, read by control
// so this browser never contacts the site. Control draws them for the
// reader (a link to something they can't see has no card), and takes the
// message numbers, not the links: one request answers for every message
// that wants a card in the next moment (`request`).
//
// A card is the object as it is now. While the room is open the cards
// that show an object are asked for again every `REFRESH` ms, so an issue
// that closes changes its card without a reload.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../maintenance";
import { Time } from "../ui/time";
import "../boards/bar.css";
import { epicLine } from "../boards/text";
import { CloseIcon } from "../ui/icons";

export type Tone = "open" | "done" | "bad" | "wait" | "off";

export type Preview =
  | { kind: "object"; url: string; what: string; title: string; state: string; tone: Tone; who: string | null; at: number }
  | {
      kind: "board";
      url: string;
      title: string;
      goal: string | null;
      done: number;
      review: number;
      doing: number;
      total: number;
      needs_you: number;
      lead: string | null;
      closed: boolean;
      note: string | null;
      at: number;
    }
  | { kind: "web"; url: string; site: string; title: string; description: string; image: string | null };

// Polled, not pushed: a card's object changes in tables that know nothing of
// chat, and finding the rooms whose messages link to it would be a search of
// message text at every issue, run and deploy change. The cards are small
// reads of tables we already have, so a gentler minute does.
const REFRESH = 60_000;
const enc = encodeURIComponent;

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

// ---- batching: one request per room per moment

const waiting = new Map<string, Map<number, Array<(p: Preview[]) => void>>>();
let timer: ReturnType<typeof setTimeout> | undefined;

/** The cards under one message, asked together with every other message
 * asked for in the next 40 ms in the same room. [] on any failure: a link
 * that can't be unfurled is the plain link. */
export function request(room: string, seq: number): Promise<Preview[]> {
  return new Promise((done) => {
    const r = waiting.get(room) ?? new Map<number, Array<(p: Preview[]) => void>>();
    r.set(seq, [...(r.get(seq) ?? []), done]);
    waiting.set(room, r);
    timer ??= setTimeout(flush, 40);
  });
}

function flush() {
  timer = undefined;
  const batch = [...waiting];
  waiting.clear();
  for (const [room, seqs] of batch) {
    const all = [...seqs.keys()];
    for (let i = 0; i < all.length; i += 50) {
      const part = all.slice(i, i + 50);
      call<{ previews: Record<string, Preview[]> }>("GET", `/api/chat/rooms/${enc(room)}/previews?seqs=${part.join(",")}`)
        .then((r) => r.previews)
        .catch(() => ({}) as Record<string, Preview[]>)
        .then((got) => {
          for (const seq of part) for (const done of seqs.get(seq) ?? []) done(got[String(seq)] ?? []);
        });
    }
  }
}

/** Whether a message's text might have a link worth asking about. */
export function mayLink(text: string): boolean {
  return /https?:\/\//i.test(text);
}

/** A link without its fragment, normalised the way control's link reading
 * does (WHATWG URLs both), or null when it isn't one. */
function bare(link: string): string | null {
  try {
    const u = new URL(link);
    u.hash = "";
    return u.href;
  } catch {
    return null;
  }
}

/** Where a card goes: the link as the message wrote it (Arugula #879). Control
 * reads and matches links without their fragment, so a card's `url` has
 * none; but a shared Doc's key is its link's fragment (`#k=…`, which never
 * reaches a server), and without it the Doc says its link is broken. So a
 * card whose link the message wrote with a fragment opens that. */
export function asWritten(text: string, url: string): string {
  const want = bare(url);
  if (want === null) return url;
  for (const m of text.matchAll(/https?:\/\/[^\s<>()"`]+/gi)) {
    // A sentence's last mark isn't the link's, as the message's own link
    // ends (ui/markup.tsx). Not `_`: a Doc's key may end in one.
    const link = m[0].replace(/[.,;:!?'"]+$/, "");
    if (link.includes("#") && bare(link) === want) return link;
  }
  return url;
}

// ---- the cards

/** The cards under one message. `mine`: its author, who may remove them. */
export function MessagePreviews({ room, seq, text, mine }: { room: string; seq: number; text: string; mine: boolean }) {
  const [cards, setCards] = useState<Preview[]>([]);
  const [removed, setRemoved] = useState(false);
  const wants = mayLink(text);
  useEffect(() => {
    if (!wants) {
      setCards([]);
      return;
    }
    let live = true;
    const ask = () => void request(room, seq).then((p) => live && setCards(p));
    ask();
    // Only while there's an object to keep current, and only when seen.
    const t = setInterval(() => {
      if (document.visibilityState === "visible") ask();
    }, REFRESH);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [room, seq, text, wants]);
  if (!wants || removed || cards.length === 0) return null;
  return (
    <div class="link-cards" data-link-cards>
      {cards.map((card) => {
        // #879: the link as written, a Doc's key with it.
        const c = { ...card, url: asWritten(text, card.url) };
        return c.kind === "object" ? <ObjectCard key={card.url} c={c} /> : c.kind === "board" ? <BoardCard key={card.url} c={c} /> : <WebCard key={card.url} c={c} />;
      })}
      {mine && (
        <button
          class="link-card-remove"
          title="Remove the preview from this message, for everyone"
          data-link-card-remove
          onClick={() => {
            setRemoved(true);
            call("POST", `/api/chat/rooms/${enc(room)}/messages/${seq}/previews`, { hidden: true }).catch(() => setRemoved(false));
          }}
        >
          <CloseIcon size="xs" /> Remove preview
        </button>
      )}
    </div>
  );
}

function ObjectCard({ c }: { c: Extract<Preview, { kind: "object" }> }) {
  return (
    <a class="link-card object" href={c.url} data-link-card-object data-what={c.what}>
      <span class="link-card-top">
        <span class="link-card-what">{c.what}</span>
        <span class={`link-card-state tone-${c.tone}`} data-link-card-state>
          {c.state}
        </span>
      </span>
      <span class="link-card-title">{c.title}</span>
      <span class="link-card-meta">
        {c.who ? `${c.who} · ` : ""}
        <Time at={Math.min(c.at, Date.now())} />
      </span>
    </a>
  );
}

function BoardCard({ c }: { c: Extract<Preview, { kind: "board" }> }) {
  // The board's own words and segments; `proven` is the board page's, not the card's.
  const e = epicLine({ ...c, proven: 0 });
  return (
    <a class="link-card object board-link" href={c.url} data-link-card-object data-what="board" data-link-card-board>
      <span class="link-card-top">
        <span class="link-card-what">Board</span>
        {c.closed && (
          <span class="link-card-state tone-off" data-link-card-state>
            Closed
          </span>
        )}
      </span>
      <span class="link-card-title">{c.title}</span>
      {c.goal && <span class="link-card-desc" data-link-card-goal>{c.goal}</span>}
      {c.total > 0 && (
        <span class="board-bar" role="progressbar" aria-valuemin={0} aria-valuemax={c.total} aria-valuenow={c.done} aria-label={`${c.done} of ${c.total} done`}>
          {e.segments.map((s) => (
            <i key={s.lane} data-lane={s.lane} style={{ width: `${s.pct}%` }} />
          ))}
        </span>
      )}
      <span class="link-card-meta" data-link-card-progress>
        {c.total > 0 ? `${c.done} of ${c.total} done` : "No work yet"}
        {c.needs_you > 0 && <b class="link-card-needs" data-link-card-needs-you>{` · ${c.needs_you} need you`}</b>}
      </span>
      {c.lead && <span class="link-card-meta" data-link-card-lead>Led by {c.lead}</span>}
      {c.closed && c.note && <span class="link-card-desc" data-link-card-note>{c.note}</span>}
    </a>
  );
}

function WebCard({ c }: { c: Extract<Preview, { kind: "web" }> }) {
  return (
    <a class="link-card web" href={c.url} target="_blank" rel="noopener noreferrer nofollow" data-link-card-web>
      <span class="link-card-text">
        <span class="link-card-what">{c.site}</span>
        <span class="link-card-title">{c.title || c.url}</span>
        {c.description && <span class="link-card-desc">{c.description}</span>}
      </span>
      {c.image && <img class="link-card-image" src={c.image} alt="" loading="lazy" referrerpolicy="no-referrer" />}
    </a>
  );
}

// ---- the team's switch

/** A team owner's switch for web previews in the team's rooms. Object
 * cards stay: they fetch nothing. */
export function WebPreviewsSwitch({ team, toast }: { team: string; toast: (m: string) => void }) {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    call<{ web: boolean }>("GET", `/api/chat/teams/${enc(team)}/previews`)
      .then((r) => setOn(r.web))
      .catch(() => setOn(null));
  }, [team]);
  if (on === null) return null;
  return (
    <label class="chat-setting" data-web-previews>
      <input
        type="checkbox"
        checked={on}
        onChange={(e) => {
          const web = (e.currentTarget as HTMLInputElement).checked;
          call("PUT", `/api/chat/teams/${enc(team)}/previews`, { web }).then(
            () => setOn(web),
            (err: Error) => toast(err.message),
          );
        }}
      />{" "}
      Preview web links
    </label>
  );
}
