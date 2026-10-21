//! PRs and issues on linked repos through Arugula control (M169): `pr
//! view|list|checks`, `issue view|list|create`, `pr comment|review` and
//! `issue comment` on `OWNER/REPO#N`, and `arugula draft`. No block opens
//! and no daemon is asked: each is one typed operation
//! (`arugula_control_wire::forge_ops`) on control's platform, as this
//! CLI's account (`arugula login`).
//!
//! A person's comment, review or new issue sends; run by an agent
//! (CLAUDECODE or AI_AGENT set), or with `--draft`, it's a draft that waits
//! for a person. Sending from control is M168's: until then a send says it
//! isn't available yet.

use anyhow::bail;
use arugula_control_wire::{
    forge_ops::{Call, CommentOn, DraftQuery, DraftRef, FORGE_OPS, ItemQuery, ItemRef, NewIssue, Op, ReviewOf},
    projects::NewDraft,
};
use serde_json::Value;

use crate::{control::Enrolled, http, util::print_json};

/// `arugula draft`: drafts waiting for a person, on linked repos.
#[derive(clap::Subcommand)]
pub enum DraftCmd {
    /// Waiting drafts: yours, or a target's.
    ///
    /// On `OWNER/REPO#N`, on `OWNER/REPO`, or a project's (`--project`).
    Ls {
        /// `OWNER/REPO#N` or `OWNER/REPO`.
        target: Option<String>,
        /// A project's id (prj_…).
        #[arg(long)]
        project: Option<String>,
        /// waiting (the default), sent, discarded, failed or all.
        #[arg(long)]
        state: Option<String>,
    },
    /// One draft, in full.
    Show { draft: String },
    /// Discard a draft.
    Discard { draft: String },
    /// Send a draft, as you.
    Send { draft: String },
}

/// `pr list` and `issue list`'s filters.
#[derive(clap::Args, Clone, Default)]
pub struct ListArgs {
    /// A linked repo, `OWNER/REPO`. Default: the project's.
    pub repo: Option<String>,
    /// A project's id (prj_…). Default: the repo's, else your personal one.
    #[arg(long)]
    pub project: Option<String>,
    /// open, closed or merged.
    #[arg(long)]
    pub state: Option<String>,
    /// A GitHub login.
    #[arg(long)]
    pub author: Option<String>,
    /// A GitHub login asked to review.
    #[arg(long)]
    pub review_requested: Option<String>,
    /// Text in the title.
    #[arg(long)]
    pub search: Option<String>,
    #[arg(long)]
    pub limit: Option<u32>,
}

/// `OWNER/REPO#N` (or its link): one for control. `%N` or `N` is a block's.
pub fn item(target: &str) -> Option<ItemRef> {
    ItemRef::parse(target)
}

/// One operation on control: its result.
pub fn call(op: Op) -> anyhow::Result<Value> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    // A send that isn't a draft's can be one instead.
    let may_draft = op.sends() && !matches!(op, Op::DraftSend(_));
    let body = serde_json::to_value(Call::new(op))?;
    match e.api("POST", FORGE_OPS, Some(&body)) {
        Ok(v) => Ok(v["result"].clone()),
        Err(e) if may_draft && format!("{e:#}").contains("isn't available yet") => {
            bail!("{e:#}; `--draft` keeps it as a draft to send later")
        }
        Err(e) => Err(e),
    }
}

/// A send, or the draft of it: an agent's (or `--draft`) waits for a person.
pub fn send_or_draft(send: Op, draft: NewDraft, as_draft: bool, json_out: bool) -> anyhow::Result<i32> {
    if as_draft || http::agent() {
        let mut d = draft;
        if http::agent() {
            d.agent = Some("cli".into());
        }
        let v = call(Op::DraftCreate(d))?;
        if json_out {
            print_json(&v);
        } else {
            println!("draft {}: waits for a person to send it", v["draft"]["id"].as_str().unwrap_or("?"));
        }
    } else {
        let v = call(send)?;
        if json_out { print_json(&v) } else { println!("{}", v["url"].as_str().unwrap_or("sent")) }
    }
    Ok(0)
}

/// `comment` on `OWNER/REPO#N`.
pub fn comment(at: ItemRef, body: String, issue: bool, as_draft: bool, json_out: bool) -> anyhow::Result<i32> {
    let draft = NewDraft {
        repo: at.repo.clone(),
        number: Some(at.number),
        kind: "comment".into(),
        body: body.clone(),
        ..Default::default()
    };
    let on = CommentOn { repo: at.repo, number: at.number, body };
    let send = if issue { Op::IssueComment(on) } else { Op::PrComment(on) };
    send_or_draft(send, draft, as_draft, json_out)
}

