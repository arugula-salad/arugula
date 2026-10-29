//! Chat and agent sessions (#314, `arugula-platform-chat`): a team's
//! channels and DMs, the threads on panes, sessions and objects
//! ([`chat`], M122, M138), and the agent sessions that live in those
//! threads ([`sessions`], [`session_live`], M256). They're one crate
//! because they're one cycle: a session is posted, carded and answered in
//! its thread, and a thread's `@arugula` starts one.
//!
//! It sits on the platform's base, whose modules it names as the platform
//! did before the split (`crate::db`, `crate::forge`, ...: the glob
//! below). Chat's core stays in the base (`arugula_platform::chat`): the
//! live bus on `Platform::chat`, whether chat is on, names and mentions, so
//! issues, boards, the inbox and the hub's port tell a thread something
//! without this crate. The rest of the platform reaches this crate through
//! [`features::Chat`]: the hub's port for a session's PR and a hosted
//! box's session calls, the forge mirror and GitHub's webhook for what a
//! session hears, hosted workspaces for a rebuilt box, boxes for a review,
//! grants for a room's notice, and agents with no machine for their
//! threads.

use std::sync::Arc;

use arugula_control_common::ApiError;
use arugula_control_wire::{
    chat::{
        AgentMessages, AgentMessagesQuery, AgentRooms, ChatFound, ChatMessage, ChatPost, ChatPosted, ChatReact,
        ChatReactQuery, ChatSearchQuery,
    },
    work::{PushAsk, PushGiven, Reported, SessionIssuesAsk, SessionIssuesGiven, SessionReport},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    routing::{get, post},
};
use futures_util::future::BoxFuture;
use serde_json::Value;

#[allow(unused_imports)]
use arugula_platform::*;

#[cfg(test)]
mod agent_grants_tests;
#[cfg(test)]
mod agent_issues_tests;
#[cfg(test)]
mod agent_sessions_tests;
#[cfg(test)]
mod agent_threads_tests;
#[cfg(test)]
mod boards_tests;
#[cfg(test)]
mod boxes_tests;
pub mod chat;
#[cfg(test)]
mod chat_files_tests;
#[cfg(test)]
mod chat_people_tests;
#[cfg(test)]
mod chat_pins_tests;
#[cfg(test)]
mod chat_previews_tests;
#[cfg(test)]
mod chat_reactions_tests;
#[cfg(test)]
mod chat_rooms_tests;
#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod chat_threads_tests;
#[cfg(test)]
mod forge_mentions_tests;
#[cfg(test)]
mod forge_viewer_tests;
#[cfg(test)]
mod hosted_wake_tests;
#[cfg(test)]
mod hub_calls_tests;
#[cfg(test)]
mod issues_tests;
#[cfg(test)]
mod notify_tests;
#[cfg(test)]
mod object_threads_tests;
#[cfg(test)]
mod restart_tests;
#[cfg(test)]
mod runs_page_tests;
#[cfg(test)]
mod session_feedback_tests;
#[cfg(test)]
mod session_issues_tests;
pub mod session_live;
#[cfg(test)]
mod session_merged_tests;
#[cfg(test)]
mod session_pr_tests;
#[cfg(test)]
mod session_wave_tests;
pub mod sessions;
#[cfg(test)]
mod sessions_hydrate_tests;
#[cfg(test)]
mod sessions_pool_tests;
#[cfg(test)]
mod sessions_tries_tests;

struct Feature;

