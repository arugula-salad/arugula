//! Asks (Arugula #367): a question on a board's row that a person answers
//! on the board. A person or an agent asks (`boards.write`), and names who
//! it is for, an account of the project. Only that person answers, or an
//! owner or editor of the project for them; a machine and its agents never
//! do, so [`answer`] takes an account and its route a [`Person`].
//!
//! A row has one open ask at most, and only a row with an issue or a PR as
//! its item can have one. Answering closes it and keeps it, with who gave the
//! answer and when, as the last few of the row's `asked`.
//!
//! The board also **raises** asks itself ([`Asks::row`]), worked out on every
//! read and never stored: a PR whose checks failed, or an agent block that
//! is waiting for input. They come from the chips and block states the
//! reader already may see, so a reader who can't read a PR isn't told about
//! it, and they clear with their cause. An asked question beats a raised one.
//! (A drafted merge isn't one of them yet: the platform keeps no merge
//! drafts, `forge::drafts` takes comment, review and issue.)

use std::{collections::HashMap, sync::Arc};

use arugula_control_wire::boards as wire;
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::{J, bad, board_place, changed, one_line, read, refused, text_ok};
use crate::{
    Platform,
    auth::Person,
    db::{
        boards_asks::{AskRow, NewAsk},
        issues::Actor,
    },
    grants::agent_names,
};
use arugula_control_common::{ApiError, err};

/// The reader's role in `project` now: none for a stranger.
pub(super) async fn role_in(app: &Platform, project: &str, account: &str) -> Result<Option<TeamRole>, ApiError> {
    let Some(p) = app.db.issue_project(project)? else { return Ok(None) };
    Ok(match (&p.account, &p.team) {
        (Some(a), _) => (a == account).then_some(TeamRole::Owner),
        (None, Some(t)) => app.cache.team(&*app.hub, t).await?.and_then(|t| t.role(account)),
        _ => None,
    })
}

/// A line of text for a question or an option: trimmed, 1 to `max`
/// characters, on one line.
fn line_ok<'a>(s: &'a str, max: usize, what: &str) -> Result<&'a str, ApiError> {
    let s = s.trim();
    if s.is_empty() || !one_line(s) || s.chars().count() > max {
        return Err(bad(&format!("{what} is 1 to {max} characters, on one line")));
    }
    Ok(s)
}

/// A new ask checked: its question, why, options and recommendation.
struct Checked<'a> {
    question: &'a str,
    why: &'a str,
    options: Vec<String>,
    recommend: Option<&'a str>,
}

fn checked(n: &wire::NewAsk) -> Result<Checked<'_>, ApiError> {
    let question = line_ok(&n.question, wire::ASK_MAX, "a question")?;
    let why = n.why.as_deref().map(|w| text_ok(w, "a why")).transpose()?.unwrap_or("");
    if n.options.len() > wire::ASK_OPTIONS_MAX {
        return Err(bad(&format!("an ask has at most {} options, or none for free text", wire::ASK_OPTIONS_MAX)));
    }
    let mut options: Vec<String> = vec![];
    for o in &n.options {
        let o = line_ok(o, wire::ASK_OPTION_MAX, "an option")?;
        if options.iter().any(|x| x == o) {
            return Err(bad("the options differ from each other"));
        }
        options.push(o.to_owned());
    }
    let recommend = n.recommend.as_deref().map(str::trim).filter(|r| !r.is_empty());
    if recommend.is_some_and(|r| !options.iter().any(|o| o == r)) {
        return Err(bad("the recommendation is one of the options"));
    }
    Ok(Checked { question, why, options, recommend })
}

/// Ask `n.who` a question on `row` of board `id`. A person's role is the
/// ceiling (a viewer asks nothing), an agent needs `boards.write`, and the
/// one asked is a member of the project.
pub async fn ask(
    app: &Platform,
    by: Actor<'_>,
    id: &str,
    row: &str,
    n: &wire::NewAsk,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, by, id, true).await?;
    super::id_ok(row, "row").map_err(|_| bad("expected a row id that starts row_"))?;
    let c = checked(n)?;
    if n.who.is_empty() || role_in(app, &board.project, &n.who).await?.is_none() {
        return Err(bad("an ask is for a member of the project: who is not one"));
    }
    let new = NewAsk {
        who: &n.who,
        question: c.question,
        why: c.why,
        options: &c.options,
        recommend: c.recommend,
        asked_by: by.account,
        asked_by_agent: by.agent,
    };
    app.db.ask_add(id, row, &new, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, by.account, id).await
}

