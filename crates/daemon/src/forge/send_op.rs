//! The `forge.send` op (M168, #1214): control asks this machine to send a
//! draft on a linked repo because its person pressed Send and no browser
//! hop could ("Sending as a person uses a one-use user token, falling back
//! to their machine"). It goes out with the owner's own `gh` login, never
//! control's App, so it's theirs on GitHub, as a block's send is.
//!
//! Only the machine's owner's draft: another person's is refused, for
//! control to ask one of theirs. No `gh` login (or none here) is
//! [`OpAnswer::Cant`], so control asks the next machine; GitHub refusing it
//! is [`OpAnswer::Failed`], for the person to see. A block open on the item
//! logs the send as its own writes are logged, and reads again.

use std::sync::Arc;

use arugula_control_wire::forge::{ForgeSend, OpAnswer};
use arugula_proto::forge::{ReviewEvent, Write};
use tracing::{info, warn};

use super::{
    Error, Sent,
    github::{self, GhToken},
    live,
};
use crate::review::Runner;

/// Who the block's log says sent it.
const BY: &str = "Arugula control, for its person";

/// A comment or review as the block's write, for its log.
fn write_of(s: &ForgeSend) -> Option<Write> {
    match s.kind.as_str() {
        "comment" => Some(Write::Comment { body: s.body.clone() }),
        "review" => Some(Write::Review {
            event: review_event(s.event.as_deref()),
            body: Some(s.body.clone()).filter(|b| !b.trim().is_empty()),
        }),
        _ => None,
    }
}

fn review_event(e: Option<&str>) -> ReviewEvent {
    match e {
        Some("APPROVE") => ReviewEvent::Approve,
        Some("REQUEST_CHANGES") => ReviewEvent::RequestChanges,
        _ => ReviewEvent::Comment,
    }
}

/// Do it: module docs.
pub async fn forge_send(app: &Arc<crate::server::App>, s: ForgeSend) -> OpAnswer {
    let owner = live::control().and_then(|c| c.enrolled()).map(|e| e.saved.cert.account.clone());
    if owner.as_deref() != Some(s.account.as_str()) {
        return OpAnswer::Cant { why: "this machine isn't the draft's person's".into() };
    }
    if s.repo.split('/').count() != 2 {
        return OpAnswer::Failed { error: format!("{} isn't OWNER/NAME", s.repo) };
    }
    let runner = match crate::api::agent_env(app).await {
        Ok((home, env)) => Runner::Local { env, home },
        Err(e) => return OpAnswer::Cant { why: e.1 },
    };
    // Their own gh login, and nothing else: no App to fall back to.
    let gh = github::client(&github::api_for("github.com"), GhToken::new(runner, "github.com"));
    let (repo, n) = (s.repo.as_str(), s.number.unwrap_or(0));
    let r: Result<Sent, Error> = match (s.kind.as_str(), s.number) {
        ("comment", Some(_)) => gh.comment(repo, n, &s.body).await,
        ("review", Some(_)) => {
            let body = Some(s.body.as_str()).filter(|b| !b.trim().is_empty());
            gh.review(repo, n, review_event(s.event.as_deref()), body).await
        }
        ("issue", None) => gh.new_issue(repo, s.title.as_deref().unwrap_or(""), &s.body).await.map(|(_, sent)| sent),
        _ => return OpAnswer::Failed { error: format!("can't send a {} draft like that", s.kind) },
    };
    if let (Some(w), Some(b)) = (write_of(&s), live::block_on(repo, n)) {
        let logged = r.as_ref().map(|x| Sent { url: x.url.clone(), said: x.said.clone() }).map_err(ToString::to_string);
        b.sent_elsewhere(&w, BY, None, &logged);
    }
    match r {
        Ok(sent) => {
            info!(
                draft = s.draft,
                repo,
                number = s.number,
                kind = s.kind,
                url = sent.url,
                "a draft sent for its person, as control asked"
            );
            OpAnswer::Done { url: sent.url }
        }
        Err(Error::Login(why)) => {
            warn!(draft = s.draft, why, "can't send a draft for control: no gh login");
            OpAnswer::Cant { why }
        }
        Err(e) => {
            warn!(draft = s.draft, error = %e, "a draft control asked for wasn't sent");
            OpAnswer::Failed { error: e.to_string() }
        }
    }
}
