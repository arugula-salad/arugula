//! Native issues from agents (M200): a project's own issues in Arugula
//! control (M194), read with `read_forge` (`issue`) and `forge_read`
//! (kinds `issue` and `native_issues`), written with `issue_write`. The
//! reading is here, since `read_forge` and `draft` are the base's; the
//! writes and `forge_read` are `arugulad-control-tools`' (#232).
//!
//! **Which issue:** `#12` is a number in the current project, `iss_…` an
//! issue anywhere the owner reads. **The current project** is the one the
//! call names (`project`: an id, or `personal`), else the issue's project
//! when the caller is an agent `start_agent` started on one (its block's
//! `work.project`, Arugula #771), else the one the caller's clone's GitHub
//! repo is linked to, else the owner's personal project. A
//! clone of a GitHub repo that no project of the owner's links is
//! refused, not guessed (as `arugula issue`'s `#N` there is GitHub's).
//!
//! **GitHub's issues stay drafts** a person sends (aru-041, aru-085):
//! `draft` and `forge_draft` refuse a native issue and say to use
//! `issue_write` ([`not_drafted`]).
//!
//! Each call is a machine-signed route on the platform
//! (`/api/daemon/issues/...`, `crates/platform/src/issues/agent.rs`). An
//! agent a guest stands behind may not use them: they act as the owner.

use arugula_control_wire::{
    chat::agent_messages,
    issues::{AGENT_ISSUE_HOME, agent_issue, agent_issue_events, agent_project_issue},
};
use reqwest::Method;
use serde_json::{Value, json};

use super::tools::Call;
use crate::{control::CloudRefused, mux::Api};

/// The current project a call meant, and why.
#[derive(Debug, PartialEq, Eq)]
pub struct Chosen {
    pub id: String,
    /// Its name (empty from a control that sends none).
    pub name: String,
    /// A team's project, not its owner's personal one (Arugula #862): where
    /// a team's Docs live.
    pub team: bool,
    pub rule: Rule,
}

/// What chose the current project.
#[derive(Debug, PartialEq, Eq)]
pub enum Rule {
    /// The call named it.
    Asked,
    /// The caller's block was started on this issue (`#N`), in it.
    Started(String),
    /// The clone's GitHub repo (`owner/name`) is linked to it.
    Linked(String),
    /// Neither: the owner's personal project.
    Personal,
    /// Control's default, asked for because the clone's GitHub repo is
    /// linked to no project yet.
    Default,
}

impl Rule {
    /// The rule that chooses with `asked` (the call's `project`), the
    /// issue the caller's block was started on and the clone's GitHub repo,
    /// if it has them: a named project wins, then the issue's, then the
    /// repo's link.
    fn of(asked: Option<&str>, started: Option<&Started>, repo: Option<&str>) -> Self {
        match (asked, started, repo) {
            (Some(_), _, _) => Rule::Asked,
            (None, Some(s), _) => Rule::Started(s.issue.clone()),
            (None, None, Some(repo)) => Rule::Linked(repo.to_owned()),
            (None, None, None) => Rule::Personal,
        }
    }
}

/// The issue the caller's block was started on, and its project.
#[derive(Debug, PartialEq, Eq)]
struct Started {
    issue: String,
    project: String,
}

impl Rule {
    /// What `issue-home` is asked, for this rule: the project named or
    /// started in, or the repo to find one by (nothing: personal).
    fn query(&self, asked: Option<&str>, started: Option<&Started>) -> Vec<(&'static str, String)> {
        match (self, asked, started) {
            (Rule::Asked, Some(p), _) => vec![("project", p.to_owned())],
            (Rule::Started(_), _, Some(s)) => vec![("project", s.project.clone())],
            (Rule::Linked(repo), _, _) => vec![("repo", repo.clone())],
            _ => vec![],
        }
    }

    /// Why it chose the project.
    fn why(&self) -> String {
        match self {
            Rule::Asked => "the one you named".into(),
            Rule::Started(issue) => format!("you were started on its issue {issue}"),
            Rule::Linked(repo) => format!("this clone's repo {repo} is linked to it"),
            Rule::Personal => "you named no project, and no GitHub repo of this clone says another".into(),
            Rule::Default => "this clone's repo is linked to no project yet, so control's default".into(),
        }
    }
}

