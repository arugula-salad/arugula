//! Links between a project's items, from agents (Arugula #761): `link` says
//! which work comes before which (or blocks, relates to, duplicates or was
//! found from another), and takes a link off.
//!
//! - **Through the owner's grants.** One machine-signed route on the
//!   platform (`/api/daemon/projects/{project}/relations/batch`), which asks
//!   this machine's `issues.write` grant over the project and never goes
//!   past its owner's role. No board is needed: the platform resolves
//!   `#N`, `owner/repo#N`, `IDENT-N` and `iss_`/`fi_` ids itself.
//! - **Each link on its own.** One that is refused (a cycle, an item that
//!   isn't the project's, a link a person made) leaves the rest to land.
//! - **Which project** is the issues' rule (`Call::chosen_project`), after
//!   the project its `IDENT-N` items are connected in (`plane_home`).

use arugula_control_wire::boards::{
    LINKS_MAX, LinkBatch, LinkEnds, LinkOutcome, LinksDone, RelationKind, agent_project_relations_batch,
};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::tools::{Call, Out, done};

pub(crate) const TOOL: &str = "link";

pub(crate) const DESCRIPTION: &str = "Say which work comes before which, in Arugula control, at once, as your person's agent on this machine: add or remove links between a project's items. A link is `from` before `to` (the default kind), or blocks, relates, duplicate_of or discovered_from. An item is #N, owner/repo#N, IDENT-N or an iss_/fi_ id. `to` may be a list, one link to each. Up to 50 links in one call; each lands on its own, and the answer is one line per link (added, already there, removed, not there, or refused and why). A cycle of before and blocks links is refused. Only blocks keeps a row out of a board's Ready line. You remove only links your machine made. Needs your person's issues.write grant over the project: with no project, the one its IDENT-N items are connected in, else the current project, the project you pass, else the one your block was started on (start_agent's issue), else your session's, else the one this clone's GitHub repo is linked to, else your person's default project. Example: {\"add\":[{\"from\":\"#506\",\"to\":\"#507\"}]}.";

#[derive(Deserialize, JsonSchema)]
pub struct LinkArgs {
    /// Links to add.
    #[serde(default)]
    pub add: Vec<LinkArg>,
    /// Links to take off: the same from, to and kind as when added.
    #[serde(default)]
    pub remove: Vec<LinkArg>,
    /// The project's id (prj_…) or personal. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct LinkArg {
    /// The first item: #N, owner/repo#N, IDENT-N, or an iss_/fi_ id.
    pub from: String,
    /// The second item, written the same way, or a list of them (one link
    /// each). `from` is `kind` it.
    pub to: LinkTo,
    /// before (from comes first), blocks (from must be closed before to can
    /// start), relates, duplicate_of or discovered_from. Default before.
    #[serde(default)]
    pub kind: LinkKind,
}

/// One item, or several to link `from` to.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LinkTo {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, JsonSchema, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    #[default]
    Before,
    Blocks,
    Relates,
    DuplicateOf,
    DiscoveredFrom,
}

impl From<LinkKind> for RelationKind {
    fn from(k: LinkKind) -> Self {
        match k {
            LinkKind::Before => RelationKind::Before,
            LinkKind::Blocks => RelationKind::Blocks,
            LinkKind::Relates => RelationKind::Relates,
            LinkKind::DuplicateOf => RelationKind::DuplicateOf,
            LinkKind::DiscoveredFrom => RelationKind::DiscoveredFrom,
        }
    }
}

/// One pair for each `to`.
fn pairs(links: Vec<LinkArg>) -> Vec<Pair> {
    let mut out = vec![];
    for l in links {
        let tos = match l.to {
            LinkTo::One(t) => vec![t],
            LinkTo::Many(ts) => ts,
        };
        out.extend(tos.into_iter().map(|to| Pair { from: l.from.clone(), to, kind: l.kind.into() }));
    }
    out
}

struct Pair {
    from: String,
    to: String,
    kind: RelationKind,
}

impl Pair {
    fn words(&self) -> String {
        format!("{} {} {}", self.from.trim(), self.kind.as_str().replace('_', " "), self.to.trim())
    }

    fn ends(self) -> LinkEnds {
        LinkEnds { from: self.from, to: self.to, kind: self.kind }
    }
}