/// Answer an ask: the person it names, or an owner or editor of the project
/// for them (403 for anyone else). `a` holds one of its options, or text for
/// an ask with none.
pub async fn answer(
    app: &Platform,
    account: &str,
    id: &str,
    ask_id: &str,
    a: &wire::AnswerAsk,
) -> Result<wire::BoardView, ApiError> {
    let board = board_place(app, Actor::person(account), id, false).await?;
    super::id_ok(ask_id, "ask").map_err(|_| bad("expected an ask id that starts ask_"))?;
    let found = app.db.board_ask(id, ask_id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such ask"))?;
    let role = role_in(app, &board.project, account).await?;
    if found.who != account && !matches!(role, Some(TeamRole::Owner | TeamRole::Editor)) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the person an ask names answers it, or an owner or editor for them",
        ));
    }
    let answer = match (&found.options[..], &a.option, &a.text) {
        (&[], None, Some(t)) => text_ok(t, "an answer")
            .and_then(|t| if t.is_empty() { Err(bad("an answer can't be empty")) } else { Ok(t.to_owned()) })?,
        (&[], _, _) => return Err(bad("this ask takes text, as {\"text\": …}")),
        (options, Some(o), None) => match options.iter().find(|x| *x == o) {
            Some(o) => o.clone(),
            None => return Err(bad(&format!("the answer is one of: {}", options.join(", ")))),
        },
        _ => return Err(bad("this ask takes one of its options, as {\"option\": …}")),
    };
    app.db.ask_answer(id, ask_id, &answer, account, now_ms())?.map_err(refused)?;
    changed(app, &board.project, id);
    read(app, account, id).await
}

/// A board's asks as one reader sees them, for [`Asks::row`].
pub(super) struct Asks {
    by_row: HashMap<String, Vec<AskRow>>,
    /// Names of the accounts and machines the asks mention.
    names: HashMap<String, String>,
    reader: String,
    /// The reader is an owner or editor of the project.
    editor: bool,
}

impl Asks {
    /// The asks of board `id`, with the names they mention.
    pub(super) async fn load(app: &Platform, reader: &str, project: &str, id: &str) -> Result<Asks, ApiError> {
        let mut by_row: HashMap<String, Vec<AskRow>> = HashMap::new();
        for a in app.db.board_asks(id)? {
            by_row.entry(a.row.clone()).or_default().push(a);
        }
        let mut accounts: Vec<String> = by_row
            .values()
            .flatten()
            .flat_map(|a| [Some(a.who.clone()), Some(a.asked_by.clone()), a.answered_by.clone()])
            .flatten()
            .filter(|a| !a.is_empty())
            .collect();
        accounts.sort();
        accounts.dedup();
        let mut names = HashMap::new();
        if !accounts.is_empty() {
            for (a, p) in app.cache.accounts(&*app.hub, &accounts).await? {
                let name = if !p.login.is_empty() { p.login } else { p.name };
                names.insert(a, name);
            }
        }
        let mut machines: Vec<String> = by_row.values().flatten().filter_map(|a| a.asked_by_agent.clone()).collect();
        machines.sort();
        machines.dedup();
        if !machines.is_empty() {
            for (m, d) in agent_names(app, &machines).await? {
                names.insert(m, d.name);
            }
        }
        let editor = matches!(role_in(app, project, reader).await?, Some(TeamRole::Owner | TeamRole::Editor));
        Ok(Asks { by_row, names, reader: reader.to_owned(), editor })
    }

