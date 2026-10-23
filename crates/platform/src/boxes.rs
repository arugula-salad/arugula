//! hud boxes as clients of the platform (M237, platform step 8: "hud is an
//! in-app client: platform assertions, a project roster"). A box an
//! operator bound to a project (the hub's `issuer_clients.project`) asks
//! the hub, with its client secret, for two things, and the hub asks the
//! platform through `PlatformPort`:
//! - **its roster:** the project's members now ([`members`]): a personal
//!   project's account, or its team's roster (a locked team's owners
//!   only), with each one's role and roster name. The hub adds their
//!   numeric GitHub ids, which hud matches people on.
//! - **a review thread:** a comment-mode review posted into the thread on
//!   an object in that project ([`post`]), for its author, who has to be a
//!   member who may post there now, as when they post themselves. The
//!   message keeps the box's name beside its author (`via`), so the page
//!   says it came through the box.
//!
//! Nothing else: the box can't read the project's threads, name another
//! project's object, or post for someone outside it. An object outside the
//! box's project answers as one that isn't there.

use arugula_control_common::ApiError;
use arugula_control_wire::chat::ChatPost;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{Platform, chat};

/// A member of a box's project (M237), as the platform knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxMember {
    pub account: String,
    /// `owner` for a personal project's account; else their team role
    /// (`owner`, `editor`, `viewer`, ...).
    pub role: String,
    /// What the roster calls them (a personal project: the account's name).
    pub name: String,
}

/// A review a box posts into an object's thread (M237).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxPost {
    /// The project the box is bound to: the object has to be in it.
    pub project: String,
    /// The box's name, kept beside the author.
    pub via: String,
    /// The object's global id (`iss_…`, `fi_…`, `hw_…`, `run_…`).
    pub target: String,
    /// Whose review it is: a member who signed in to the box.
    pub author: String,
    pub text: String,
}

/// What became of a [`BoxPost`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoxPosted {
    /// In the thread `room` as message `seq`.
    Posted { room: String, seq: i64 },
    /// Refused, as the author posting themselves would be: the status and
    /// why.
    Refused { status: u16, error: String },
}

impl From<ApiError> for BoxPosted {
    fn from(e: ApiError) -> Self {
        BoxPosted::Refused { status: e.0.as_u16(), error: e.1 }
    }
}

/// A box's project's members now, or `None` when there's no such project.
pub async fn members(app: &Platform, project: &str) -> anyhow::Result<Option<Vec<BoxMember>>> {
    let Some(owner) = app.db.project_owner(project)? else { return Ok(None) };
    if let Some(account) = owner.account {
        let names = app.cache.accounts(&*app.hub, std::slice::from_ref(&account)).await?;
        let Some(a) = names.get(&account) else { return Ok(Some(vec![])) };
        return Ok(Some(vec![BoxMember { account, role: "owner".into(), name: a.name.clone() }]));
    }
    let Some(team) = owner.team else { return Ok(Some(vec![])) };
    let found = match app.cache.team(&*app.hub, &team).await? {
        Some(t) => Some(t),
        None => app.cache.team_now(&*app.hub, &team).await?,
    };
    let Some(t) = found else { return Ok(Some(vec![])) };
    Ok(Some(
        t.members
            .iter()
            .filter_map(|m| {
                // A locked team's other members are as good as out.
                let role = t.role(&m.account)?;
                Some(BoxMember { account: m.account.clone(), role: role.as_str().into(), name: m.name.clone() })
            })
            .collect(),
    ))
}

/// Post a box's review into the thread on `p.target` for `p.author`.
pub async fn post(app: &Platform, p: &BoxPost) -> anyhow::Result<BoxPosted> {
    if p.text.trim().is_empty() {
        return Ok(BoxPosted::Refused { status: 400, error: "say something".into() });
    }
    let room = match chat::box_object_room(app, &p.author, &p.project, &p.target).await {
        Ok(r) => r,
        Err(e) if e.0 == StatusCode::INTERNAL_SERVER_ERROR => anyhow::bail!("{}", e.1),
        Err(e) => return Ok(e.into()),
    };
    let b = ChatPost { text: p.text.clone(), pane: None, quote: None };
    match chat::post_full(app, &p.author, None, Some(&p.via), &room, b).await {
        Ok(posted) => Ok(BoxPosted::Posted { room, seq: posted.seq }),
        Err(e) if e.0 == StatusCode::INTERNAL_SERVER_ERROR => anyhow::bail!("{}", e.1),
        Err(e) => Ok(e.into()),
    }
}
