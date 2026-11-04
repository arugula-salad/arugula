//! `arugula access`: who can reach this machine, and as what (Arugula
//! #681), and the session shares it changes. With no arguments it shows the
//! machine's access view (`access.get`): the owner, the roster's people by
//! their team role, session shares and their floors, links and when they
//! end, ssh guests and trust live now, each row saying whose word it is;
//! then this machine's agents' powers, which are Arugula cloud's word
//! (`GET /api/grants`, as `arugula grants ls` reads it, as this CLI's
//! login), labelled so. An older daemon has no `access.get`: then its
//! session shares alone, as before, and a line saying what's missing.

use super::Ctx;
use crate::http::{request, request_op, send_op};
use crate::util::{print_json, time};
use arugula_proto::{
    api::{AccessView, Empty},
    op::ops::{AccessGet, AclGet, AclSet},
};
use serde_json::{Value, json};

#[derive(clap::Subcommand)]
pub enum AccessCmd {
    /// Let someone reach a session, at once.
    ///
    /// As a viewer (watch), an editor (drive its panes, make and close tabs
    /// and splits) or an owner. WHO is a tailnet login: everyone signed in
    /// to Tailscale as it gets the role, on any device.
    Grant { session: String, who: String, role: String },
    /// Take it away; they're cut off at once.
    Revoke { session: String, who: String },
    /// Every grant and revoke, oldest first.
    Log,
}

pub fn run(cmd: Option<AccessCmd>, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let principal = |who: &str| if who.contains(':') { who.to_owned() } else { format!("tailnet:{who}") };
    // A session by name or id ($N), from the panes list.
    let session_id = |want: &str| -> anyhow::Result<u64> {
        let v = request(&sock, "GET", "/api/panes", None)?.json()?;
        let id = want.strip_prefix('$').and_then(|n| n.parse().ok());
        v.as_array()
            .into_iter()
            .flatten()
            .find(|p| Some(p["session"].as_u64().unwrap_or(0)) == id || p["session_name"].as_str() == Some(want))
            .and_then(|p| p["session"].as_u64())
            .ok_or_else(|| anyhow::anyhow!("no session {want}"))
    };
    let log = matches!(cmd, Some(AccessCmd::Log));
    if cmd.is_none() {
        return view(&sock, json_out);
    }
    let v = match cmd {
        None | Some(AccessCmd::Log) => send_op::<AclGet>(&sock, &(), &Empty {})?.json()?,
        Some(AccessCmd::Grant { session, who, role }) => {
            if !matches!(role.as_str(), "viewer" | "editor" | "owner") {
                anyhow::bail!("a role is viewer, editor or owner");
            }
            let body =
                serde_json::json!({ "session": session_id(&session)?, "principal": principal(&who), "role": role });
            request_op::<AclSet>(&sock, &(), &body)?.json()?
        }
        Some(AccessCmd::Revoke { session, who }) => {
            let body =
                serde_json::json!({ "session": session_id(&session)?, "principal": principal(&who), "role": null });
            request_op::<AclSet>(&sock, &(), &body)?.json()?
        }
    };
    if json_out {
        print_json(&v);
        return Ok(0);
    }
    if log {
        for e in v["audit"].as_array().into_iter().flatten() {
            println!(
                "{}  {:<6} ${:<3} {:<30} {}",
                time(e["at"].as_u64().unwrap_or(0)),
                e["action"].as_str().unwrap_or(""),
                e["session"],
                e["principal"].as_str().unwrap_or(""),
                e["role"].as_str().unwrap_or("")
            );
        }
    } else {
        let grants = v["grants"].as_array().cloned().unwrap_or_default();
        if grants.is_empty() {
            println!("nothing is shared");
        }
        for g in grants {
            println!(
                "${:<4} {:<7} {}",
                g["session"],
                g["role"].as_str().unwrap_or(""),
                g["principal"].as_str().unwrap_or("")
            );
        }
        for note in caller_notes(&v["callers"]) {
            println!("note: {note}");
        }
    }
    Ok(0)
}

