//! `arugula join-token`: a join token (aru-141) for a machine to join
//! with at once, `arugulad join --token "$(arugula join-token)"`, as in a
//! script or an image's build. Control makes it (`POST /api/join-tokens`)
//! for this CLI's account (`arugula login`), or for a team the account
//! owns or edits. It's good once, until it runs out.

use std::io::IsTerminal;

use anyhow::{Context, bail};
use arugula_control_wire::{self as wire, JoinTokenMade, JoinTokenRequest};

use crate::control::Enrolled;
use crate::util::print_json;

#[derive(clap::Args)]
pub struct Args {
    /// Join the machine to this team (its id, from the team's page), not
    /// your account alone. Owners and editors of the team only.
    #[arg(long)]
    team: Option<String>,
}

pub fn run(args: Args, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula cloud: `arugula login` first");
    };
    let made = make(&e, args.team)?;
    if json_out {
        print_json(&serde_json::to_value(&made)?);
        return Ok(0);
    }
    // Only the token on stdout, so `$(arugula join-token)` is it.
    println!("{}", made.token);
    if std::io::stderr().is_terminal() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let mins = made.expires.saturating_sub(now).div_ceil(60);
        eprintln!("Good once, for the next {mins} minutes: `arugulad join --token {}`", made.token);
    }
    Ok(0)
}

/// Control's join token for `team`, or for the account with none.
fn make(e: &Enrolled, team: Option<String>) -> anyhow::Result<JoinTokenMade> {
    let body = serde_json::to_value(JoinTokenRequest { team })?;
    let v = e.api("POST", wire::JOIN_TOKENS, Some(&body)).context("making a join token")?;
    serde_json::from_value(v).context("control's join token")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    /// A control that answers each `POST /api/join-tokens` in turn with
    /// `answers` (status, body), and sends back the bodies it was asked with.
    fn control(answers: Vec<(u16, &'static str)>) -> (Enrolled, std::sync::mpsc::Receiver<(String, String)>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for ((status, answer), sock) in answers.into_iter().zip(l.incoming()) {
                let sock = sock.unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let mut len = 0;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if h.trim().is_empty() {
                        break;
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                tx.send((path, String::from_utf8(body).unwrap())).unwrap();
                let mut sock = sock;
                write!(sock, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).unwrap();
            }
        });
        (Enrolled::for_tests_session(url, "ilg_session=s"), rx)
    }

    /// The account's token asks with `{}`, a team's names the team, and a
    /// refusal (a team's viewer) says control's words.
    #[test]
    fn a_join_token_is_the_accounts_or_a_teams_and_a_refusal_says_why() {
        let (e, asked) = control(vec![
            (200, r#"{"token":"ajt_one","expires":1800000000}"#),
            (200, r#"{"token":"ajt_two","expires":1800000000}"#),
            (403, r#"{"error":"only the team's owners and editors make join tokens"}"#),
        ]);
        let made = make(&e, None).unwrap();
        assert_eq!((made.token.as_str(), made.expires), ("ajt_one", 1_800_000_000));
        assert_eq!(asked.recv().unwrap(), (wire::JOIN_TOKENS.to_owned(), "{}".to_owned()));
        assert_eq!(make(&e, Some("tm_1".into())).unwrap().token, "ajt_two");
        assert_eq!(asked.recv().unwrap().1, r#"{"team":"tm_1"}"#);
        let err = make(&e, Some("tm_2".into())).unwrap_err();
        assert!(format!("{err:#}").contains("only the team's owners and editors"), "{err:#}");
    }
}