/// `review` of `OWNER/REPO#N`: approve, request_changes or comment.
pub fn review(at: ItemRef, event: &str, body: Option<String>, as_draft: bool, json_out: bool) -> anyhow::Result<i32> {
    let event = match event.to_ascii_lowercase().replace('-', "_").as_str() {
        "approve" => "APPROVE",
        "request_changes" => "REQUEST_CHANGES",
        "comment" => "COMMENT",
        _ => bail!("a review's event: approve, request_changes or comment"),
    };
    let body = body.unwrap_or_default();
    let draft = NewDraft {
        repo: at.repo.clone(),
        number: Some(at.number),
        kind: "review".into(),
        event: Some(event.into()),
        body: body.clone(),
        ..Default::default()
    };
    let send = Op::PrReview(ReviewOf { repo: at.repo, number: at.number, event: event.into(), body });
    send_or_draft(send, draft, as_draft, json_out)
}

/// `issue create OWNER/REPO`.
pub fn create_issue(
    repo: String,
    title: String,
    body: Option<String>,
    as_draft: bool,
    json_out: bool,
) -> anyhow::Result<i32> {
    let body = body.unwrap_or_default();
    let draft = NewDraft {
        repo: repo.clone(),
        kind: "issue".into(),
        title: Some(title.clone()),
        body: body.clone(),
        ..Default::default()
    };
    send_or_draft(Op::IssueCreate(NewIssue { repo, title, body }), draft, as_draft, json_out)
}

/// `pr view` and `issue view`.
pub fn view(at: ItemRef, issue: bool, json_out: bool) -> anyhow::Result<i32> {
    let v = call(if issue { Op::IssueGet(at) } else { Op::PrGet(at) })?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    let i = &v["item"];
    println!("{}", line(i));
    let mut facts = vec![i["id"].as_str().unwrap_or("").to_owned()];
    if let (Some(h), Some(b)) = (i["head_ref"].as_str(), i["base_ref"].as_str()) {
        let sha = i["head_sha"].as_str().map(|s| format!(" ({})", &s[..s.len().min(7)])).unwrap_or_default();
        facts.push(format!("{h}{sha} → {b}"));
    }
    for (key, name) in [("reviewers", "reviewers"), ("assignees", "assignees"), ("labels", "labels")] {
        let list: Vec<&str> = i[key].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !list.is_empty() {
            facts.push(format!("{name}: {}", list.join(", ")));
        }
    }
    println!("  {}", facts.join(" · "));
    for e in v["events"].as_array().into_iter().flatten() {
        let what = e["review_state"].as_str().map_or_else(
            || e["kind"].as_str().unwrap_or("?").to_owned(),
            |s| format!("{} {}", e["kind"].as_str().unwrap_or("?"), s.replace('_', " ")),
        );
        let by = e["actor"].as_str().map(|a| format!(" by {a}")).unwrap_or_default();
        println!("  {what}{by}, {}", crate::hosts::ago(e["at"].as_u64().unwrap_or(0)));
    }
    for d in v["drafts"].as_array().into_iter().flatten() {
        println!("  {}", draft_line(d));
    }
    Ok(0)
}

/// `pr list` and `issue list`.
pub fn list(a: ListArgs, kind: &str, json_out: bool) -> anyhow::Result<i32> {
    let q = ItemQuery {
        project: a.project,
        repo: a.repo,
        kind: Some(kind.into()),
        state: a.state,
        author: a.author,
        review_requested: a.review_requested,
        q: a.search,
        limit: a.limit,
    };
    let v = call(Op::ItemList(q))?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    let items = v["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        println!("None.");
    }
    for i in &items {
        println!("{}", line(i));
    }
    for w in v["withheld"].as_array().into_iter().flatten() {
        eprintln!("{}: withheld ({})", w["repo"].as_str().unwrap_or("?"), w["why"].as_str().unwrap_or("?"));
    }
    Ok(0)
}

/// `pr checks`.
pub fn checks(at: ItemRef, json_out: bool) -> anyhow::Result<i32> {
    let v = call(Op::ChecksGet(at.clone()))?;
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    let checks = v["checks"].as_array().cloned().unwrap_or_default();
    if checks.is_empty() {
        println!("{at} has no checks.");
    }
    for c in &checks {
        let how = c["conclusion"].as_str().or(c["status"].as_str()).unwrap_or("?");
        println!("{:<12} {}", how, c["name"].as_str().unwrap_or("?"));
    }
    Ok(0)
}

