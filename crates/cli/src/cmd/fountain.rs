//! `arugula fountain`: Fountain agents as a catalog block, and this daemon's
//! Fountain sign-in (`login`, `logout`, `status`).

use super::Ctx;
use crate::http::{call_raw, enc, request};
use crate::util::{env_pane, loaded, open_block, print_json, split_of};
use anyhow::bail;
use arugula_proto::{
    BlockType,
    api::{Empty, FountainAgents, FountainLogin, FountainLoginRequest, OpenRequest},
    op::ops::{FountainLoginGet, FountainLoginStart, FountainLogout},
};
use serde_json::json;

/// Fountain: the catalog.
#[derive(clap::Subcommand)]
pub enum FountainCmd {
    /// List your agents here, one line each, without opening a block.
    Agents {
        /// Words to look for (names, descriptions, skills, MCP servers).
        query: Option<String>,
        /// agent-specs, hand or app.
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Sign this daemon in to Fountain with a code you approve in a browser.
    ///
    /// Prints the code and where to approve it (a phone signed in to
    /// Fountain will do), then waits. The key is kept in the daemon's state
    /// directory, readable by you only. Without a URL: the Fountain this
    /// daemon is signed in to, else FOUNTAIN_BASE_URL or the fountain CLI
    /// profile's.
    Login {
        /// The Fountain instance, e.g. `https://fountain.example`.
        url: Option<String>,
        /// Start the sign-in and return without waiting.
        #[arg(long)]
        no_wait: bool,
    },
    /// Forget this daemon's Fountain key, revoking it on Fountain.
    ///
    /// The fountain CLI's own login (FOUNTAIN_API_KEY,
    /// ~/.fountain/credentials) is left alone and used from then on.
    Logout,
    /// Which Fountain login this daemon uses, and its account stream.
    Status,
}

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Option<FountainCmd>,
    /// Start with this search (names, descriptions, skills, MCP servers).
    #[arg(long, short)]
    query: Option<String>,
    /// Start with this source: agent-specs, hand or app.
    #[arg(long)]
    source: Option<String>,
    /// The credentials profile (default: FOUNTAIN_PROFILE, else default).
    #[arg(long)]
    profile: Option<String>,
    /// Split a block instead of opening a tab: `right` for the one this
    /// runs in, or `%N`.
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    session: Option<String>,
}

/// Say what a login status means, for people.
fn say(s: &FountainLogin) {
    if let Some(n) = &s.note {
        println!("{n}");
    }
    match (&s.base_url, s.source.as_deref()) {
        (Some(base), Some(source)) if s.signed_in => {
            let from = match source {
                "arugula" => "this daemon's sign-in",
                "env" => "FOUNTAIN_API_KEY",
                _ => "the fountain CLI's credentials",
            };
            let who = s.email.as_deref().map(|e| format!(" as {e}")).unwrap_or_default();
            let prefix = s.key_prefix.as_deref().map(|p| format!(", key {p}…")).unwrap_or_default();
            println!("signed in to {base}{who} ({from}{prefix})");
        }
        _ => println!("not signed in to Fountain"),
    }
    if let Some(p) = &s.pending {
        println!("waiting for code {} to be approved at {}", p.user_code, p.verification_uri);
    }
    if let Some(e) = &s.error {
        println!("{e}");
    }
    let st = &s.stream;
    if st.enabled {
        let conn = if st.connected { "connected" } else { "not connected" };
        println!("account stream: {conn}, cursor {}, {} events since the daemon started", st.cursor, st.delivered);
    }
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    match args {
        Args { cmd: Some(FountainCmd::Status), .. } => {
            let (s, v) = call_raw::<FountainLoginGet>(&sock, &(), &Empty {})?;
            if json_out {
                print_json(&v);
            } else {
                say(&s);
            }
            return Ok(if s.signed_in { 0 } else { 1 });
        }
        Args { cmd: Some(FountainCmd::Logout), .. } => {
            let (s, v) = call_raw::<FountainLogout>(&sock, &(), &Empty {})?;
            if json_out {
                print_json(&v);
            } else {
                say(&s);
            }
        }
        Args { cmd: Some(FountainCmd::Login { url, no_wait }), .. } => {
            let (s, v) = call_raw::<FountainLoginStart>(&sock, &(), &FountainLoginRequest { base_url: url })?;
            let Some(p) = s.pending.clone() else { bail!("Fountain didn't give a code") };
            if json_out && no_wait {
                print_json(&v);
                return Ok(0);
            }
            if !json_out {
                println!("To sign this daemon in to {}, approve code {}", p.base_url, p.user_code);
                println!("at {}", p.verification_uri_complete.as_deref().unwrap_or(&p.verification_uri));
            }
            if no_wait {
                return Ok(0);
            }
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let (s, v) = call_raw::<FountainLoginGet>(&sock, &(), &Empty {})?;
                if s.pending.as_ref().is_some_and(|q| q.user_code == p.user_code) {
                    continue;
                }
                if json_out {
                    print_json(&v);
                }
                if s.source.as_deref() == Some("arugula") && s.error.is_none() {
                    if !json_out {
                        say(&s);
                    }
                    return Ok(0);
                }
                bail!("{}", s.error.as_deref().unwrap_or("the sign-in didn't finish"));
            }
        }
        Args { cmd: Some(FountainCmd::Agents { query, source, profile }), .. } => {
            let mut q = vec![];
            for (k, v) in [("query", query), ("source", source), ("profile", profile)] {
                if let Some(v) = v {
                    q.push(format!("{k}={}", enc(&v)));
                }
            }
            let path = if q.is_empty() {
                "/api/fountain/agents".into()
            } else {
                format!("/api/fountain/agents?{}", q.join("&"))
            };
            let (agents, v) = request(&sock, "GET", &path, None)?.parse_raw::<FountainAgents>()?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            let rows = agents.agents;
            let base_url = if agents.base_url.is_empty() { "Fountain" } else { &agents.base_url };
            println!("{} of {} agents on {base_url}", rows.len(), agents.total);
            if agents.unreadable > 0 {
                let n = agents.unreadable;
                println!("({n} couldn't be read: Fountain sent something this Arugula doesn't understand)");
            }
            for r in rows {
                let s = |k: &str| r[k].as_str().unwrap_or("").to_owned();
                let list = |k: &str| {
                    r[k].as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default()
                };
                let mut line = format!("{}  [{}] {}", s("name"), s("runtime"), s("source"));
                if !list("skills").is_empty() {
                    line.push_str(&format!("  skills: {}", list("skills")));
                }
                if !list("mcp").is_empty() {
                    line.push_str(&format!("  mcp: {}", list("mcp")));
                }
                println!("{line}");
            }
        }
        Args { cmd: None, query, source, profile, split, session } => {
            let mut filter = json!({});
            if let Some(q) = query {
                filter["query"] = json!(q);
            }
            if let Some(s) = source {
                filter["sources"] = json!(s.split(',').map(str::trim).collect::<Vec<_>>());
            }
            let body = OpenRequest {
                kind: BlockType::Fountain,
                config: json!({ "profile": profile, "filter": filter }),
                split: split_of(split.as_deref())?,
                session,
                from_pane: env_pane(),
                ..Default::default()
            };
            let block = open_block(&sock, &body)?.0.block;
            let v = loaded(&sock, block)?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            println!("%{block}");
            if let Some(e) = v["state"]["error"].as_str() {
                bail!("{e}");
            }
            print!("{}", request(&sock, "GET", &format!("/api/panes/{block}/capture"), None)?.text()?);
        }
    }
    Ok(0)
}
