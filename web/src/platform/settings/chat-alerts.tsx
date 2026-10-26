// Chat's alerts (#114): the words that notify you in chat as if you were
// mentioned, on /settings; and, for a team you own, whether `@channel` and
// `@here` and a channel's topic are for owners only, which is the team's,
// so it's in its project's settings, as Team chat (UI audit #168). Control
// keeps both (`/api/chat/keywords`, `/api/chat/teams/{team}/settings`).

import { useEffect, useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import type { ChatTeamSettings } from "../../shared/proto";
import { keywords, setKeywords, setTeamSettings, teamSettings } from "../../shared/chat/alerts";
import { useSubscribe } from "../../shared/ui/hooks";
import type { PlatformProject } from "../session";
import { Section } from "./section";

function Words({ words, setWords }: { words: string[]; setWords: (w: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const [err, setErr] = useState("");
  const save = async (next: string[]) => {
    setErr("");
    try {
      setWords((await setKeywords(next)).words);
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <>
      <ul class="platform-list settings-rows" data-chat-keywords>
        {words.map((w) => (
          <li key={w}>
            <b>{w}</b>
            <button class="secondary small" data-chat-keyword-remove={w} onClick={() => void save(words.filter((x) => x !== w))}>
              Remove
            </button>
          </li>
        ))}
        {!words.length && <li class="dim">None yet.</li>}
      </ul>
      <form
        class="settings-add"
        onSubmit={(e) => {
          e.preventDefault();
          if (!draft.trim()) return;
          void save([...words, draft]).then(() => setDraft(""));
        }}
      >
        <input
          class="field"
          value={draft}
          aria-label="A word or phrase to be notified of"
          placeholder="deploy failed"
          maxLength={60}
          data-chat-keyword-input
          onInput={(e) => setDraft((e.target as HTMLInputElement).value)}
        />
        <button class="primary" type="submit" data-chat-keyword-add>
          Add
        </button>
      </form>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}

function OwnersOnly({ team, name }: { team: string; name: string }) {
  const [s, setS] = useState<ChatTeamSettings | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    teamSettings(team).then(setS, (e: Error) => setErr(e.message));
  }, [team]);
  if (s === null) return err ? <p class="control-error">{err}</p> : null;
  // The two switches are one record on the server: each sends both.
  const flip = (change: Partial<ChatTeamSettings>) => void setTeamSettings(team, { ...s, ...change }).then(setS, (e: Error) => setErr(e.message));
  return (
    <>
      <label class="settings-switch">
        <input
          type="checkbox"
          role="switch"
          class="sw-switch"
          data-chat-owners-only={team}
          checked={s.broadcast_owners}
          onChange={() => flip({ broadcast_owners: !s.broadcast_owners })}
        />
        <span>Only {name}'s owners may use @channel and @here</span>
      </label>
      <label class="settings-switch">
        <input
          type="checkbox"
          role="switch"
          class="sw-switch"
          data-chat-topic-owners={team}
          checked={s.topic_owners}
          onChange={() => flip({ topic_owners: !s.topic_owners })}
        />
        <span>Only {name}'s owners may set a channel's topic</span>
      </label>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}

export function ChatAlerts() {
  useSubscribe((fn) => chat.subscribe(fn));
  // Only while chat is on for the account: the keywords are control's chat's.
  if (chat.on !== true) return null;
  return <Alerts />;
}

function Alerts() {
  const [words, setWords] = useState<string[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    keywords().then(
      (k) => setWords(k.words),
      (e: Error) => setErr(e.message),
    );
  }, []);
  if (words === null) return err ? <p class="control-error">{err}</p> : null;
  return (
    <Section
      id="chat-alerts"
      title="Chat alerts"
      count={words.length}
      lede="Words that notify you in chat as if someone had mentioned you."
      more={<p class="dim">A word counts when a message has it whole, in any case: "deploy" doesn't match "redeployed". Rooms you muted stay quiet.</p>}
      hooks={{ "data-chat-alerts": "" }}
    >
      <Words words={words} setWords={setWords} />
    </Section>
  );
}

/** A team project's settings, for its owners while chat is on: who in the
 * team may notify a whole room, and set a channel's topic. */
export function TeamChat({ p }: { p: PlatformProject }) {
  useSubscribe((fn) => chat.subscribe(fn));
  if (chat.on !== true || p.team?.role !== "owner") return null;
  return (
    <Section
      id="team-chat"
      title="Team chat"
      lede={`Who in ${p.team.name || "this team"} may notify a whole room, and set a channel's topic.`}
      more={<p>Everyone in the team may, until you turn these on. @channel notifies everyone in a room; @here, those active now.</p>}
      hooks={{ "data-team-chat": p.team.id }}
    >
      <OwnersOnly team={p.team.id} name={p.team.name || "the team"} />
    </Section>
  );
}