    fn name(&self, account: &str) -> String {
        match self.names.get(account) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => "someone who left".to_owned(),
        }
    }

    /// Who asked, as a name: the person, or the agent and its machine.
    fn asker(&self, a: &AskRow) -> String {
        match &a.asked_by_agent {
            Some(m) => {
                format!("an agent on {}", self.names.get(m).filter(|n| !n.is_empty()).map_or("a machine", |n| n))
            }
            None if a.asked_by.is_empty() => "someone who left".to_owned(),
            None => self.name(&a.asked_by),
        }
    }

    /// A row's open ask (asked, else `raised`) and its last few answered.
    pub(super) fn row(&self, row: &str, raised: Option<wire::Ask>) -> (Option<wire::Ask>, Vec<wire::AnsweredAsk>) {
        let all = self.by_row.get(row).map(Vec::as_slice).unwrap_or_default();
        let open = all.iter().find(|a| a.answer.is_none()).map(|a| wire::Ask {
            id: a.id.clone(),
            who: a.who.clone(),
            who_name: self.name(&a.who),
            question: a.question.clone(),
            why: Some(a.why.clone()).filter(|w| !w.is_empty()),
            options: a.options.clone(),
            recommend: a.recommend.clone(),
            asked_by: self.asker(a),
            raised: false,
            mine: a.who == self.reader,
            can_answer: a.who == self.reader || self.editor,
            created_at: a.created_at,
        });
        let mut done: Vec<&AskRow> = all.iter().filter(|a| a.answer.is_some()).collect();
        done.sort_by_key(|a| std::cmp::Reverse((a.answered_at, a.created_at)));
        let asked = done
            .into_iter()
            .take(wire::ASKED_SHOWN)
            .map(|a| wire::AnsweredAsk {
                id: a.id.clone(),
                question: a.question.clone(),
                answer: a.answer.clone().unwrap_or_default(),
                asked_by: self.asker(a),
                answered_by: self.name(a.answered_by.as_deref().unwrap_or_default()),
                answered_at: a.answered_at.unwrap_or_default(),
            })
            .collect();
        (open.or(raised), asked)
    }
}

/// What the board raises on a row, in this order: a PR of the row (its item
/// or attached work) whose checks failed, then an agent block attached to it
/// that is waiting for input. `live` and the attachments' chips are the
/// reader's own, so a PR or a block they may not see raises nothing for them.
pub(super) fn raised(
    live: Option<&wire::Live>,
    attachments: &[wire::Attachment],
    at: u64,
    machine_name: &dyn Fn(&str) -> String,
) -> Option<wire::Ask> {
    let ask = |question: String| wire::Ask {
        id: String::new(),
        who: String::new(),
        who_name: String::new(),
        question,
        why: None,
        options: vec![],
        recommend: None,
        asked_by: String::new(),
        raised: true,
        mine: true,
        can_answer: false,
        created_at: at,
    };
    let work = attachments.iter().filter(|a| a.role == wire::AttachRole::Work);
    let seen = live.into_iter().chain(work.clone().filter_map(|a| a.live.as_ref()));
    if let Some(l) = seen.into_iter().find(|l| l.kind == "pr" && l.state == wire::LiveState::ChecksFailed) {
        return Some(ask(format!("#{}'s checks failed", l.number)));
    }
    work.into_iter().find_map(|a| match (&a.pointer, &a.block) {
        (wire::Pointer::Block { machine, .. }, Some(b)) if b.state == wire::BlockWord::NeedsInput => {
            Some(ask(format!("{}'s agent is waiting for you", machine_name(machine))))
        }
        _ => None,
    })
}

// ---------------------------------------------------------------- routes

/// `POST /api/boards/{id}/rows/{row}/ask`: ask a member of the project a
/// question on a row (owners and editors).
pub async fn ask_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, row)): Path<(String, String)>,
    Json(n): Json<wire::NewAsk>,
) -> J<wire::BoardView> {
    Ok(Json(ask(&app, Actor::person(&s.account), &id, &row, &n).await?))
}

/// `POST /api/boards/{id}/asks/{ask}/answer`: answer an ask, as the person it
/// names or an owner or editor for them. A machine's signature is 401.
pub async fn answer_route(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, ask)): Path<(String, String)>,
    Json(a): Json<wire::AnswerAsk>,
) -> J<wire::BoardView> {
    Ok(Json(answer(&app, &s.account, &id, &ask, &a).await?))
}