/// The machine's access view (Arugula #681), and its agents' powers from
/// Arugula cloud. An older daemon (404 on `access.get`) gets the session
/// shares `GET /api/acl` lists, and a line saying so.
fn view(sock: &crate::http::Target, json_out: bool) -> anyhow::Result<i32> {
    let res = send_op::<AccessGet>(sock, &(), &Empty {})?;
    if res.status == 404 {
        let v: Value = send_op::<AclGet>(sock, &(), &Empty {})?.json()?;
        let grants = v["grants"].as_array().cloned().unwrap_or_default();
        if json_out {
            print_json(&json!({ "older_daemon": true, "grants": grants, "callers": v["callers"] }));
            return Ok(0);
        }
        println!(
            "This daemon is older than this arugula: it lists its session shares only. Update it to see who can reach it."
        );
        if grants.is_empty() {
            println!("nothing is shared");
        }
        for g in grants {
            println!(
                "${:<4} {:<7} {}",
                g["session"],
                g["role"].as_str().unwrap_or(""),
                g["principal"].as_str().unwrap_or("")
            );
        }
        for note in caller_notes(&v["callers"]) {
            println!("note: {note}");
        }
        return Ok(0);
    }
    let (view, raw) = res.parse_raw::<AccessView>()?;
    let powers = powers_of(&view);
    if json_out {
        let mut out = raw;
        match &powers {
            Ok(p) => out["powers"] = json!(p),
            Err(e) => out["powers_error"] = json!(e),
        }
        print_json(&out);
        return Ok(0);
    }
    for line in lines(&view) {
        println!("{line}");
    }
    println!();
    println!("Its agents' powers on Arugula cloud (control's word, `arugula grants`):");
    match powers {
        Ok(p) if p.is_empty() => println!("  none"),
        Ok(p) => {
            for g in p {
                println!("  {}", power_line(&g));
            }
        }
        Err(e) => println!("  {e}"),
    }
    Ok(0)
}

