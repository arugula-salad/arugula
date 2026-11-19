//! `arugula feedback "text"`: send the Arugula team a bug, something
//! missing, an idea or praise, through the daemon (the `feedback.send`
//! operation), which adds Arugula's version and the OS. Run by an agent
//! (CLAUDECODE or AI_AGENT set) it's the agent's, which the machine's owner
//! may have turned off; a person's always goes.

use super::Ctx;
use crate::http::{agent, call_raw};
use crate::util::print_json;
use arugula_control_wire::feedback::MAX_TITLE;
use arugula_proto::{
    feedback::{self as said, FeedbackAgentInfo, FeedbackSendRequest},
    op::ops::FeedbackSend,
};

#[derive(clap::Args)]
pub struct Args {
    /// What you have to say. Its first line is the title, unless `--title`
    /// gives one.
    text: String,
    /// What it is.
    #[arg(long, value_enum, default_value = "idea")]
    kind: Kind,
    /// One line on what it's about (at most 200 characters).
    #[arg(long)]
    title: Option<String>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum Kind {
    /// Something in Arugula doesn't work.
    Bug,
    /// Something you needed isn't there.
    Missing,
    Idea,
    Praise,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Bug => "bug",
            Kind::Missing => "missing",
            Kind::Idea => "idea",
            Kind::Praise => "praise",
        }
    }
}

/// The title and body: `--title` and the whole text, or the text's first
/// line (cut to [`MAX_TITLE`] characters) and the rest.
fn split(text: &str, title: Option<&str>) -> (String, String) {
    if let Some(t) = title {
        return (t.trim().to_owned(), text.trim().to_owned());
    }
    let text = text.trim();
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let title: String = first.trim().chars().take(MAX_TITLE).collect();
    // A first line too long for a title stays whole in the body.
    let body = if first.trim().chars().count() > MAX_TITLE { text } else { rest.trim() };
    (title, body.to_owned())
}

/// The agent that runs this, as its environment says: `AI_AGENT`'s name, or
/// Claude Code's.
fn agent_info() -> Option<FeedbackAgentInfo> {
    let var = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty() && v != "0");
    let name = var("AI_AGENT")
        .filter(|v| !["1", "true", "yes"].contains(&v.to_lowercase().as_str()))
        .or_else(|| var("CLAUDECODE").map(|_| "claude-code".to_owned()))?;
    Some(FeedbackAgentInfo { name, version: None, model: None })
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out } = ctx;
    let agent_runs = agent();
    if agent_runs && said::env_off(&|k| std::env::var(k).ok()) {
        anyhow::bail!("{}", said::AGENTS_OFF);
    }
    let (title, body) = split(&args.text, args.title.as_deref());
    if title.is_empty() {
        anyhow::bail!("say what it's about: the text's first line is its title, or give --title");
    }
    let req = FeedbackSendRequest {
        kind: args.kind.word().into(),
        title,
        body,
        contact_ok: false,
        source: Some(if agent_runs { "agent" } else { "person" }.into()),
        surface: Some("cli".into()),
        agent: agent_runs.then(agent_info).flatten(),
        route: None,
    };
    let (sent, v) = call_raw::<FeedbackSend>(&sock, &(), &req)?;
    match json_out {
        true => print_json(&v),
        false => println!("{}", sent.sentence()),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_title() {
        assert_eq!(split("Panes flicker\n\nWhen I resize.\n", None), ("Panes flicker".into(), "When I resize.".into()));
        assert_eq!(split("  Just this  ", None), ("Just this".into(), String::new()));
        assert_eq!(split("All of it\nhere", Some(" Mine ")), ("Mine".into(), "All of it\nhere".into()));
        let long = "x".repeat(MAX_TITLE + 5);
        let (t, b) = split(&long, None);
        assert_eq!((t.chars().count(), b), (MAX_TITLE, long));
    }
}
