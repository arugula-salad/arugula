//! What counts as a mention beyond a name (#114): `@channel`, `@here` and
//! an account's keyword alerts, and the settings that go with them: the
//! team's owners-only guard and each account's words.
//!
//! All of them resolve to account ids stored in a message's `mentions`
//! beside the named people, so the per-room mention counts, the push and
//! Activity see one kind of thing. What differs is only the push's words
//! ([`ChatPushWhy::Keyword`]).

use super::*;
use wire::{ChatKeywords, ChatTeamSettings, KEYWORD_CHARS, KEYWORDS_MAX};

/// The two words that reach a room's people.
pub const CHANNEL: &str = "channel";
pub const HERE: &str = "here";

/// Which broadcast words a text uses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Broadcast {
    pub channel: bool,
    pub here: bool,
}

impl Broadcast {
    pub fn of(words: &[String]) -> Self {
        Self { channel: words.iter().any(|w| w == CHANNEL), here: words.iter().any(|w| w == HERE) }
    }

    pub fn any(self) -> bool {
        self.channel || self.here
    }
}

/// Who in `people` is active now, for `@here`. Presence (#110) fills this
/// in; until it lands everyone is active, so `@here` acts as `@channel`.
pub async fn active(_app: &Platform, _room: &Room, people: &[String]) -> anyhow::Result<Vec<String>> {
    Ok(people.to_vec())
}

/// Whom `@channel` and `@here` reach: the room's people but the author,
/// those of `here` only for `@here` alone.
pub fn reach(b: Broadcast, people: &[String], active: &[String], author: &str) -> Vec<String> {
    let from = if b.channel {
        people
    } else if b.here {
        active
    } else {
        return vec![];
    };
    from.iter().filter(|a| *a != author).cloned().collect()
}

/// Whether `text` has `keyword` (lowercase) as a word or phrase: not inside
/// a longer word, so "deploy" doesn't alert on "redeployed".
pub fn has_keyword(text: &str, keyword: &str) -> bool {
    let text = text.to_lowercase();
    let edge = |c: Option<char>| !c.is_some_and(char::is_alphanumeric);
    text.match_indices(keyword)
        .any(|(i, m)| edge(text[..i].chars().next_back()) && edge(text[i + m.len()..].chars().next()))
}

/// Those of `lists` (an account's words) whose words `text` has, but the
/// author.
pub fn keyword_hits(text: &str, lists: &[(String, Vec<String>)], author: &str) -> Vec<String> {
    lists
        .iter()
        .filter(|(a, words)| a != author && words.iter().any(|w| has_keyword(text, w)))
        .map(|(a, _)| a.clone())
        .collect()
}

/// What a text notifies beyond the people it names: who `@channel` and
/// `@here` reach, and who a keyword alerts (a person already named isn't
/// listed twice). Refused (403) for a team's non-owner when its owners
/// limited the broadcast words.
pub(super) async fn extra(
    app: &Platform,
    facts: &Facts,
    room: &Room,
    text: &str,
    people: &[String],
    author: &str,
    named: &[String],
) -> Result<Extra, ApiError> {
    let b = Broadcast::of(&at_words(text));
    let mut out = Extra::default();
    if b.any() {
        if let Some(team) = room.scope.strip_prefix("team:")
            && app.db.chat_broadcast_owners(team)?
            && facts.role(team, author) != Some(TeamRole::Owner)
        {
            return Err(err(StatusCode::FORBIDDEN, "only this team's owners may use @channel and @here"));
        }
        let active = if b.here && !b.channel { active(app, room, people).await? } else { vec![] };
        out.broadcast = reach(b, people, &active, author);
    }
    let lists = app.db.chat_keywords_of(people)?;
    for a in keyword_hits(text, &lists, author) {
        if !named.contains(&a) && !out.broadcast.contains(&a) {
            out.keyword.push(a);
        }
    }
    Ok(out)
}

/// See [`extra`].
#[derive(Debug, Default)]
pub struct Extra {
    pub broadcast: Vec<String>,
    pub keyword: Vec<String>,
}

/// `GET /api/chat/keywords`.
pub async fn keywords(State(app): State<Arc<Platform>>, s: Person) -> J<ChatKeywords> {
    gate(&app, &s.account).await?;
    Ok(Json(ChatKeywords { words: app.db.chat_keywords(&s.account)? }))
}

/// A list of words as it's kept: trimmed, lowercased, no repeats.
pub fn clean_keywords(words: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let w = w.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        if w.is_empty() || out.contains(&w) {
            continue;
        }
        if w.chars().count() > KEYWORD_CHARS {
            return Err(err(StatusCode::BAD_REQUEST, "a keyword is at most 60 characters"));
        }
        out.push(w);
    }
    if out.len() > KEYWORDS_MAX {
        return Err(err(StatusCode::BAD_REQUEST, "at most 20 keywords"));
    }
    Ok(out)
}