/// This machine's live powers, as `GET /api/grants` lists them for this
/// CLI's login: by its device id in control's directory, else its name.
/// `Err`: why they couldn't be read, in a sentence to show.
fn powers_of(view: &AccessView) -> Result<Vec<Value>, String> {
    if view.kind == "local" {
        return Err("not joined to Arugula cloud: its agents have no powers there".into());
    }
    let e = match crate::control::Enrolled::load() {
        Ok(Some(e)) => e,
        Ok(None) => {
            return Err("this CLI isn't logged in to Arugula cloud (`arugula login`), so they aren't shown".into());
        }
        Err(e) => return Err(format!("couldn't read this CLI's login: {e:#}")),
    };
    // The machine's control is the one to ask: another's knows nothing of it.
    if let Some(theirs) = &view.control
        && theirs.trim_end_matches('/') != e.url.trim_end_matches('/')
    {
        return Err(format!(
            "this CLI is logged in to {}, and the machine is joined to {theirs}: `arugula grants ls` there lists them",
            e.url
        ));
    }
    let listed = e.api("GET", "/api/grants", None).map_err(|e| format!("Arugula cloud didn't answer: {e:#}"))?;
    let mine = listed["grants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["revoked_at"].is_null() && g["subject"]["kind"] == "daemon")
        .filter(|g| match &view.machine {
            Some(id) => g["subject"]["id"] == id.as_str(),
            None => g["subject"]["name"] == view.name.as_str(),
        })
        .cloned()
        .collect();
    Ok(mine)
}

fn power_line(g: &Value) -> String {
    let scope = match (g["scope"]["kind"].as_str(), g["power"].as_str()) {
        (Some("project"), _) => {
            format!(
                "over the project {}",
                g["scope"]["name"].as_str().or(g["scope"]["id"].as_str()).unwrap_or_default()
            )
        }
        (_, Some("cloud.drive")) => "over your account".into(),
        _ => "over your DMs".into(),
    };
    format!("{:<16} {scope}", g["power"].as_str().unwrap_or_default())
}

/// The view as lines: the machine, its notes, then one line per row.
pub fn lines(v: &AccessView) -> Vec<String> {
    let mut out = Vec::new();
    let whose = match &v.team {
        Some(t) => format!("the team {}'s project", t.name),
        None => "a personal project".to_owned(),
    };
    let kind = match (v.kind.as_str(), v.purpose.as_deref(), &v.team) {
        ("local", _, _) => "not joined to Arugula cloud".to_owned(),
        ("team", _, Some(t)) => format!("a machine of the team {}{}", t.name, if t.locked { " (locked)" } else { "" }),
        ("team", _, None) => "a team machine (its roster isn't checked yet)".to_owned(),
        ("hosted", Some("deploy" | "both"), _) => format!("a deploy's box of {whose}: nobody gets in"),
        ("hosted", Some("snapshot"), Some(t)) if t.locked => format!("a box building a snapshot of {whose} (locked)"),
        ("hosted", Some("snapshot"), _) => format!("a box building a snapshot of {whose}"),
        ("hosted", _, Some(t)) if t.locked => format!("a Cloud workspace of {whose} (locked)"),
        ("hosted", _, _) => format!("a Cloud workspace of {whose}"),
        _ => "joined to Arugula cloud".to_owned(),
    };
    out.push(format!("{}: {kind}", v.name));
    out.extend(v.notes.iter().map(|n| format!("note: {n}")));
    if v.rows.is_empty() {
        out.push("nobody can reach it".into());
        return out;
    }
    out.push(format!("{:<24} {:<7} {:<20} {:<13} {}", "who", "as", "where", "says", ""));
    for r in &v.rows {
        let session_name = |id: u32| v.sessions.iter().find(|s| s.id == id).map(|s| s.name.as_str()).unwrap_or("");
        let where_ = match (r.pane, r.session) {
            (Some(p), Some(s)) => format!("%{p} in ${s} {}", session_name(s)),
            (Some(p), None) => format!("%{p}"),
            (None, Some(s)) => format!("${s} {}", session_name(s)),
            (None, None) => "every session".into(),
        };
        let says = match r.word.as_str() {
            "roster" => "the roster",
            _ => "this machine",
        };
        let mut more: Vec<String> = Vec::new();
        if let Some(f) = r.from_ms {
            more.push(format!("from {}", time(f)));
        }
        if let Some(u) = r.until_ms {
            more.push(format!("until {}", until(u)));
        }
        if let Some(n) = &r.note {
            more.push(n.clone());
        }
        out.push(format!(
            "{:<24} {:<7} {:<20} {:<13} {}",
            r.who,
            r.role.as_str(),
            where_.trim_end(),
            says,
            more.join(", ")
        ));
    }
    out
}

/// A moment ahead, as "in 3m", or "passed".
fn until(ms: u64) -> String {
    let now = crate::util::now_ms();
    if ms <= now { "passed".into() } else { format!("in {}", crate::util::span((ms - now) / 1000)) }
}

/// What `GET /api/acl`'s `callers` says (#663): a granted login seen from
/// more than one device, whose people share its one role, and tagged
/// devices, refused for having no login. Also `arugula status`'s.
pub fn caller_notes(callers: &serde_json::Value) -> Vec<String> {
    let names = |v: &serde_json::Value, key: &str| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x[key].as_str().or_else(|| x.as_str()).map(str::to_owned))
            .collect()
    };
    let mut out = Vec::new();
    for s in callers["shared"].as_array().into_iter().flatten() {
        let devices = names(&s["devices"], "device");
        out.push(format!(
            "{} is signed in on {} devices ({}): everyone on them has its role, and Arugula can't tell them apart",
            s["login"].as_str().unwrap_or("?"),
            devices.len(),
            devices.join(", ")
        ));
    }
    for t in callers["tagged"].as_array().into_iter().flatten() {
        let tags = names(&t["tags"], "");
        out.push(format!(
            "{} ({}) is a tagged device: it has no login, so it was refused and nothing can be shared with it",
            t["device"].as_str().unwrap_or("?"),
            tags.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view's lines: the machine, its notes, one line per row with
    /// where it reaches and whose word it is.
    #[test]
    fn the_view_reads_as_lines() {
        let v: AccessView = serde_json::from_value(json!({
            "kind": "team", "name": "box", "machine": "dev1",
            "team": { "id": "t1", "name": "Acme", "locked": false },
            "sessions": [{ "id": 3, "name": "build" }],
            "notes": ["The team Acme is locked: only its owners get in."],
            "rows": [
                { "kind": "owner", "word": "this_machine", "who": "alex", "role": "owner", "note": "joined to Arugula cloud as alex" },
                { "kind": "team_role", "word": "roster", "who": "bea", "role": "editor" },
                { "kind": "share", "word": "this_machine", "who": "friend@example.com", "role": "viewer", "session": 3 },
                { "kind": "trust", "word": "this_machine", "who": "friend@example.com", "role": "editor", "session": 3, "pane": 7, "until_ms": 0 },
            ],
        }))
        .unwrap();
        let l = lines(&v);
        assert_eq!(l[0], "box: a machine of the team Acme");
        let deploy = AccessView {
            kind: "hosted".into(),
            purpose: Some("deploy".into()),
            name: "box".into(),
            ..Default::default()
        };
        assert_eq!(lines(&deploy)[0], "box: a deploy's box of a personal project: nobody gets in");
        assert_eq!(l[1], "note: The team Acme is locked: only its owners get in.");
        assert!(l[2].starts_with("who"));
        assert_eq!(
            l[3].split_whitespace().collect::<Vec<_>>(),
            ["alex", "owner", "every", "session", "this", "machine", "joined", "to", "Arugula", "cloud", "as", "alex"]
        );
        assert_eq!(l[4].split_whitespace().collect::<Vec<_>>(), ["bea", "editor", "every", "session", "the", "roster"]);
        assert_eq!(
            l[5].split_whitespace().collect::<Vec<_>>(),
            ["friend@example.com", "viewer", "$3", "build", "this", "machine"]
        );
        assert_eq!(
            l[6].split_whitespace().collect::<Vec<_>>(),
            ["friend@example.com", "editor", "%7", "in", "$3", "build", "this", "machine", "until", "passed"]
        );
        // The powers' line names the project.
        assert_eq!(
            power_line(&json!({ "power": "issues.write", "scope": { "kind": "project", "name": "acme" } })),
            "issues.write     over the project acme"
        );
        // Not joined: nothing to ask control for.
        let local = AccessView { kind: "local".into(), ..Default::default() };
        assert!(powers_of(&local).unwrap_err().starts_with("not joined"));
        assert_eq!(l[3].split_whitespace().next(), Some("alex"));
    }

    #[test]
    fn says_who_shares_a_login_and_which_tagged_devices_called() {
        let v = serde_json::json!({
            "shared": [{"login": "bob@x.com", "devices": [{"device": "ipad", "at": 1}, {"device": "laptop", "at": 2}]}],
            "tagged": [{"device": "ci-1", "tags": ["tag:ci"], "at": 3}],
        });
        let n = super::caller_notes(&v);
        assert_eq!(
            n,
            [
                "bob@x.com is signed in on 2 devices (ipad, laptop): everyone on them has its role, and Arugula can't tell them apart",
                "ci-1 (tag:ci) is a tagged device: it has no login, so it was refused and nothing can be shared with it",
            ]
        );
        // An older daemon says nothing of callers.
        assert!(super::caller_notes(&serde_json::Value::Null).is_empty());
    }
}
