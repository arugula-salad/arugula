//! `arugula studio`: your studio's token and follower links.

use super::Ctx;
use crate::http::{call_raw, enc};
use crate::{term, util::print_json};
use anyhow::bail;
use arugula_proto::{
    api::{Empty, FollowerLinkRequest, StudioLoggedIn, StudioLoginRequest, StudioStatus},
    op::ops::{StudioFollow, StudioGet, StudioLogin, StudioLogout, StudioUnfollow},
};
use std::io::Write;

/// What a studio route said, for people.
enum Said {
    Status(StudioStatus),
    LoggedIn(StudioLoggedIn),
    Nothing,
}

#[derive(clap::Subcommand)]
pub enum StudioCmd {
    /// Keep a studio token in the daemon.
    ///
    /// Mode 0600, never sent to a client. The token is read from stdin, or
    /// asked for.
    Login {
        /// The studio, e.g. `https://studio.example`.
        url: String,
    },
    /// Forget the token, and every follower link.
    Logout,
    /// Keep an app's follower link, or drop it.
    ///
    /// The link the box's owner made with `hud share --role follower` (read
    /// from stdin); with `--forget`, drop it.
    Follower {
        app: String,
        #[arg(long)]
        forget: bool,
    },
}

pub fn run(cmd: Option<StudioCmd>, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let (said, v) = match cmd {
        None => {
            let (s, v) = call_raw::<StudioGet>(&sock, &(), &Empty {})?;
            (Said::Status(s), v)
        }
        Some(StudioCmd::Login { url }) => {
            let token = secret_input("Studio token: ")?;
            let (l, v) = call_raw::<StudioLogin>(&sock, &(), &StudioLoginRequest { url, token })?;
            (Said::LoggedIn(l), v)
        }
        Some(StudioCmd::Logout) => (Said::Nothing, call_raw::<StudioLogout>(&sock, &(), &Empty {})?.1),
        Some(StudioCmd::Follower { app, forget: true }) => {
            (Said::Nothing, call_raw::<StudioUnfollow>(&sock, &enc(&app), &Empty {})?.1)
        }
        Some(StudioCmd::Follower { app, forget: false }) => {
            let link = secret_input("Follower link: ")?;
            (Said::Nothing, call_raw::<StudioFollow>(&sock, &enc(&app), &FollowerLinkRequest { link })?.1)
        }
    };
    if json_out {
        print_json(&v);
    } else {
        match said {
            Said::LoggedIn(l) => {
                println!("logged in; {} app{}", l.apps.len(), if l.apps.len() == 1 { "" } else { "s" })
            }
            Said::Status(StudioStatus { url: Some(url), logged_in, followers }) => {
                let state = if logged_in { "logged in" } else { "logged out" };
                println!("{url}: {state}");
                for f in followers {
                    println!("  follower link for {f}");
                }
            }
            Said::Status(_) => println!("no studio: `arugula studio login <url>`"),
            Said::Nothing => {}
        }
    }
    Ok(0)
}

/// A secret from stdin: piped, the first line; on a terminal, asked for
/// without echo.
fn secret_input(prompt: &str) -> anyhow::Result<String> {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    let mut line = String::new();
    if tty {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        let read = term::read_hidden(&mut line);
        eprintln!();
        read?;
    } else {
        stdin.read_line(&mut line)?;
    }
    let v = line.trim().to_owned();
    if v.is_empty() {
        bail!("nothing given on stdin");
    }
    Ok(v)
}