impl features::Feature for Feature {
    fn routes(&self, r: features::Routes) -> features::Routes {
        r
            // Chat (M122, M138), and agents' threads (M198).
            .route("/api/chat", get(chat::rooms))
            .route("/api/chat/live", get(chat::live))
            .route("/api/chat/live/open", post(chat::open))
            .route("/api/chat/search", get(chat::search))
            .route("/api/chat/presence", post(chat::people::beat))
            .route("/api/chat/avatar/{account}", get(chat::people::avatar))
            .route("/api/chat/sidebar", get(chat::sidebar::get).put(chat::sidebar::put))
            .route("/api/chat/targets", post(chat::open_target))
            .route("/api/chat/teams/{team}/channels", get(chat::members::channels).post(chat::new_channel))
            .route("/api/chat/teams/{team}/dms", post(chat::new_dm))
            .route("/api/chat/rooms/{id}", get(chat::room).patch(chat::change_room))
            .route("/api/chat/rooms/{id}/messages", get(chat::messages).post(chat::post))
            .route("/api/chat/rooms/{id}/messages/{seq}", axum::routing::patch(chat::edit).delete(chat::delete))
            .route(
                "/api/chat/rooms/{id}/files",
                post(chat::files::upload)
                    .layer(axum::extract::DefaultBodyLimit::max(arugula_control_wire::chat::MAX_FILE_BYTES + 1)),
            )
            .route("/api/chat/rooms/{id}/files/{file}", get(chat::files::get))
            .route("/api/chat/rooms/{id}/previews", get(chat::previews::previews))
            .route("/api/chat/rooms/{id}/messages/{seq}/previews", post(chat::previews::hide))
            .route("/api/chat/previews/image", get(chat::previews::image))
            .route("/api/chat/teams/{team}/previews", get(chat::previews::team_get).put(chat::previews::team_put))
            .route(
                "/api/chat/rooms/{id}/messages/{seq}/reactions",
                post(chat::reactions::add).delete(chat::reactions::remove),
            )
            .route("/api/chat/rooms/{id}/topic", axum::routing::put(chat::pins::set_topic))
            .route("/api/chat/rooms/{id}/pins", get(chat::pins::list))
            .route(
                "/api/chat/rooms/{id}/messages/{seq}/pin",
                axum::routing::put(chat::pins::pin).delete(chat::pins::unpin),
            )
            .route("/api/chat/rooms/{id}/join", post(chat::members::join))
            .route("/api/chat/rooms/{id}/leave", post(chat::members::leave))
            .route("/api/chat/rooms/{id}/members", post(chat::members::add))
            .route("/api/chat/rooms/{id}/members/{account}", axum::routing::delete(chat::members::remove))
            .route("/api/chat/rooms/{id}/read", post(chat::read))
            .route("/api/chat/threads", get(chat::threads::followed))
            .route("/api/chat/rooms/{id}/threads/{seq}/read", post(chat::threads::read))
            .route("/api/chat/rooms/{id}/threads/{seq}/follow", post(chat::threads::follow))
            .route("/api/chat/rooms/{id}/mute", post(chat::mute))
            .route("/api/chat/rooms/{id}/unread", post(chat::saved::unread))
            .route("/api/chat/saved", get(chat::saved::list))
            .route(
                "/api/chat/rooms/{id}/messages/{seq}/saved",
                axum::routing::put(chat::saved::save).patch(chat::saved::done).delete(chat::saved::unsave),
            )
            .route("/api/chat/rooms/{id}/typing", post(chat::typing::typing))
            .route("/api/chat/keywords", get(chat::alerts::keywords).put(chat::alerts::set_keywords))
            .route(
                "/api/chat/teams/{team}/settings",
                get(chat::alerts::team_settings).put(chat::alerts::set_team_settings),
            )
            .route("/api/chat/rooms/{id}/level", post(chat::level))
            .route(arugula_control_wire::chat::CHAT_READERS, post(chat::daemon_readers))
            .route(arugula_control_wire::chat::AGENT_ROOMS, get(chat::agent_threads::rooms))
            .route(
                "/api/daemon/chat/rooms/{id}/messages",
                get(chat::agent_threads::messages).post(chat::agent_threads::post),
            )
            .route(
                "/api/daemon/chat/rooms/{id}/files",
                post(chat::agent_threads::upload)
                    .layer(axum::extract::DefaultBodyLimit::max(arugula_control_wire::chat::MAX_FILE_BYTES + 1)),
            )
            .route("/api/daemon/chat/rooms/{id}/files/{file}", get(chat::agent_threads::download))
            .route(
                "/api/daemon/chat/rooms/{id}/messages/{seq}/reactions",
                post(chat::agent_threads::react).delete(chat::agent_threads::unreact),
            )
            .route(
                "/api/daemon/chat/rooms/{id}/messages/{seq}/pin",
                axum::routing::put(chat::agent_threads::pin).delete(chat::agent_threads::unpin),
            )
            .route(arugula_control_wire::chat::AGENT_SEARCH, get(chat::agent_threads::search))
            // An agent session's card and page (M257).
            .route(arugula_control_wire::sessions::SESSION, get(crate::session_live::show))
            .route(arugula_control_wire::sessions::SESSION_STOP, post(crate::session_live::stop))
            .route(arugula_control_wire::sessions::SESSION_ACTIONS, post(crate::session_live::act))
            // Its pull request, which its asker drafts and sends (M259).
            .route(
                arugula_control_wire::sessions::SESSION_PR,
                get(crate::sessions::pr::show).put(crate::sessions::pr::save),
            )
            .route(arugula_control_wire::sessions::SESSION_PR_SEND, post(crate::sessions::pr::route))
            // Try 3 and its pick (M264).
            .route(arugula_control_wire::sessions::SESSION_TRY, post(crate::sessions::tries::start))
            .route(arugula_control_wire::sessions::SESSION_PICK, post(crate::sessions::tries::pick))
            .route(arugula_control_wire::sessions::PROJECT_SESSION_STATS, get(crate::sessions::merged::stats))
            // Which model its agents use (Arugula #215).
            .route(
                arugula_control_wire::sessions::PROJECT_SESSION_SETTINGS,
                get(crate::sessions::model::show).put(crate::sessions::model::set),
            )
            .route(arugula_control_wire::sessions::ISSUE_AGENT, post(crate::chat::sessions::issue_agent))
            .route(arugula_control_wire::sessions::SESSION_MOVE, post(crate::sessions::moves::route))
            // A person's agent starts and replies to sessions, by grant (aru-121).
            .route(arugula_control_wire::sessions::AGENT_SESSIONS, post(crate::chat::agent_sessions::start))
            .route(arugula_control_wire::sessions::AGENT_SESSION_REPLY, post(crate::chat::agent_sessions::reply))
            // A wave of sessions, a session's whole answer, and a wait (Fleet 5).
            .route(arugula_control_wire::sessions::PROJECT_SESSIONS, get(crate::sessions::wave::list))
            .route(arugula_control_wire::sessions::AGENT_PROJECT_SESSIONS, get(crate::sessions::wave::agent_list))
            .route(arugula_control_wire::sessions::AGENT_SESSIONS_WAIT, post(crate::sessions::wave::agent_wait))
            .route(arugula_control_wire::sessions::AGENT_SESSION, get(crate::sessions::wave::agent_read))
            .route(
                arugula_control_wire::sessions::ASK,
                get(crate::chat::sessions::ask_show).post(crate::chat::sessions::ask_answer),
            )
    }