impl Chosen {
    /// One line for a result: which project, and which rule.
    pub fn says(&self) -> String {
        let name = if self.name.is_empty() { self.id.clone() } else { self.name.clone() };
        match &self.rule {
            Rule::Personal => format!("Your personal project {name} ({})", self.rule.why()),
            r => format!("Project {name} ({})", r.why()),
        }
    }

    /// What a `#N` that isn't in it says: where it looked, why, and how to
    /// look in another (Arugula #771).
    pub fn has_no(&self, n: u64) -> String {
        let named = if self.name.is_empty() { self.id.clone() } else { format!("{} ({})", self.name, self.id) };
        format!(
            "no #{n} in {named}, the current project ({}); pass project (a prj_ id, or personal) to look in another",
            self.rule.why()
        )
    }

    /// "A team Doc in <name>: <rule>", for `doc new`: the rule in its own
    /// words, as `issue_write` says it. A team machine's default project is
    /// the one no rule chose.
    pub fn team_doc(&self) -> String {
        let name = if self.name.is_empty() { self.id.clone() } else { self.name.clone() };
        let why = match &self.rule {
            Rule::Personal => "this is the team's machine, and no project was named".to_owned(),
            r => r.why(),
        };
        format!("A team Doc in {name}: {why}")
    }

    /// Whether nothing chose it but the fallback.
    pub fn is_personal_by_default(&self) -> bool {
        self.rule == Rule::Personal
    }
}

/// A native issue: `#N` in the current project, or `iss_…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Native {
    Id(String),
    Number(u64),
}

impl Native {
    /// `None`: not a native issue's (GitHub's, a block's).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.strip_prefix("iss_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Some(Native::Id(s.to_owned()));
        }
        s.strip_prefix('#').and_then(|n| n.parse().ok()).map(Native::Number)
    }
}

pub fn native(s: &str) -> Result<Native, String> {
    Native::parse(s).ok_or_else(|| format!("issue: #N (in the current project) or iss_…, not {s:?}"))
}

/// What `draft` and `forge_draft` say to a native issue.
pub fn not_drafted(s: &str) -> String {
    format!(
        "{} is a native issue in Arugula control: those aren't drafted, issue_write writes to them at once (kind comment, edit, label, assign, close or reopen). Drafts are for GitHub, where a person sends them: forge_draft kind open_on_github drafts opening it there.",
        s.trim()
    )
}

/// What a refusal says to an agent.
pub fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where native issues live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents use native issues yet".into()
        }
        e => e.to_string(),
    }
}

/// `#12 [open] Title`, its labels and who opened it.
pub fn line(i: &Value) -> String {
    let mut s = format!(
        "#{} [{}{}] {} ({})",
        i["number"],
        i["state"].as_str().unwrap_or("?"),
        i["close_reason"].as_str().map(|r| format!(", {}", r.replace('_', " "))).unwrap_or_default(),
        i["title"].as_str().unwrap_or(""),
        i["id"].as_str().unwrap_or("?"),
    );
    let labels: Vec<&str> = i["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !labels.is_empty() {
        s += &format!(" labels {}", labels.join(", "));
    }
    s
}

/// Who: an account, or "<account>'s agent on <machine>".
fn who(account: &Value, agent: &Value) -> String {
    let a = account.as_str().filter(|a| !a.is_empty()).unwrap_or("a deleted account");
    match agent["machine_name"].as_str().filter(|n| !n.is_empty()).or(agent["machine"].as_str()) {
        Some(m) => format!("{a}'s agent on {m}"),
        None => a.to_owned(),
    }
}

