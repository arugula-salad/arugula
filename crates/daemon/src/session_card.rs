//! Putting a session on a card from the project's Sessions page (Arugula
//! #1019): control asks `sessions.set_card` over the relay
//! ([`SessionCardAsk`]), for the machine's owner, and the session takes it
//! as work set by hand (`Intent::SetSessionWork`), so a branch never
//! moves it after. Every pane with no card of its own follows, as the
//! session's card is theirs. It's kept in the layout, so it survives a
//! restart, and the next sessions report tells control
//! ([`machine_sessions`](crate::machine_sessions)).
//!
//! `card: null` takes the session back to its branch's card
//! (`Mux::unset_work`): its work goes, and its terminals' branches are
//! read again (`board_attach::infer_card`), so one on `feat/N-…` is on
//! `#N` again, and any other is on no card.
//!
//! An older control never asks: then nothing here runs, and a session's
//! card is its branch's, or the one set from its pane's chip, as before.

use arugula_control_wire::forge::{OpAnswer, SessionCardAsk};
use arugula_proto::{BlockType, SessionId};
use tracing::debug;

use crate::{mux::Api, server::App};

/// What an ask names: the session, and the card as the layout keeps it
/// (`#N` or `iss_…`; `None` for the branch's), or why it names none.
pub(crate) fn asked(ask: &SessionCardAsk) -> Result<(SessionId, Option<String>), String> {
    let session = ask.session.trim().parse().map_err(|_| format!("no session {:?} on this machine", ask.session))?;
    if crate::machine_sessions::project_id(&ask.project).is_none() {
        return Err(format!("a card is in a project (prj_…), not {:?}", ask.project));
    }
    let card = match ask.card.as_deref() {
        None => None,
        Some(c) => Some(crate::mcp::issues::native(c).map(|_| c.trim().to_owned())?),
    };
    Ok((session, card))
}

/// `sessions.set_card`: `done` once the session is on the card (or back
/// on its branch's), `failed` with why not.
pub async fn set(app: &App, ask: &SessionCardAsk) -> OpAnswer {
    match put(app, ask).await {
        Ok(()) => OpAnswer::Done { url: None, routed: None },
        Err(error) => OpAnswer::Failed { error },
    }
}

async fn put(app: &App, ask: &SessionCardAsk) -> Result<(), String> {
    let (session, card) = asked(ask)?;
    let gone = || "the daemon is shutting down".to_owned();
    let Some(card) = card else {
        app.mux.api(|r| Api::UnsetWork(session, r)).await.ok_or_else(gone)??;
        again_from_branches(app, session).await;
        return Ok(());
    };
    let item = number_of(app, &ask.project, card).await?;
    let work = arugula_core::WorkRef { project: ask.project.clone(), item: Some(item) };
    let name = ask.project_name.clone().filter(|n| !n.trim().is_empty());
    app.mux.api(|r| Api::SetSessionWork(session, Some(work), name, r)).await.ok_or_else(gone)??;
    debug!(session, project = ask.project, "a session put on a card from control");
    Ok(())
}

/// An `iss_…` card as `#N`, the way a branch names it, when control says
/// it's in `project`; one in another project isn't this one's card. A
/// lookup that fails keeps the id, which is a card as well.
async fn number_of(app: &App, project: &str, card: String) -> Result<String, String> {
    if !card.starts_with("iss_") {
        return Ok(card);
    }
    match crate::board_attach::issue_project(app, &card, Some(project)).await {
        Ok(Some((p, n))) if p == project => Ok(n),
        Ok(Some((p, _))) => Err(format!("{card} is in {p}, not {project}")),
        Ok(None) | Err(_) => Ok(card),
    }
}

/// Read the session's terminals' branches again, as when they opened, so
/// a `feat/N-…` one gives it `#N` again.
async fn again_from_branches(app: &App, session: SessionId) {
    let Some(panes) = app.mux.api(Api::Panes).await else { return };
    for p in panes.into_iter().filter(|p| p.session == session && p.info.host.is_none()) {
        if p.info.kind != BlockType::Terminal {
            continue;
        }
        let Some(branch) = p.info.branch else { continue };
        let project = p.info.session_project.map(|s| s.id);
        if crate::board_attach::infer_card(app, session, project, &branch, p.info.id).await {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(session: &str, project: &str, card: Option<&str>) -> SessionCardAsk {
        SessionCardAsk {
            session: session.into(),
            project: project.into(),
            project_name: None,
            card: card.map(Into::into),
        }
    }

    /// A card is a native issue in a project; `null` is the branch's.
    #[test]
    fn an_ask_names_a_session_and_a_native_card_or_none() {
        assert_eq!(asked(&ask("3", "prj_1", Some("#7"))), Ok((3, Some("#7".into()))));
        let iss = format!("iss_{}", "a".repeat(32));
        assert_eq!(asked(&ask("3", "prj_1", Some(&iss))), Ok((3, Some(iss))));
        assert_eq!(asked(&ask("3", "prj_1", None)), Ok((3, None)));
        assert!(asked(&ask("s3", "prj_1", Some("#7"))).is_err(), "not a session's id");
        assert!(asked(&ask("3", "personal", Some("#7"))).is_err(), "not a project");
        assert!(asked(&ask("3", "prj_1", Some("acme/app#7"))).is_err(), "a GitHub issue isn't a card");
    }
}