    fn start(&self, p: &Arc<Platform>) {
        // Saved messages' reminders (the base follows the hub's change
        // feed and the live bus: `arugula_platform::chat::start`).
        chat::start_reminders(p);
    }

    fn jobs(&self, p: &Arc<Platform>, w: jobs::Worker) -> jobs::Worker {
        // Thread sessions' follow (M256).
        w.with(sessions::job::Follow { app: p.clone() })
    }
}

struct Hooks;

impl features::Chat for Hooks {
    fn is_pr(&self, draft: &str) -> bool {
        draft.starts_with(sessions::pr::PREFIX)
    }

    fn send_pr<'a>(
        &'a self,
        app: &'a Platform,
        account: &'a str,
        draft: &'a str,
        how: forge::send::How,
    ) -> BoxFuture<'a, forge::send::Outcome> {
        Box::pin(async move {
            match sessions::pr::send(app, account, draft, how).await {
                Ok(s) => forge::send::Outcome { pr: s.pr.url, ..Default::default() },
                Err(e) => forge::send::Outcome { refused: Some(e.1), ..Default::default() },
            }
        })
    }

    fn session_report<'a>(
        &'a self,
        app: &'a Platform,
        m: &'a work::HostedMachine,
        report: &'a SessionReport,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        Box::pin(session_live::report(app, m, report))
    }

    fn session_push<'a>(
        &'a self,
        app: &'a Platform,
        m: &'a work::HostedMachine,
        ask: &'a PushAsk,
    ) -> BoxFuture<'a, anyhow::Result<PushGiven>> {
        Box::pin(sessions::push::token(app, m, ask))
    }

    fn session_issues<'a>(
        &'a self,
        app: &'a Platform,
        m: &'a work::HostedMachine,
        ask: &'a SessionIssuesAsk,
    ) -> BoxFuture<'a, anyhow::Result<SessionIssuesGiven>> {
        Box::pin(sessions::issues::answer(app, m, ask))
    }

    fn pr_noted(&self, app: &Platform, repo: u64) {
        sessions::pr::noted(app, repo);
    }

    fn merged_seen(&self, app: &Platform, repo: u64, items: &[&db::mirror::ItemRow]) {
        sessions::merged::seen(app, repo, items);
    }

    fn after_rebuild<'a>(
        &'a self,
        app: &'a Platform,
        s: &'a db::sessions::Session,
    ) -> BoxFuture<'a, anyhow::Result<features::Rebuilt>> {
        Box::pin(sessions::hydrate::after_rebuild(app, s))
    }

    fn told(&self, app: &Platform, ids: &[String]) -> anyhow::Result<()> {
        session_live::told(app, ids)
    }

    fn suite_done(&self, app: &Platform, event: &str, v: &Value, now: u64) -> anyhow::Result<()> {
        sessions::feedback::suite_done(app, event, v, now)
    }

    fn route_feedback(
        &self,
        app: &Platform,
        seen: &triggers::events::Seen,
        v: &Value,
        now: u64,
    ) -> anyhow::Result<features::Routed> {
        sessions::feedback::route(app, seen, v, now)
    }

    fn box_post<'a>(
        &'a self,
        app: &'a Platform,
        author: &'a str,
        project: &'a str,
        target: &'a str,
        via: &'a str,
        text: String,
    ) -> BoxFuture<'a, Result<(String, i64), ApiError>> {
        Box::pin(async move {
            let room = chat::box_object_room(app, author, project, target).await?;
            let b = ChatPost { text, ..Default::default() };
            let posted = chat::post_full(app, author, None, Some(via), &room, b).await?;
            Ok((room, posted.seq))
        })
    }

    fn grant_notice<'a>(
        &'a self,
        app: &'a Platform,
        power: &'a str,
        scope: (&'a str, &'a str),
    ) -> BoxFuture<'a, Option<features::GrantNotice>> {
        Box::pin(async move {
            // A grant that lets agents into a room says so there (#122).
            let rooms = if power == db::projects::THREADS_READ || power == db::projects::THREADS_POST {
                chat::agent_notice::rooms(app, scope).unwrap_or_default()
            } else {
                vec![]
            };
            let before = chat::agent_notice::reach(app, &rooms).await.ok()?;
            Some(features::GrantNotice(Box::new((rooms, before))))
        })
    }

    fn announce<'a>(&'a self, app: &'a Platform, notice: features::GrantNotice) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Ok(n) = notice.0.downcast::<(Vec<db::chat::Room>, chat::agent_notice::Reach)>() {
                let (rooms, before) = *n;
                chat::agent_notice::announce(app, &rooms, &before).await;
            }
        })
    }

    fn agent_rooms(&self, app: Arc<Platform>, me: auth::Machine) -> BoxFuture<'static, Result<AgentRooms, ApiError>> {
        Box::pin(async move { Ok(chat::agent_threads::rooms(State(app), me).await?.0) })
    }

    fn agent_messages(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        room: String,
        q: AgentMessagesQuery,
    ) -> BoxFuture<'static, Result<AgentMessages, ApiError>> {
        Box::pin(async move { Ok(chat::agent_threads::messages(State(app), me, Path(room), Query(q)).await?.0) })
    }

    fn agent_post(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        room: String,
        b: ChatPost,
    ) -> BoxFuture<'static, Result<ChatPosted, ApiError>> {
        Box::pin(async move { Ok(chat::agent_threads::post(State(app), me, Path(room), Json(b)).await?.0) })
    }

    fn agent_react(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        room: String,
        seq: i64,
        emoji: String,
        on: bool,
    ) -> BoxFuture<'static, Result<ChatMessage, ApiError>> {
        Box::pin(async move {
            let path = Path((room, seq));
            Ok(if on {
                chat::agent_threads::react(State(app), me, path, Json(ChatReact { emoji })).await?.0
            } else {
                chat::agent_threads::unreact(State(app), me, path, Query(ChatReactQuery { emoji })).await?.0
            })
        })
    }

    fn agent_pin(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        room: String,
        seq: i64,
        on: bool,
    ) -> BoxFuture<'static, Result<ChatMessage, ApiError>> {
        Box::pin(async move {
            let path = Path((room, seq));
            Ok(if on {
                chat::agent_threads::pin(State(app), me, path).await?.0
            } else {
                chat::agent_threads::unpin(State(app), me, path).await?.0
            })
        })
    }

    fn agent_search(
        &self,
        app: Arc<Platform>,
        me: auth::Machine,
        q: ChatSearchQuery,
    ) -> BoxFuture<'static, Result<ChatFound, ApiError>> {
        Box::pin(async move { Ok(chat::agent_threads::search(State(app), me, Query(q)).await?.0) })
    }
}