/// `arugula draft`.
pub fn draft(cmd: DraftCmd, json_out: bool) -> anyhow::Result<i32> {
    let v = match cmd {
        DraftCmd::Ls { target, project, state } => {
            let (repo, number) = match target.as_deref() {
                None => (None, None),
                Some(t) => match item(t) {
                    Some(at) => (Some(at.repo), Some(at.number)),
                    None => (Some(t.to_owned()), None),
                },
            };
            let v = call(Op::DraftList(DraftQuery { project, repo, number, state, ..Default::default() }))?;
            if !json_out {
                let drafts = v["drafts"].as_array().cloned().unwrap_or_default();
                if drafts.is_empty() {
                    println!("No drafts.");
                }
                for d in &drafts {
                    println!("{}", draft_line(d));
                }
                return Ok(0);
            }
            v
        }
        DraftCmd::Show { draft } => {
            let v = call(Op::DraftGet(DraftRef { draft }))?;
            if !json_out {
                println!("{}", draft_line(&v));
                if let Some(t) = v["title"].as_str() {
                    println!("\n{t}");
                }
                if let Some(b) = v["body"].as_str().filter(|b| !b.is_empty()) {
                    println!("\n{b}");
                }
                if let Some(e) = v["error"].as_str() {
                    println!("\nfailed: {e}");
                }
                return Ok(0);
            }
            v
        }
        DraftCmd::Discard { draft } => {
            let v = call(Op::DraftDiscard(DraftRef { draft: draft.clone() }))?;
            if !json_out {
                println!("{draft} discarded");
                return Ok(0);
            }
            v
        }
        DraftCmd::Send { draft } => {
            if http::agent() {
                bail!("an agent doesn't send: {draft} waits for a person to send it");
            }
            let v = call(Op::DraftSend(DraftRef { draft }))?;
            if !json_out {
                println!("{}", v["url"].as_str().unwrap_or("sent"));
                return Ok(0);
            }
            v
        }
    };
    print_json(&v);
    Ok(0)
}

/// `acme/app#7  pr  open  Fix it  (alice-gh)`.
fn line(i: &Value) -> String {
    let at = format!("{}#{}", i["repo"].as_str().unwrap_or("?"), i["number"]);
    let state = match (i["state"].as_str().unwrap_or("?"), i["draft"] == true) {
        ("open", true) => "draft".to_owned(),
        (s, _) => s.to_owned(),
    };
    let by = i["author"].as_str().map(|a| format!("  ({a})")).unwrap_or_default();
    format!("{at:<28} {:<5} {state:<6} {}{by}", i["kind"].as_str().unwrap_or("?"), i["title"].as_str().unwrap_or(""))
}

/// `dft_…  review (approve) on acme/app#7  waiting  by mcp:claude-code on geek`.
fn draft_line(d: &Value) -> String {
    let what = match (d["kind"].as_str(), d["event"].as_str()) {
        (Some("review"), Some(e)) => format!("review ({})", e.to_ascii_lowercase()),
        (Some("issue"), _) => format!("new issue {:?}", d["title"].as_str().unwrap_or("")),
        (k, _) => k.unwrap_or("?").to_owned(),
    };
    let on = match d["number"].as_u64() {
        Some(n) => format!("{}#{n}", d["repo"].as_str().unwrap_or("?")),
        None => d["repo"].as_str().unwrap_or("?").to_owned(),
    };
    let by = match (d["agent"].as_str(), d["machine_name"].as_str().or(d["machine"].as_str())) {
        (Some(a), Some(m)) => format!("  by {a} on {m}"),
        (Some(a), None) => format!("  by {a}"),
        _ => String::new(),
    };
    format!("{}  {what} on {on}  {}{by}", d["id"].as_str().unwrap_or("?"), d["state"].as_str().unwrap_or("?"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn targets_and_commands() {
        assert_eq!(item("acme/app#7").map(|r| r.number), Some(7));
        assert!(item("%7").is_none() && item("7").is_none());
        for argv in [
            &["arugula", "pr", "view", "acme/app#7"][..],
            &["arugula", "pr", "list"],
            &["arugula", "pr", "list", "acme/app", "--state", "open", "--review-requested", "me"],
            &["arugula", "pr", "checks", "acme/app#7"],
            &["arugula", "pr", "comment", "acme/app#7", "lgtm", "--draft"],
            &["arugula", "pr", "comment", "%3", "lgtm"],
            &["arugula", "pr", "review", "acme/app#7", "approve"],
            &["arugula", "pr", "review", "%3", "request_changes", "split it"],
            &["arugula", "issue", "create", "acme/app", "--title", "T", "--body", "B"],
            &["arugula", "issue", "comment", "acme/app#2", "on it"],
            &["arugula", "issue", "view", "acme/app#2"],
            &["arugula", "issue", "list", "acme/app"],
            &["arugula", "draft", "ls"],
            &["arugula", "draft", "ls", "acme/app#7"],
            &["arugula", "draft", "show", "dft_1"],
            &["arugula", "draft", "discard", "dft_1"],
            &["arugula", "draft", "send", "dft_1"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
    }

    #[test]
    fn lines() {
        let i = json!({ "repo": "acme/app", "number": 7, "kind": "pr", "state": "open", "draft": true,
                        "title": "Fix it", "author": "alice" });
        let l = line(&i);
        assert!(l.starts_with("acme/app#7") && l.contains("draft") && l.ends_with("Fix it  (alice)"), "{l}");
        let d = json!({ "id": "dft_1", "kind": "review", "event": "REQUEST_CHANGES", "repo": "acme/app",
                        "number": 7, "state": "waiting", "agent": "mcp:claude-code", "machine_name": "geek" });
        assert_eq!(
            draft_line(&d),
            "dft_1  review (request_changes) on acme/app#7  waiting  by mcp:claude-code on geek"
        );
    }
}