/// An issue, its timeline and its newest comments as text.
pub fn text(issue: &Value, events: &[Value], comments: &[Value]) -> String {
    let mut out = format!("{}\nOpened by {}", line(issue), who(&issue["author"], &issue["agent"]));
    let assignees: Vec<&str> = issue["assignees"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !assignees.is_empty() {
        out += &format!(", assigned to {}", assignees.join(", "));
    }
    if let Some(b) = issue["body"].as_str().filter(|b| !b.is_empty()) {
        out += &format!("\n\n{b}");
    }
    if !events.is_empty() {
        out += "\n\nTimeline:";
        for e in events {
            let d = &e["detail"];
            let what = d["label"].as_str().or(d["assignee"].as_str()).or(d["reason"].as_str());
            // A merged PR that closed it, and its words (Arugula #743).
            let pr = match (d["pr"].as_str(), d["by"].as_str()) {
                (Some(pr), Some(by)) => format!(" with {pr}, by {by}"),
                (Some(pr), None) => format!(" with {pr}"),
                _ => String::new(),
            };
            out += &format!(
                "\n- {} {}{}{pr}",
                who(&e["actor"], &e["agent"]),
                e["kind"].as_str().unwrap_or("?"),
                what.map(|w| format!(" {w}")).unwrap_or_default()
            );
        }
    }
    if !comments.is_empty() {
        out += "\n\nComments:";
        for m in comments {
            let by = match m["agent"]["machine_name"].as_str() {
                Some(machine) => format!("{}'s agent on {machine}", m["name"].as_str().unwrap_or("?")),
                None => m["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("someone").to_owned(),
            };
            // Posted for them by a product (M237: a hud box's review); or
            // what an agent session said, no person's (Arugula #215).
            let said = m["via"].as_str().and_then(arugula_control_wire::sessions::said_of);
            let by = match (said, m["via"].as_str()) {
                (Some(ses), _) => {
                    format!("agent session {}", ses.trim_start_matches("ses_").chars().take(8).collect::<String>())
                }
                (None, Some(v)) => format!("{by} via {v}"),
                (None, None) => by,
            };
            out += &format!("\n[{}] {by}: {}", m["seq"], m["text"].as_str().unwrap_or(""));
        }
    }
    out
}

pub fn query(pairs: &[(&str, String)]) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        q.append_pair(k, v);
    }
    q.finish()
}

