// Chat's alerts (#114): the words that notify you in chat as if you were
// mentioned, and, for a team you own, whether `@channel` and `@here` are
// for owners only. Control keeps both (`/api/chat/keywords`,
// `/api/chat/teams/{team}/settings`).

import { useEffect, useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import type { ChatTeamSettings } from "../../shared/proto";
import { keywords, setKeywords, setTeamSettings, teamSettings } from "../../shared/chat/alerts";
import { usePlatformApi } from "../../shared/platform-api";
import { Section } from "./section";

function Words() {
  const [words, setWords] = useState<string[] | null>(null);
  const [draft, setDraft] = useState("");
  const [err, setErr] = useState("");
  useEffect(() => {
    keywords().then(
      (k) => setWords(k.words),
      (e: Error) => setErr(e.message),
    );
  }, []);
  const save = async (next: string[]) => {
    setErr("");
    try {
      setWords((await setKeywords(next)).words);
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  if (words === null) return err ? <p class="control-error">{err}</p> : null;
  return (
    <>
      <ul class="platform-list settings-rows" data-chat-keywords>
        {words.map((w) => (
          <li key={w}>
            <b>{w}</b>
            <button class="control-linkish" data-chat-keyword-remove={w} onClick={() => void save(words.filter((x) => x !== w))}>
              Remove
            </button>
          </li>
        ))}
        {!words.length && <li class="dim">No words yet.</li>}
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
          value={draft}
          aria-label="A word or phrase to be notified of"
          placeholder="deploy failed"
          maxLength={60}
          data-chat-keyword-input
          onInput={(e) => setDraft((e.target as HTMLInputElement).value)}
        />
        <button class="control-linkish" type="submit" data-chat-keyword-add>
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
        <span>Only owners of {name} may use @channel and @here</span>
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
        <span>Only owners of {name} may set a channel's topic</span>
      </label>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}

export function ChatAlerts() {
  const api = usePlatformApi();
  // Only while chat is on for the account: the keywords are control's chat's.
  if (chat.on !== true) return null;
  const owned = api.teams().filter((t) => t.role === "owner");
  return (
    <Section
      id="chat-alerts"
      title="Chat alerts"
      lede="Words that notify you in chat as if someone had mentioned you."
      more={<p class="dim">A word counts when a message has it whole, in any case: "deploy" doesn't match "redeployed". Rooms you muted stay quiet.</p>}
      hooks={{ "data-chat-alerts": "" }}
    >
      <Words />
      {owned.map((t) => (
        <OwnersOnly key={t.team} team={t.team} name={t.roster.name || "this team"} />
      ))}
    </Section>
  );
}
