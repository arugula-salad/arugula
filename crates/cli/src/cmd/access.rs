//! `arugula access`: who else can reach which sessions.

use super::Ctx;
use crate::http::{request, request_op, send_op};
use crate::util::{print_json, time};
use arugula_proto::{
    api::Empty,
    op::ops::{AclGet, AclSet},
};

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