/// Puts chat and its sessions in this platform: the binaries do, before
/// it's made (`arugula-platform-all`). Once.
pub fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        features::register(Box::new(Feature));
        features::install_chat(Box::new(Hooks));
    });
}

/// The base's test harness, with this crate registered before a router is
/// made: what the tests here call as `crate::testing` and, through
/// `chat_tests`, `crate::chat_tests`.
#[cfg(test)]
mod testing {
    pub use arugula_platform::testing::*;

    pub fn router(app: std::sync::Arc<crate::Platform>) -> axum::Router {
        crate::register();
        arugula_platform::testing::router(app)
    }

    pub mod hub {
        use std::sync::Arc;

        pub use arugula_platform::testing::hub::*;

        use crate::Platform;

        pub async fn serve(hub: Arc<TestHub>) -> (Arc<Platform>, String) {
            crate::register();
            arugula_platform::testing::hub::serve(hub).await
        }

        pub async fn serve_on(hub: Arc<TestHub>, db: crate::db::Db) -> (Arc<Platform>, String) {
            crate::register();
            arugula_platform::testing::hub::serve_on(hub, db).await
        }
    }

    /// The base's platform on a fake GitHub (`forge::links_tests::serve`),
    /// with this crate registered.
    pub async fn forge_serve(
        hub: &std::sync::Arc<TestHub>,
    ) -> (arugula_platform::forge::links_tests::Shared, std::sync::Arc<crate::Platform>, String) {
        crate::register();
        arugula_platform::forge::links_tests::serve(hub).await
    }
}

/// The base's agents rig, with this crate registered before its router is
/// made: what the tests here call as `crate::agents_tests`.
#[cfg(test)]
mod agents_tests {
    pub use arugula_platform::agents_tests::*;

    pub async fn seeded()
    -> (std::sync::Arc<crate::chat_tests::TestHub>, std::sync::Arc<crate::Platform>, String, String, String) {
        crate::register();
        arugula_platform::agents_tests::seeded().await
    }
}