/// `PUT /api/chat/keywords`: replace the list.
pub async fn set_keywords(State(app): State<Arc<Platform>>, s: Person, Json(b): Json<ChatKeywords>) -> J<ChatKeywords> {
    gate(&app, &s.account).await?;
    let words = clean_keywords(&b.words)?;
    app.db.set_chat_keywords(&s.account, &words)?;
    Ok(Json(ChatKeywords { words }))
}

/// A team the account is in (404 otherwise), and whether it owns it.
async fn team_role(app: &Platform, account: &str, team: &str) -> Result<bool, ApiError> {
    let teams = app.hub.teams(&[team.to_owned()]).await?;
    match teams.first().and_then(|t| t.role(account)) {
        Some(r) => Ok(r == TeamRole::Owner),
        None => Err(err(StatusCode::NOT_FOUND, "no such team")),
    }
}

/// `GET /api/chat/teams/{team}/settings`: for any member, the composer
/// reads it.
pub async fn team_settings(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
) -> J<ChatTeamSettings> {
    gate(&app, &s.account).await?;
    team_role(&app, &s.account, &team).await?;
    Ok(Json(ChatTeamSettings { broadcast_owners: app.db.chat_broadcast_owners(&team)? }))
}

/// `PUT /api/chat/teams/{team}/settings`: an owner's.
pub async fn set_team_settings(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
    Json(b): Json<ChatTeamSettings>,
) -> J<ChatTeamSettings> {
    gate(&app, &s.account).await?;
    if !team_role(&app, &s.account, &team).await? {
        return Err(err(StatusCode::FORBIDDEN, "only an owner can change this"));
    }
    app.db.set_chat_broadcast_owners(&team, b.broadcast_owners)?;
    Ok(Json(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_tests::{TestHub, call, hears, open_live, seed, serve};
    use serde_json::json;
    use std::time::Duration;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn channel_reaches_everyone_but_the_author_and_here_the_active() {
        let people = ids(&["a", "b", "c"]);
        let active = ids(&["a", "c"]);
        let words = |t: &str| Broadcast::of(&at_words(t));
        assert_eq!(reach(words("hi @channel"), &people, &active, "a"), ["b", "c"]);
        assert_eq!(reach(words("hi @here"), &people, &active, "a"), ["c"]);
        // Both: the wider one wins.
        assert_eq!(reach(words("@here @channel"), &people, &active, "b"), ["a", "c"]);
        assert!(reach(words("hi @bob"), &people, &active, "a").is_empty());
        assert!(!words("mail a@channel.example").any(), "an email isn't a mention");
    }

    #[test]
    fn keywords_match_whole_words_and_phrases_case_blind() {
        assert!(has_keyword("The Deploy failed on geek", "deploy failed"));
        assert!(has_keyword("deploy, now", "deploy"));
        assert!(!has_keyword("we redeployed", "deploy"));
        assert!(!has_keyword("deployments", "deploy"));
        assert!(has_keyword("(arugula) shipped", "arugula"));
        let lists = vec![
            ("b".to_owned(), ids(&["deploy failed"])),
            ("c".to_owned(), ids(&["ship"])),
            ("a".to_owned(), ids(&["deploy"])),
        ];
        assert_eq!(keyword_hits("deploy failed, ship it", &lists, "a"), ["b", "c"], "not the author");
    }

    #[test]
    fn keywords_are_kept_clean() {
        assert_eq!(
            clean_keywords(&ids(&["  Deploy   Failed ", "deploy failed", "", "X"])).unwrap(),
            ["deploy failed", "x"]
        );
        assert!(clean_keywords(&["x".repeat(KEYWORD_CHARS + 1)]).is_err());
        assert!(clean_keywords(&(0..=KEYWORDS_MAX).map(|i| format!("w{i}")).collect::<Vec<_>>()).is_err());
    }

    async fn post(base: &str, who: &str, text: &str) -> (u16, serde_json::Value) {
        call(base, who, "POST", "/api/chat/rooms/ch_general/messages", Some(json!({ "text": text }))).await
    }

    #[tokio::test]
    async fn channel_notifies_the_room_and_a_team_can_keep_it_to_owners() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        let (st, v) = post(&base, "a", "standup @channel").await;
        assert_eq!(
            (st, &v["mentions"], &v["pushed"]),
            (200, &json!(["b"]), &json!([{ "account": "b", "why": "mention" }])),
            "{v}"
        );
        assert_eq!(v["unreached"], json!([]), "the word was understood");
        let (_, v) = post(&base, "a", "@here quick one").await;
        assert_eq!(v["mentions"], json!(["b"]), "until presence, @here is @channel");

        // Only an owner sets the guard; any member reads it.
        let settings = "/api/chat/teams/t/settings";
        let (st, _) = call(&base, "b", "PUT", settings, Some(json!({ "broadcast_owners": true }))).await;
        assert_eq!(st, 403);
        let (st, _) = call(&base, "a", "PUT", settings, Some(json!({ "broadcast_owners": true }))).await;
        assert_eq!(st, 200);
        let (st, v) = call(&base, "b", "GET", settings, None).await;
        assert_eq!((st, &v["broadcast_owners"]), (200, &json!(true)));
        assert_eq!(call(&base, "x", "GET", settings, None).await.0, 403, "chat is off for x");

        let (st, _) = post(&base, "b", "@channel hello").await;
        assert_eq!(st, 403, "b isn't an owner");
        let (st, v) = post(&base, "b", "just a hello").await;
        assert_eq!((st, &v["mentions"]), (200, &json!([])));
        let (st, _) = post(&base, "a", "@channel again").await;
        assert_eq!(st, 200, "an owner may");
    }

    #[tokio::test]
    async fn a_keyword_notifies_as_a_mention_does() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        let (st, v) = call(&base, "b", "PUT", "/api/chat/keywords", Some(json!({ "words": ["Deploy Failed"] }))).await;
        assert_eq!((st, &v["words"]), (200, &json!(["deploy failed"])), "{v}");
        let (_, v) = post(&base, "a", "redeploy failed? no, the deploy failed").await;
        assert_eq!(v["mentions"], json!(["b"]), "{v}");
        assert_eq!(v["pushed"], json!([{ "account": "b", "why": "keyword" }]));
        // A name and a keyword in one message: one mention, as a mention.
        let (_, v) = post(&base, "a", "@b deploy failed").await;
        assert_eq!(v["mentions"], json!(["b"]));
        let (_, v) = post(&base, "a", "all quiet").await;
        assert_eq!(v["mentions"], json!([]));
        // Mine are mine: a's words don't show to b, and the list is per account.
        let (_, v) = call(&base, "a", "GET", "/api/chat/keywords", None).await;
        assert_eq!(v["words"], json!([]));
        // The room counts it as a mention for b.
        let (_, v) = call(&base, "b", "GET", "/api/chat", None).await;
        assert_eq!(v["rooms"][0]["mentions"], json!(2), "{v}");
    }

    #[tokio::test]
    async fn typing_reaches_others_with_the_room_open_and_is_never_replayed() {
        let hub = Arc::new(TestHub::default());
        let (app, base) = serve(hub.clone()).await;
        seed(&app, &hub);
        let (a, b) = (open_live(&base, "a", None).await, open_live(&base, "b", None).await);
        let stream = |h: &Arc<std::sync::Mutex<String>>| {
            let t = h.lock().unwrap().clone();
            let i = t.find("\"stream\":\"").unwrap() + 10;
            t[i..].split('"').next().unwrap().to_owned()
        };
        hears(&a, "\"type\":\"hello\"", Duration::from_secs(5)).await;
        hears(&b, "\"type\":\"hello\"", Duration::from_secs(5)).await;
        let typing = "/api/chat/rooms/ch_general/typing";
        assert_eq!(call(&base, "a", "POST", typing, None).await.0, 204);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!b.lock().unwrap().contains("typing"), "b has nothing open");
        for (who, h) in [("a", &a), ("b", &b)] {
            let (st, _) = call(
                &base,
                who,
                "POST",
                "/api/chat/live/open",
                Some(json!({ "stream": stream(h), "room": "ch_general" })),
            )
            .await;
            assert_eq!(st, 200);
        }
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(call(&base, "a", "POST", typing, None).await.0, 204);
        hears(&b, "\"type\":\"typing\",\"room\":\"ch_general\",\"account\":\"a\"", Duration::from_secs(2)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!a.lock().unwrap().contains("typing"), "not to the typist");
        assert_eq!(call(&base, "x", "POST", typing, None).await.0, 403, "chat is off for x");
        // Kept for the platform's other machines, as `typing` rows, and not
        // sent again to a stream that resumes.
        let rows = app.db.chat_events_after(0, 100).unwrap();
        assert!(rows.iter().filter(|e| e.body.contains("typing")).all(|e| e.kind == "typing"));
        assert_eq!(rows.iter().filter(|e| e.kind == "typing").count(), 2);
        assert!(app.chat.missed(&app.db, "c-0").is_none_or(|(_, outs)| outs.is_empty()));
    }
}