/// `link`.
pub(crate) async fn link(c: &Call<'_>, a: LinkArgs) -> Out {
    c.owners_own(TOOL).await?;
    let (add, remove) = (pairs(a.add), pairs(a.remove));
    let n = add.len() + remove.len();
    if n == 0 {
        return Err("link needs add or remove, each with a `to` (one item, or a list of them)".into());
    }
    if n > LINKS_MAX {
        return Err(format!("at most {LINKS_MAX} links in one call; you gave {n}"));
    }
    let ends_said: Vec<&str> = add.iter().chain(&remove).flat_map(|l| [l.from.as_str(), l.to.as_str()]).collect();
    let project = super::plane_home::home_or_main(c, a.project.as_deref(), &ends_said).await?;
    let said: Vec<String> = add.iter().chain(&remove).map(Pair::words).collect();
    let batch = LinkBatch {
        add: add.into_iter().map(Pair::ends).collect(),
        remove: remove.into_iter().map(Pair::ends).collect(),
    };
    let body = serde_json::to_value(batch).map_err(|e| e.to_string())?;
    let v = c.issues_call(Method::POST, &agent_project_relations_batch(&project), Some(body)).await?;
    let got: LinksDone =
        serde_json::from_value(v).map_err(|e| format!("Arugula control sent an answer I can't read: {e}"))?;
    Ok(answer(&said, &got))
}

/// One line for each link, then the count.
fn answer(said: &[String], got: &LinksDone) -> Value {
    let mut lines = vec![];
    let mut count = std::collections::BTreeMap::<&str, usize>::new();
    for (words, r) in said.iter().zip(got.add.iter().chain(&got.remove)) {
        let what = match r.outcome {
            LinkOutcome::Added => "added",
            LinkOutcome::AlreadyThere => "already there",
            LinkOutcome::Removed => "removed",
            LinkOutcome::NotThere => "not there",
            LinkOutcome::Refused => "refused",
        };
        *count.entry(what).or_default() += 1;
        lines.push(match &r.why {
            Some(why) => format!("{what}: {words} ({why})"),
            None => format!("{what}: {words}"),
        });
    }
    let total = count.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ");
    lines.push(format!("{} link(s): {total}.", said.len()));
    done(lines.join("\n"), got).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arugula_control_wire::boards::LinkResult;

    /// A link with no kind is `before`; the answer is a line for each, then the count.
    #[test]
    fn link_defaults_to_before_and_answers_a_line_each() {
        let a: LinkArgs = serde_json::from_value(serde_json::json!({
            "add": [{ "from": "#1", "to": "#2" }, { "from": "#2", "to": "#1" }],
            "remove": [{ "from": "#3", "to": "#4", "kind": "blocks" }]
        }))
        .unwrap();
        assert!(matches!(a.add[0].kind, LinkKind::Before) && matches!(a.remove[0].kind, LinkKind::Blocks));
        let r = |outcome, why: Option<&str>| LinkResult { outcome, why: why.map(Into::into) };
        let got = LinksDone {
            add: vec![r(LinkOutcome::Added, None), r(LinkOutcome::Refused, Some("that would make a cycle"))],
            remove: vec![r(LinkOutcome::NotThere, None)],
        };
        let said = ["#1 before #2".to_owned(), "#2 before #1".to_owned(), "#3 blocks #4".to_owned()];
        let v = answer(&said, &got);
        assert_eq!(
            v["summary"],
            "added: #1 before #2\nrefused: #2 before #1 (that would make a cycle)\nnot there: #3 blocks #4\n3 link(s): 1 added, 1 not there, 1 refused."
        );
    }

    /// A batch of three with a list `to`: each `to` is its own pair, in order,
    /// and the one refused is said in its line while the others land.
    #[test]
    fn a_list_to_is_a_pair_each_and_one_refused_leaves_the_rest() {
        let a: LinkArgs = serde_json::from_value(serde_json::json!({
            "add": [
                { "from": "#1", "to": ["#2", "#nope"], "kind": "relates" },
                { "from": "#3", "to": "#4" }
            ]
        }))
        .unwrap();
        let add = pairs(a.add);
        let said: Vec<String> = add.iter().map(Pair::words).collect();
        assert_eq!(said, ["#1 relates #2", "#1 relates #nope", "#3 before #4"]);
        let r = |outcome, why: Option<&str>| LinkResult { outcome, why: why.map(Into::into) };
        let got = LinksDone {
            add: vec![
                r(LinkOutcome::Added, None),
                r(LinkOutcome::Refused, Some("#nope: no such item")),
                r(LinkOutcome::Added, None),
            ],
            remove: vec![],
        };
        assert_eq!(
            answer(&said, &got)["summary"],
            "added: #1 relates #2\nrefused: #1 relates #nope (#nope: no such item)\nadded: #3 before #4\n3 link(s): 2 added, 1 refused."
        );
    }

    /// The schema says `to` is a string or a list.
    #[test]
    fn the_schema_offers_to_as_one_or_a_list() {
        let s = serde_json::to_string(&schemars::schema_for!(LinkArgs)).unwrap();
        assert!(s.contains("\"LinkTo\"") && s.contains("\"array\""), "{s}");
    }
}
