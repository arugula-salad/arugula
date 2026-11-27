//! `show` kind `place` (Arugula #1496): an agent puts a pane on a place its
//! person looks at, a board or a project's Overview, through the person's
//! layout of it on the platform (`arugula_control_wire::layout`). `beside`
//! puts it next to the place's own panes, part of the conversation now;
//! `for_you` opens a tab for something waiting on them, with a row under
//! Needs you pointing at it, and never switches them there. The platform
//! keeps the rules: the agent only moves what agents opened and the person
//! hasn't touched, and a fourth agent pane in a tab closes the oldest.

use arugula_control_wire::layout::{
    Intent, LayoutChange, LayoutChanged, LayoutPrefs, NewPane, PaneKind, Place, REF_MAX, TITLE_MAX, agent_layout,
};
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::{
    control::CloudRefused,
    mcp::tools::{Call, Kind, Out, done},
};

pub(crate) const SHOW: Kind = Kind {
    name: "place",
    description: "A pane on a board or the Overview in the app (place), not a block here; intent beside or for_you.",
    schema: schema_for_type::<ShowPlaceArgs>,
    flag: None,
};

#[derive(Deserialize, JsonSchema)]
pub struct ShowPlaceArgs {
    /// board brd_... or overview (overview prj_... for another project).
    pub place: String,
    /// What the pane shows: doc, pr, preview, run, terminal, card, or prompt (a blocked agent's permission or question, answered there).
    pub pane: String,
    /// What it is, by pane: a Doc's id, OWNER/REPO#N, a URL or port, run_..., %N, iss_..., or the waiting pane's %N.
    #[serde(rename = "ref")]
    pub target: String,
    /// Its title, one line.
    pub title: String,
    /// Its state now, one line ("running · 3m", "waits for Allow"): shown while its machine is offline.
    #[serde(default)]
    pub state: Option<String>,
    /// beside (default): next to the place's own panes, for what you're talking about now. for_you: a tab of its own for what waits on them (a review, a PR to approve, a prompt), with a Needs you row pointing at it. Neither takes their focus. You only move what you opened and they haven't touched; a fourth pane of yours in a tab closes your oldest. Shown again, a pane updates.
    #[serde(default)]
    pub intent: Option<String>,
    /// For a while, in place of one of the place's own panes (its name, such as boards); closing yours puts it back.
    #[serde(default)]
    pub in_place_of: Option<String>,
    /// The machine its source is on, for terminal, preview and prompt. Default: this one.
    #[serde(default)]
    pub machine: Option<String>,
}

/// `board brd_…`, `board:brd_…`, `brd_…`, `overview` (the current
/// project's) or `overview prj_…`, as the platform names it.
async fn place_of(c: &Call<'_>, asked: &str) -> Result<String, String> {
    let asked = asked.trim();
    let (kind, id) = match asked.split_once([' ', ':']) {
        Some((k, id)) => (k.trim(), id.trim()),
        None if asked.starts_with("brd_") => ("board", asked),
        None => (asked, ""),
    };
    let s = match (kind, id) {
        ("overview", "") => format!("overview:{}", c.current_project(None).await?),
        (k, id) => format!("{k}:{id}"),
    };
    Place::parse(&s)
        .map(|p| p.to_string())
        .ok_or_else(|| format!("place is board brd_... or overview (overview prj_...), not {asked:?}"))
}

fn kind_of(s: &str) -> Result<PaneKind, String> {
    match serde_json::from_value(json!(s.trim())) {
        Ok(PaneKind::Other) | Err(_) => {
            Err(format!("pane is doc, pr, preview, run, terminal, card, prompt or room, not {s:?}"))
        }
        Ok(k) => Ok(k),
    }
}

fn line(what: &str, s: &str) -> Result<String, String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.chars().count() {
        0 => Err(format!("{what} can't be empty")),
        n if n > TITLE_MAX => Err(format!("{what} is at most {TITLE_MAX} characters")),
        _ => Ok(s),
    }
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula cloud, where places live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 404 | 405, .. } => {
            "Arugula cloud doesn't take panes on places yet, or there's no such place".into()
        }
        e => e.to_string(),
    }
}

pub(crate) async fn show_place(c: &Call<'_>, a: ShowPlaceArgs) -> Out {
    c.owners_own("show (kind place)").await?;
    let place = place_of(c, &a.place).await?;
    let intent = match a.intent.as_deref().map(str::trim) {
        None | Some("" | "beside") => Intent::Beside,
        Some("for_you") => Intent::ForYou,
        Some(other) => return Err(format!("intent is beside or for_you, not {other:?}")),
    };
    let target = a.target.trim().to_owned();
    if target.is_empty() || target.chars().count() > REF_MAX {
        return Err(format!("ref names what the pane shows, in at most {REF_MAX} characters"));
    }
    let pane = NewPane {
        kind: kind_of(&a.pane)?,
        target,
        title: line("title", &a.title)?,
        machine: a.machine.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty()),
        state: a.state.as_deref().map(|s| line("state", s)).transpose()?,
    };
    let in_place_of = a.in_place_of.map(|d| d.trim().to_owned()).filter(|d| !d.is_empty());
    let change = LayoutChange::Show { pane, intent, tab: None, in_place_of };
    let body = serde_json::to_value(&change).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, &agent_layout(&place), Some(&body)).await.map_err(said)?;
    let got: LayoutChanged = serde_json::from_value(v).map_err(|e| format!("Arugula cloud answered oddly: {e}"))?;
    let title = match &change {
        LayoutChange::Show { pane, .. } => pane.title.clone(),
        _ => unreachable!(),
    };
    let mut text = match intent {
        Intent::Beside => format!("Showing {title} beside {place}'s own panes"),
        Intent::ForYou => {
            format!(
                "Put {title} in a tab of its own on {place}, with a Needs you row pointing at it; they aren't switched there"
            )
        }
    };
    for p in &got.closed {
        text.push_str(&format!("; closed {} (your oldest) to make room", p.title));
    }
    let prefs = got.prefs.as_ref().map(LayoutPrefs::on).unwrap_or_default();
    if !prefs.is_empty() {
        text.push_str(&format!(". Their layout says: {}; follow it", prefs.join("; ")));
    }
    done(
        text,
        json!({ "place": place, "pane": got.pane, "tab": got.tab, "closed": got.closed.iter().map(|p| &p.title).collect::<Vec<_>>(), "prefs": prefs }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_kind_is_one_of_seven() {
        assert_eq!(kind_of("prompt"), Ok(PaneKind::Prompt));
        assert_eq!(kind_of("room"), Ok(PaneKind::Room));
        assert_eq!(kind_of(" doc "), Ok(PaneKind::Doc));
        assert!(kind_of("hologram").unwrap_err().contains("doc, pr, preview"));
    }

    #[test]
    fn a_line_is_one_line_and_not_too_long() {
        assert_eq!(line("title", " Review:\n Needs  you ").unwrap(), "Review: Needs you");
        assert!(line("title", "  ").is_err());
        assert!(line("title", &"x".repeat(TITLE_MAX + 1)).is_err());
    }
}