impl Call<'_> {
    /// They act as this machine's owner: only the owner's own agents may.
    pub async fn owners_own(&self, what: &str) -> Result<(), String> {
        if let Some(me) = self.me()
            && self.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
        {
            return Err(format!("{what} acts as this machine's owner: only their own agents may"));
        }
        Ok(())
    }

    pub async fn issues_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
    }

    /// The GitHub repo (`owner/name`) of the caller's own clone, if its
    /// pane is on this host in a clone of one.
    async fn my_github_repo(&self) -> Option<String> {
        let pane = self.own_pane()?;
        let info = self.readable(pane).await.ok()?.info;
        if info.host.is_some() {
            return None;
        }
        github_repo_of(&info.cwd?).await
    }

    /// M203: the project whose own issue `#N` in `dir` is, when the clone's
    /// GitHub repo is linked to one whose issue home is native (M199's and
    /// this module's rule). `None`: the clone's forge's, as before.
    pub async fn native_home_of(&self, dir: &str) -> Option<String> {
        let repo = github_repo_of(dir).await?;
        let path = format!("{AGENT_ISSUE_HOME}?{}", query(&[("repo", repo)]));
        let v = self.issues_call(Method::GET, &path, None).await.ok()?;
        (v["home"] == "native").then(|| v["project"].as_str().map(str::to_owned)).flatten()
    }

    /// The current project's id (module docs).
    pub async fn current_project(&self, project: Option<&str>) -> Result<String, String> {
        Ok(self.chosen_project(project).await?.id)
    }

    /// The issue the caller's own block was started on, and its project
    /// (`start_agent`'s `issue`, kept as its `work`).
    async fn my_started_issue(&self) -> Option<Started> {
        let pane = self.own_pane()?;
        let block = self.app().mux.api(|r| Api::Block(pane, r)).await.flatten()?;
        let work = &block.config()["work"];
        let project = work["project"].as_str().filter(|p| !p.is_empty())?;
        Some(Started { issue: work["issue"].as_str().unwrap_or_default().to_owned(), project: project.to_owned() })
    }

    /// The current project, and the rule that chose it (module docs).
    pub async fn chosen_project(&self, project: Option<&str>) -> Result<Chosen, String> {
        let asked = project.map(str::trim).filter(|p| !p.is_empty());
        let started = match asked {
            Some(_) => None,
            None => self.my_started_issue().await,
        };
        let repo = match (asked, &started) {
            (None, None) => self.my_github_repo().await,
            _ => None,
        };
        let rule = Rule::of(asked, started.as_ref(), repo.as_deref());
        let q = rule.query(asked, started.as_ref());
        let path = if q.is_empty() { AGENT_ISSUE_HOME.to_owned() } else { format!("{AGENT_ISSUE_HOME}?{}", query(&q)) };
        let v = self.issues_call(Method::GET, &path, None).await?;
        let id = v["project"].as_str().ok_or("Arugula control named no project")?;
        let (name, team) = (v["name"].as_str().unwrap_or_default().to_owned(), v["team"] == true);
        Ok(Chosen { id: id.to_owned(), name, team, rule })
    }

    /// Control's default project (issue-home with no query: personal on a
    /// personal machine, the team's on a team machine), for a call that
    /// links a repo from a clone whose own repo no project links yet.
    pub async fn default_project(&self) -> Result<Chosen, String> {
        let v = self.issues_call(Method::GET, AGENT_ISSUE_HOME, None).await?;
        let id = v["project"].as_str().ok_or("Arugula control named no project")?;
        let (name, team) = (v["name"].as_str().unwrap_or_default().to_owned(), v["team"] == true);
        Ok(Chosen { id: id.to_owned(), name, team, rule: Rule::Default })
    }

    /// A native issue, as the platform answers it.
    /// A `#N` that isn't there names the project it looked in.
    pub async fn native_issue(&self, at: &Native, project: Option<&str>) -> Result<Value, String> {
        let n = match at {
            Native::Id(id) => return self.issues_call(Method::GET, &agent_issue(id), None).await,
            Native::Number(n) => *n,
        };
        let chosen = self.chosen_project(project).await?;
        match self.app().control.cloud(Method::GET, &agent_project_issue(&chosen.id, n), None).await {
            Err(CloudRefused::Said { status: 404, .. }) => Err(chosen.has_no(n)),
            r => r.map_err(said),
        }
    }

    /// `read_forge`'s `issue` and `forge_read`'s kind `issue` on a native
    /// one: it, its timeline and its newest comments (with threads.read;
    /// left out, saying why, if they can't be read).
    pub async fn read_native(&self, issue: &str, project: Option<&str>) -> Result<(String, Value), String> {
        self.owners_own("reading a native issue").await?;
        let i = self.native_issue(&native(issue)?, project).await?;
        let id = i["id"].as_str().unwrap_or_default().to_owned();
        let events = self.issues_call(Method::GET, &agent_issue_events(&id), None).await?;
        let events = events["events"].as_array().cloned().unwrap_or_default();
        let (comments, more, note) = match self.issues_call(Method::GET, &agent_messages(&id), None).await {
            Ok(v) => (v["messages"].as_array().cloned().unwrap_or_default(), v["more"].clone(), None),
            Err(e) => (vec![], json!(false), Some(e)),
        };
        let mut t = text(&i, &events, &comments);
        if let Some(n) = &note {
            t += &format!("\n\n(Its comments weren't read: {n})");
        }
        let summary = format!(
            "Native issue {} in project {}: {} event(s), {} comment(s){}; read_thread with room {id} pages its comments, issue_write writes to it at once",
            line(&i),
            i["project"].as_str().unwrap_or("?"),
            events.len(),
            comments.len(),
            if more == true { " (the newest)" } else { "" },
        );
        let v = json!({ "issue": i, "events": events, "comments": comments, "more_comments": more,
                        "comments_note": note, "text": t });
        Ok((summary, v))
    }
}

