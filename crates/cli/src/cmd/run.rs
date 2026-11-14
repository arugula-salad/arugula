//! `arugula run`: start a command in a new tab or split.

use super::Ctx;
use crate::http::{self, request, request_as};
use crate::util::{Pane, REMOTE, env_pane, print_json};
use anyhow::bail;
use arugula_proto::{
    Policy,
    api::{RunRequest, RunResponse, WaitResult},
};

fn policy(s: &str) -> anyhow::Result<Policy> {
    Ok(match s {
        "shell" => Policy::Shell,
        "none" => Policy::None,
        "rerun" => Policy::Rerun { confirm: false },
        "rerun-ask" => Policy::Rerun { confirm: true },
        "resume" => Policy::Resume,
        h if h.starts_with("hook:") => Policy::Hook { command: h[5..].to_owned() },
        _ => bail!("policy: shell, none, rerun, rerun-ask, resume or hook:COMMAND"),
    })
}

/// The shell command line for `run`: a single argument as written, several
/// as words, each quoted if it needs to be.
fn shell_command(argv: &[String]) -> String {
    if let [one] = argv {
        return one.clone();
    }
    let safe = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b));
    argv.iter()
        .map(|w| if safe(w) { w.clone() } else { format!("'{}'", w.replace('\'', r"'\''")) })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(clap::Args)]
pub struct Args {
    /// Session name or id (created if missing).
    #[arg(long)]
    session: Option<String>,
    /// Split this pane instead of opening a tab.
    #[arg(long)]
    split: Option<Pane>,
    /// With --split: run where that pane runs instead of this host.
    #[arg(long, requires = "split")]
    join: bool,
    /// Where it starts (with --join: a directory there).
    #[arg(long)]
    cwd: Option<String>,
    /// After a restart: shell, none, rerun, rerun-ask, or hook:COMMAND.
    #[arg(long)]
    policy: Option<String>,
    /// Wait for it to finish and exit with its exit code.
    #[arg(long)]
    wait: bool,
    /// On a new throwaway VM, deleted when the pane closes. Without a
    /// command: a shell on one.
    #[arg(long, hide = true)]
    vm: bool,
    /// In a new tab whose panes share one throwaway VM (splits join it),
    /// deleted when the tab closes.
    #[arg(long, hide = true, conflicts_with_all = ["vm", "split"])]
    vm_tab: bool,
    /// The VM's image (with --vm or --vm-tab).
    #[arg(long, hide = true)]
    image: Option<String>,
    /// On a sandbox that exists (`arugula sandboxes`), over a plain
    /// exec with no daemon there: disposable, and the sandbox stays
    /// when the pane closes. Without a command: a shell.
    #[arg(long, hide = true, conflicts_with_all = ["vm", "vm_tab", "image"])]
    sandbox: Option<String>,
    /// One argument is a shell command line (`'make && ./app'`);
    /// several are a program and its arguments, quoted as given. None
    /// (with --cwd or --join): a shell.
    #[arg(trailing_var_arg = true, required_unless_present_any = ["vm", "vm_tab", "sandbox", "join", "cwd"])]
    command: Vec<String>,
}

pub fn run(args: Args, ctx: Ctx) -> anyhow::Result<i32> {
    let Ctx { sock, json_out, .. } = ctx;
    let Args { session, split, join, cwd, policy: pol, wait, vm, vm_tab, image, sandbox, command } = args;
    if image.is_some() && !vm && !vm_tab {
        anyhow::bail!("--image is for --vm or --vm-tab");
    }
    // A VM (or another daemon's host) has none of this host's
    // directories.
    let cwd = if vm || vm_tab || join || sandbox.is_some() || REMOTE.load(std::sync::atomic::Ordering::Relaxed) {
        cwd
    } else {
        cwd.or_else(|| std::env::current_dir().ok().map(|d| d.display().to_string()))
    };
    let body = RunRequest {
        command: (!command.is_empty()).then(|| shell_command(&command)),
        vm,
        vm_tab,
        image,
        sandbox,
        session,
        split: split.map(|p| p.0),
        join,
        cwd,
        policy: pol.as_deref().map(policy).transpose()?,
        from_pane: env_pane(),
        project: None,
        issue: None,
    };
    let (RunResponse { pane }, v) = request_as(&sock, "POST", "/api/run", &body)?.parse_raw()?;
    if json_out {
        print_json(&v);
    } else {
        println!("%{pane}");
    }
    if wait {
        return exit_code(&sock, pane);
    }
    Ok(0)
}

/// `--wait`: the pane's exit code (1 when it ended some other way).
fn exit_code(sock: &http::Target, pane: u32) -> anyhow::Result<i32> {
    let w: WaitResult = request(sock, "GET", &format!("/api/panes/{pane}/wait?until=exit"), None)?.parse()?;
    Ok(match w {
        WaitResult::Exit { code } => code.unwrap_or(1),
        _ => 1,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_quotes_words() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            super::policy("hook:claude --continue").unwrap(),
            arugula_proto::Policy::Hook { command: "claude --continue".into() }
        );
        assert_eq!(super::shell_command(&v(&["make && ./app"])), "make && ./app");
        assert_eq!(super::shell_command(&v(&["make", "test"])), "make test");
        assert_eq!(super::shell_command(&v(&["bash", "-c", "echo hi; exit 3"])), "bash -c 'echo hi; exit 3'");
        assert_eq!(super::shell_command(&v(&["echo", "it's"])), r"echo 'it'\''s'");
    }
}