/// The GitHub repo (`owner/name`) of the clone at `dir`: its `origin`'s,
/// else the first remote's that's on github.com.
async fn github_repo_of(dir: &str) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["remote", "-v"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .await
        .ok()?;
    let mut remotes: Vec<(String, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some((p.next()?.to_owned(), p.next()?.to_owned()))
        })
        .collect();
    remotes.sort_by_key(|(name, _)| name != "origin");
    remotes
        .iter()
        .filter_map(|(_, url)| crate::git_remote::parse_remote(url))
        .find(|(host, _)| host == "github.com")
        .map(|(_, repo)| repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_project_wins_then_the_started_issues_then_the_repos_link() {
        let started = Started { issue: "#771".into(), project: "prj_2".into() };
        assert_eq!(Rule::of(Some("prj_1"), Some(&started), Some("acme/app")), Rule::Asked);
        assert_eq!(Rule::of(None, Some(&started), Some("acme/app")), Rule::Started("#771".into()));
        assert_eq!(Rule::of(None, None, Some("acme/app")), Rule::Linked("acme/app".into()));
        assert_eq!(Rule::of(None, None, None), Rule::Personal);
        assert_eq!(Rule::Asked.query(Some("prj_1"), None), [("project", "prj_1".to_owned())]);
        assert_eq!(Rule::Started("#771".into()).query(None, Some(&started)), [("project", "prj_2".to_owned())]);
        assert_eq!(Rule::Linked("acme/app".into()).query(None, None), [("repo", "acme/app".to_owned())]);
        assert!(Rule::Personal.query(None, None).is_empty());
    }

    #[test]
    fn the_chosen_project_says_which_rule_chose_it() {
        let chosen = |rule| Chosen { id: "prj_1".into(), name: "Arugula".into(), team: false, rule };
        assert_eq!(chosen(Rule::Asked).says(), "Project Arugula (the one you named)");
        assert_eq!(
            chosen(Rule::Linked("acme/app".into())).says(),
            "Project Arugula (this clone's repo acme/app is linked to it)"
        );
        assert!(chosen(Rule::Personal).says().starts_with("Your personal project Arugula (you named no project"));
        assert!(chosen(Rule::Personal).is_personal_by_default());
        assert_eq!(chosen(Rule::Started("#9".into())).says(), "Project Arugula (you were started on its issue #9)");
        let nameless = Chosen { id: "prj_1".into(), name: String::new(), team: false, rule: Rule::Asked };
        assert_eq!(nameless.says(), "Project prj_1 (the one you named)");
    }

    /// Arugula #771: a missing `#N` says where it looked and how to look
    /// elsewhere.
    #[test]
    fn a_missing_number_names_the_project_it_looked_in() {
        let personal = Chosen { id: "prj_125a".into(), name: "Personal".into(), team: false, rule: Rule::Personal };
        let said = personal.has_no(761);
        assert!(
            said.starts_with("no #761 in Personal (prj_125a), the current project (you named no project"),
            "{said}"
        );
        assert!(said.ends_with("pass project (a prj_ id, or personal) to look in another"), "{said}");
        let nameless = Chosen { id: "prj_1".into(), name: String::new(), team: false, rule: Rule::Asked };
        assert!(nameless.has_no(3).starts_with("no #3 in prj_1, the current project (the one you named)"));
    }

    #[test]
    fn native_issues_are_named_by_number_or_id() {
        let id = format!("iss_{}", "0a".repeat(16));
        assert_eq!(Native::parse(&id), Some(Native::Id(id.clone())));
        assert_eq!(Native::parse(" #12 "), Some(Native::Number(12)));
        for not in ["12", "%12", "acme/app#12", "https://github.com/acme/app/issues/12", "iss_12", "#x"] {
            assert_eq!(Native::parse(not), None, "{not}");
        }
        assert!(native("acme/app#1").unwrap_err().contains("#N"));
    }

    /// A comment an agent session made is the session's, not a person's
    /// (Arugula #215); a product's names who it posted for.
    #[test]
    fn a_sessions_comment_reads_as_the_session() {
        let issue = json!({ "number": 1, "title": "t", "state": "open", "author": "a" });
        let comments = [
            json!({ "seq": 1, "name": "", "via": "said:ses_0123456789abcdef", "text": "bisected" }),
            json!({ "seq": 2, "name": "Ann", "via": "hud", "text": "reviewed" }),
        ];
        let t = text(&issue, &[], &comments);
        assert!(t.contains("[1] agent session 01234567: bisected"), "{t}");
        assert!(t.contains("[2] Ann via hud: reviewed"), "{t}");
    }

    /// A close by a merged PR names the PR and the words that did it
    /// (Arugula #743).
    #[test]
    fn a_close_by_a_pr_says_which_and_by_what_words() {
        let issue = json!({ "number": 4, "title": "t", "state": "closed", "author": "a" });
        let closed = |detail| json!({ "actor": "bob", "kind": "closed", "detail": detail });
        let by = json!({ "reason": "completed", "pr": "acme/site#70", "by": "\"(#4)\" in the title" });
        let t = text(&issue, &[closed(by), closed(json!({ "reason": "not_planned" }))], &[]);
        assert!(t.contains("\n- bob closed completed with acme/site#70, by \"(#4)\" in the title"), "{t}");
        assert!(t.contains("\n- bob closed not_planned\n") || t.ends_with("\n- bob closed not_planned"), "{t}");
    }
}
