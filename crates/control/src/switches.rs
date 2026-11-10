//! The launch switches (#531): what the operator flips in seconds, without
//! a deploy or a restart, when sign-ups or hosted boxes need holding back.
//! Each is a row in the hub's database (`db::switches`), set with
//! `arugula-control switches` on the hub's machine and read by the running
//! hub every second, so it holds across a restart:
//!
//! ```text
//! fly ssh console -a illogical-control -C "/arugula-control switches status --db /data/control.db"
//! ```
//!
//! - **Sign-ups paused** (`signups pause|open`): nobody new gets an
//!   account, by GitHub or a passkey; whoever has one signs in as ever.
//!   GitHub's sign-in shows a page saying so, and the passkey sign-up
//!   (what the page and the CLI's browser use) refuses with
//!   `reason: signups_paused` and the same sentence.
//! - **Sign-ups per address** (`cap signups-per-hour`, [`SIGNUPS_PER_HOUR`]):
//!   new accounts one client address may make in an hour, by either way.
//! - **New hosted boxes** (`hosted open|waitlist|off`): the platform asks
//!   ([`HubPort::standings`](arugula_platform_api::HubPort::standings))
//!   before it makes a cloud workspace's box. `waitlist` (the default) lets
//!   only admitted accounts (`admit`, or the platform's own
//!   `ARUGULA_HOSTED_ACCOUNTS`) make them, and puts whoever else asks on
//!   the waitlist (`waitlist` lists it); `off` lets nobody, and the boxes
//!   there are keep going.
//! - **Per-account caps** (`cap NAME VALUE [--account A]`): live cloud
//!   workspaces made ([`BOXES`]), hours their boxes run a UTC month
//!   ([`BOX_HOURS`]), both the platform's to count, and relayed traffic a
//!   UTC day, past which the account's relayed traffic slows to about
//!   64 KB/s (the relay's, `relay::Ticket::count`; the default is
//!   `--relay-daily-mb`, 10 GB). Each has a default for everyone and may be
//!   set for one account; an account's wins.
//! - **Suspending an account** (`suspend A --by --why`), for abuse: it can't
//!   sign in or use a session, device or machine it has (`Session`,
//!   `verify_daemon`, `HubPort::resolve`), its relay sockets hang up and
//!   new ones are refused, and the platform stops the boxes of cloud
//!   workspaces it made and won't wake them. `unsuspend` undoes it all but
//!   the stopped boxes, which wake when opened.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::Context as _;
use arugula_control_common::port::Standing;
use arugula_control_common::{ApiError, refusal};
use axum::{http::StatusCode, response::Response};
use clap::Parser;
use rusqlite::{Connection, OptionalExtension as _};
use tracing::{info, warn};

use crate::{
    App,
    db::switches::{self as rows, Row},
};

/// Live cloud workspaces an account may have made, unless the operator
/// says otherwise (Jake's "moderate" caps, #531).
pub const BOXES: u64 = 2;
/// Hours an account's cloud workspaces' boxes may run each UTC month.
pub const BOX_HOURS: u64 = 60;
/// New accounts one client address may make an hour.
pub const SIGNUPS_PER_HOUR: u64 = 60;
/// Relayed MB an account may move a UTC day before it slows: the default
/// of `--relay-daily-mb`, which the operator's cap overrides.
pub const RELAY_MB: u64 = 10_000;

/// How often the running hub reads the switches.
const READ_EVERY: Duration = Duration::from_secs(1);

/// The `reason` beside a refused sign-up while they're paused.
pub const SIGNUPS_PAUSED: &str = "signups_paused";
/// The `reason` beside anything a suspended account is refused.
pub const SUSPENDED: &str = "suspended";

/// What a refused sign-up says while sign-ups are paused, unless the
/// operator said something else.
pub const PAUSED_SAY: &str = "Arugula isn't taking new sign-ups right now. Try again in a little while; \
                              if you already have an account, sign in with it as usual.";
/// What a suspended account is told, everywhere.
pub const SUSPENDED_SAY: &str =
    "This Arugula account is suspended. If you think that's a mistake, write to hello@arugula.io.";

/// Who may make new hosted boxes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Hosted {
    /// Any signed-in account, under its caps.
    Open,
    /// Admitted accounts only; whoever else asks goes on the waitlist.
    #[default]
    Waitlist,
    /// Nobody: the boxes there are keep going.
    Off,
}

impl Hosted {
    pub fn as_str(self) -> &'static str {
        match self {
            Hosted::Open => "open",
            Hosted::Waitlist => "waitlist",
            Hosted::Off => "off",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [Hosted::Open, Hosted::Waitlist, Hosted::Off].into_iter().find(|h| h.as_str() == s)
    }
}

/// A cap the operator sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Cap {
    /// Live cloud workspaces an account has made.
    Boxes,
    /// Hours an account's cloud workspaces' boxes run a UTC month.
    BoxHours,
    /// Relayed MB an account moves a UTC day before it slows.
    RelayMb,
    /// New accounts one client address makes an hour (everyone's only).
    SignupsPerHour,
}

impl Cap {
    /// Its row's name.
    fn row(self) -> &'static str {
        match self {
            Cap::Boxes => "cap.boxes",
            Cap::BoxHours => "cap.box-hours",
            Cap::RelayMb => "cap.relay-mb",
            Cap::SignupsPerHour => "cap.signups-per-hour",
        }
    }
}

/// One account's switches that are set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    /// Let in from the waitlist (by whom).
    pub admitted: Option<String>,
    /// Suspended (by whom, and why).
    pub suspended: Option<String>,
    /// On the waitlist since (ms).
    pub waiting: Option<u64>,
    pub boxes: Option<u64>,
    pub box_hours: Option<u64>,
    pub relay_mb: Option<u64>,
}

/// Every switch, as the hub last read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Sign-ups are paused, and what a refused one is told.
    pub signups_paused: Option<String>,
    pub hosted: Hosted,
    pub boxes: u64,
    pub box_hours: u64,
    /// Everyone's relay cap, when the operator set one over
    /// `--relay-daily-mb`.
    pub relay_mb: Option<u64>,
    pub signups_per_hour: u64,
    pub accounts: HashMap<String, Account>,
}

impl State {
    /// Every switch at its default.
    pub fn defaults() -> Self {
        Self {
            signups_paused: None,
            hosted: Hosted::default(),
            boxes: BOXES,
            box_hours: BOX_HOURS,
            relay_mb: None,
            signups_per_hour: SIGNUPS_PER_HOUR,
            accounts: HashMap::new(),
        }
    }

    /// The switches the rows set, over the defaults. A row the hub can't
    /// read (from a newer hub, say) is logged and left at its default.
    pub fn from_rows(global: &[Row], accounts: &[(String, Row)]) -> Self {
        let mut s = Self::defaults();
        let num = |r: &Row| r.value.parse::<u64>().ok();
        for r in global {
            match r.name.as_str() {
                "signups" => s.signups_paused = Some(r.value.clone()),
                "hosted" => match Hosted::parse(&r.value) {
                    Some(h) => s.hosted = h,
                    None => warn!(value = r.value, "a launch switch the hub can't read: hosted"),
                },
                "cap.relay-mb" => s.relay_mb = num(r),
                n => {
                    let slot = match n {
                        "cap.boxes" => &mut s.boxes,
                        "cap.box-hours" => &mut s.box_hours,
                        "cap.signups-per-hour" => &mut s.signups_per_hour,
                        _ => {
                            warn!(name = n, "a launch switch the hub doesn't know");
                            continue;
                        }
                    };
                    match num(r) {
                        Some(v) => *slot = v,
                        None => warn!(name = n, value = r.value, "a launch switch the hub can't read"),
                    }
                }
            }
        }
        for (account, r) in accounts {
            let a = s.accounts.entry(account.clone()).or_default();
            match r.name.as_str() {
                "admitted" => a.admitted = Some(r.value.clone()),
                "suspended" => a.suspended = Some(r.value.clone()),
                "waiting" => a.waiting = Some(r.at),
                "cap.boxes" => a.boxes = num(r),
                "cap.box-hours" => a.box_hours = num(r),
                "cap.relay-mb" => a.relay_mb = num(r),
                n => warn!(name = n, "an account's launch switch the hub doesn't know"),
            }
        }
        s
    }

    fn account(&self, account: &str) -> Option<&Account> {
        self.accounts.get(account)
    }

    /// Whether the account is suspended.
    pub fn suspended(&self, account: &str) -> bool {
        self.account(account).is_some_and(|a| a.suspended.is_some())
    }

    /// The bytes the account may relay a day before it slows (0: no cap),
    /// when the operator set a cap: `None` is `--relay-daily-mb`'s.
    pub fn relay_bytes(&self, account: &str) -> Option<u64> {
        self.account(account).and_then(|a| a.relay_mb).or(self.relay_mb).map(|mb| mb.saturating_mul(1_000_000))
    }

    /// What the platform is told of the account.
    pub fn standing(&self, account: &str) -> Standing {
        let a = self.account(account);
        Standing {
            account: account.to_owned(),
            suspended: a.is_some_and(|a| a.suspended.is_some()),
            hosted: self.hosted.as_str().to_owned(),
            admitted: a.is_some_and(|a| a.admitted.is_some()),
            boxes: a.and_then(|a| a.boxes).unwrap_or(self.boxes),
            box_hours: a.and_then(|a| a.box_hours).unwrap_or(self.box_hours),
        }
    }

    /// The sign-up limit, as the rate limiter counts it.
    pub fn signup_limit(&self) -> (&'static str, usize) {
        (crate::limit::ACCOUNTS.0, usize::try_from(self.signups_per_hour).unwrap_or(usize::MAX))
    }
}

/// The switches as the running hub last read them.
pub struct Switches {
    now: RwLock<Arc<State>>,
}

impl Default for Switches {
    fn default() -> Self {
        Self { now: RwLock::new(Arc::new(State::defaults())) }
    }
}

impl Switches {
    pub fn now(&self) -> Arc<State> {
        self.now.read().unwrap().clone()
    }

    fn set(&self, s: State) -> Arc<State> {
        std::mem::replace(&mut *self.now.write().unwrap(), Arc::new(s))
    }

    /// Refuse a suspended account (`reason: suspended`).
    pub fn check(&self, account: &str) -> Result<(), ApiError> {
        if self.now.read().unwrap().suspended(account) {
            return Err(suspended());
        }
        Ok(())
    }

    /// Refuse a sign-up while they're paused (`reason: signups_paused`).
    pub fn check_signup(&self) -> Result<(), ApiError> {
        match &self.now.read().unwrap().signups_paused {
            Some(say) => Err(refusal(StatusCode::FORBIDDEN, SIGNUPS_PAUSED, paused_say(say))),
            None => Ok(()),
        }
    }
}

/// The `reason` beside a sign-up refused for the limit per address.
pub const SIGNUPS_LIMITED: &str = "signups_limited";

/// Count a new account against the sign-up limit for its client address
/// (`cap signups-per-hour`), or refuse it, saying so.
pub fn count_signup(app: &App, ip: std::net::IpAddr) -> Result<(), ApiError> {
    app.limits.check(app.switches.now().signup_limit(), ip).map_err(|_| {
        refusal(
            StatusCode::TOO_MANY_REQUESTS,
            SIGNUPS_LIMITED,
            "Too many new Arugula accounts were made from this address in the last hour. Try again later.",
        )
    })
}

/// What a paused sign-up says: the operator's words, or [`PAUSED_SAY`].
pub fn paused_say(said: &str) -> &str {
    if said.trim().is_empty() { PAUSED_SAY } else { said }
}

/// What a suspended account is refused with.
pub fn suspended() -> ApiError {
    refusal(StatusCode::FORBIDDEN, SUSPENDED, SUSPENDED_SAY)
}

/// The page GitHub's sign-in ends on when it's refused for a switch.
pub fn page(e: &ApiError) -> Response {
    let title = match e.2 {
        Some(SIGNUPS_PAUSED) => "New sign-ups are paused",
        Some(SUSPENDED) => "This account is suspended",
        Some(SIGNUPS_LIMITED) => "Too many new accounts from here",
        _ => "Sign-in didn't go through",
    };
    let body = format!(
        "<h1>{}</h1><p>{}</p><p class=\"dim\"><a href=\"/\" style=\"color:inherit\">Back to Arugula</a></p>",
        crate::issuer::escape(title),
        crate::issuer::escape(&e.1)
    );
    crate::hud_box::page(e.0, title, &body)
}

/// Read the switches from the database now: what the running hub does
/// every second. A newly suspended account's relay sockets hang up.
pub fn refresh(app: &App) -> anyhow::Result<()> {
    let (global, accounts) = app.db.switches()?;
    let now = State::from_rows(&global, &accounts);
    if *app.switches.now() == now {
        return Ok(());
    }
    let was = app.switches.set(now.clone());
    log_changes(&was, &now);
    for (account, a) in &now.accounts {
        if a.suspended.is_some() && !was.suspended(account) {
            app.relay.account_gone(account);
        }
    }
    Ok(())
}

fn log_changes(was: &State, now: &State) {
    if was.signups_paused != now.signups_paused {
        match &now.signups_paused {
            Some(_) => info!("launch switches: sign-ups paused"),
            None => info!("launch switches: sign-ups open"),
        }
    }
    if was.hosted != now.hosted {
        info!(hosted = now.hosted.as_str(), "launch switches: new hosted boxes");
    }
    if (was.boxes, was.box_hours, was.relay_mb, was.signups_per_hour)
        != (now.boxes, now.box_hours, now.relay_mb, now.signups_per_hour)
    {
        info!(
            boxes = now.boxes,
            box_hours = now.box_hours,
            relay_mb = ?now.relay_mb,
            signups_per_hour = now.signups_per_hour,
            "launch switches: caps changed"
        );
    }
    for (account, a) in &now.accounts {
        let before = was.accounts.get(account);
        if a.suspended.is_some() && before.is_none_or(|b| b.suspended.is_none()) {
            warn!(%account, "launch switches: account suspended");
        }
        if a.admitted.is_some() && before.is_none_or(|b| b.admitted.is_none()) {
            info!(%account, "launch switches: account admitted");
        }
    }
    for (account, b) in &was.accounts {
        if b.suspended.is_some() && now.accounts.get(account).is_none_or(|a| a.suspended.is_none()) {
            info!(%account, "launch switches: account no longer suspended");
        }
    }
}

/// Read the switches every [`READ_EVERY`].
pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = refresh(&app) {
                warn!(error = %e, "reading the launch switches");
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    });
}

/// `HubPort::standings`: each account's standing, and `waiting` puts the
/// one asked about on the waitlist when it isn't admitted.
pub fn standings(app: &App, accounts: &[String], waiting: bool) -> anyhow::Result<Vec<Standing>> {
    let s = app.switches.now();
    let mut out = Vec::new();
    for a in accounts {
        if app.db.account(a)?.is_none() {
            continue;
        }
        let st = s.standing(a);
        if waiting && accounts.len() == 1 && !st.admitted && !st.suspended && s.hosted == Hosted::Waitlist {
            app.db.wait_for_box(a, arugula_e2e::now_ms())?;
        }
        out.push(st);
    }
    Ok(out)
}

// ---------------------------------------------------------------- the operator's command

#[derive(Parser, Debug)]
#[command(
    name = "arugula-control switches",
    about = "The launch switches (#531): pause sign-ups, hold hosted boxes, set caps, suspend accounts"
)]
struct Args {
    #[command(subcommand)]
    what: What,
    /// Control's database, which the running hub reads the switches from.
    #[arg(long, global = true, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// Every switch, and one account's when it's named.
    Status {
        /// An account id, or `github:LOGIN`.
        account: Option<String>,
    },
    /// Pause sign-ups, or open them again.
    Signups {
        #[command(subcommand)]
        to: Signups,
    },
    /// Who may make new hosted boxes: `open` (anyone), `waitlist`
    /// (admitted accounts only; the default) or `off` (nobody; those made
    /// keep going).
    Hosted { to: Hosted },
    /// Set a cap, for everyone or (with --account) for one account.
    /// `default` puts it back.
    Cap {
        name: Cap,
        /// A whole number, or `default`.
        value: String,
        /// One account's (an id, or `github:LOGIN`), not everyone's.
        #[arg(long)]
        account: Option<String>,
    },
    /// Let accounts in from the waitlist: they may make hosted boxes.
    Admit {
        #[arg(required = true)]
        accounts: Vec<String>,
        /// Who lets them in, for the record.
        #[arg(long, default_value = "operator")]
        by: String,
    },
    /// Take an admission back (their boxes stay).
    Unadmit { account: String },
    /// Who asked for a hosted box and wasn't admitted, first first.
    Waitlist,
    /// Suspend an account for abuse: no sign-in, no relay, its boxes stop.
    Suspend {
        account: String,
        /// Who suspends it, for the record.
        #[arg(long)]
        by: String,
        /// Why, for the record.
        #[arg(long)]
        why: String,
    },
    /// Lift a suspension.
    Unsuspend { account: String },
}

#[derive(clap::Subcommand, Debug)]
enum Signups {
    /// Nobody new gets an account; people with one sign in as ever.
    Pause {
        /// What a refused sign-up is told, in place of the usual sentence.
        #[arg(long)]
        message: Option<String>,
    },
    /// Sign-ups are taken again.
    Open,
}

/// An account by its id, or `github:LOGIN` (its GitHub sign-in's login).
fn account(c: &Connection, who: &str) -> anyhow::Result<String> {
    let found: Option<String> = match who.strip_prefix("github:") {
        Some(login) => c
            .query_row(
                "SELECT account FROM identities WHERE provider = 'github' AND lower(login) = lower(?1)",
                [login],
                |r| r.get(0),
            )
            .optional()?,
        None => c.query_row("SELECT id FROM accounts WHERE id = ?1", [who], |r| r.get(0)).optional()?,
    };
    found.with_context(|| format!("no account {who}"))
}

/// `--relay-daily-mb`, as the hub's machine has it set.
fn relay_mb_default() -> u64 {
    std::env::var("ARUGULA_RELAY_DAILY_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(RELAY_MB)
}

fn state(c: &Connection) -> anyhow::Result<State> {
    Ok(State::from_rows(&rows::global(c)?, &rows::accounts(c)?))
}

/// The day an instant was, `YYYY-MM-DD` (UTC).
fn when(ms: u64) -> String {
    crate::day(ms)
}

fn status(c: &Connection, who: Option<&str>) -> anyhow::Result<String> {
    let s = state(c)?;
    let mut out = String::new();
    match &s.signups_paused {
        Some(say) => out += &format!("sign-ups: paused (\"{}\")\n", paused_say(say)),
        None => out += "sign-ups: open\n",
    }
    out += &format!("new hosted boxes: {}\n", s.hosted.as_str());
    out += &format!(
        "caps: boxes {}, box-hours {} a month, relay-mb {} a day, signups-per-hour {} per address\n",
        s.boxes,
        s.box_hours,
        s.relay_mb.unwrap_or_else(relay_mb_default),
        s.signups_per_hour
    );
    let mut suspended: Vec<_> = s.accounts.iter().filter(|(_, a)| a.suspended.is_some()).collect();
    suspended.sort_by_key(|(id, _)| *id);
    out += &format!("suspended: {}\n", suspended.len());
    for (id, a) in suspended {
        out += &format!("  {id}  {}\n", a.suspended.as_deref().unwrap_or_default());
    }
    let admitted = s.accounts.values().filter(|a| a.admitted.is_some()).count();
    let waiting = s.accounts.values().filter(|a| a.waiting.is_some() && a.admitted.is_none()).count();
    out += &format!("admitted: {admitted}, waiting: {waiting}\n");
    if let Some(who) = who {
        let id = account(c, who)?;
        let st = s.standing(&id);
        let a = s.accounts.get(&id).cloned().unwrap_or_default();
        out += &format!(
            "account {id}: {}{}, boxes {}, box-hours {}, relay-mb {}\n",
            if st.suspended { "suspended, " } else { "" },
            match (&a.admitted, a.waiting) {
                (Some(by), _) => format!("admitted by {by}"),
                (None, Some(at)) => format!("waiting since {}", when(at)),
                (None, None) => "not admitted".to_owned(),
            },
            st.boxes,
            st.box_hours,
            s.relay_bytes(&id).map_or_else(relay_mb_default, |b| b / 1_000_000),
        );
    }
    Ok(out)
}

/// `arugula-control switches ...`.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control switches".to_owned()).chain(args));
    // The hub's file, never a new one.
    let c = Connection::open_with_flags(
        &a.db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", a.db.display()))?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.execute_batch(rows::SCHEMA)?;
    print!("{}", command(&c, a.what)?);
    Ok(())
}

/// Do what was asked, and say what it did.
fn command(c: &Connection, what: What) -> anyhow::Result<String> {
    let now = arugula_e2e::now_ms();
    let soon = "the running hub takes it within a second";
    Ok(match what {
        What::Status { account } => status(c, account.as_deref())?,
        What::Signups { to: Signups::Pause { message } } => {
            let say = message.unwrap_or_default();
            rows::set(c, "signups", Some(say.trim()), now)?;
            format!("sign-ups paused ({soon}): new accounts are told \"{}\"\n", paused_say(&say))
        }
        What::Signups { to: Signups::Open } => {
            rows::set(c, "signups", None, now)?;
            format!("sign-ups open ({soon})\n")
        }
        What::Hosted { to } => {
            rows::set(c, "hosted", (to != Hosted::default()).then_some(to.as_str()), now)?;
            format!("new hosted boxes: {} ({soon})\n", to.as_str())
        }
        What::Cap { name, value, account: who } => {
            let v = match value.as_str() {
                "default" => None,
                v => Some(v.parse::<u64>().context("a cap is a whole number, or `default`")?),
            };
            let shown = v.map_or("its default".to_owned(), |v| v.to_string());
            match who {
                Some(who) => {
                    anyhow::ensure!(name != Cap::SignupsPerHour, "signups-per-hour is everyone's: no --account");
                    let id = account(c, &who)?;
                    rows::set_account(c, &id, name.row(), v.map(|v| v.to_string()).as_deref(), now)?;
                    format!("{} for {id}: {shown} ({soon})\n", name.row().trim_start_matches("cap."))
                }
                None => {
                    rows::set(c, name.row(), v.map(|v| v.to_string()).as_deref(), now)?;
                    format!("{} for everyone: {shown} ({soon})\n", name.row().trim_start_matches("cap."))
                }
            }
        }
        What::Admit { accounts, by } => {
            let mut out = String::new();
            for who in accounts {
                let id = account(c, &who)?;
                rows::set_account(c, &id, "admitted", Some(by.trim()), now)?;
                rows::set_account(c, &id, "waiting", None, now)?;
                out += &format!("admitted {id}: it may make hosted boxes, under its caps ({soon})\n");
            }
            out
        }
        What::Unadmit { account: who } => {
            let id = account(c, &who)?;
            rows::set_account(c, &id, "admitted", None, now)?;
            format!("{id} is no longer admitted: it makes no new hosted boxes, and keeps those it has ({soon})\n")
        }
        What::Waitlist => {
            let s = state(c)?;
            let mut w: Vec<_> = s
                .accounts
                .iter()
                .filter(|(_, a)| a.admitted.is_none())
                .filter_map(|(id, a)| Some((a.waiting?, id)))
                .collect();
            w.sort();
            let mut out = format!("waiting: {}\n", w.len());
            for (at, id) in w {
                let login: Option<String> = c
                    .query_row(
                        "SELECT login FROM identities WHERE account = ?1 AND provider = 'github' LIMIT 1",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()?;
                out += &format!(
                    "  {id}  since {}{}\n",
                    when(at),
                    login.map(|l| format!("  github:{l}")).unwrap_or_default()
                );
            }
            out
        }
        What::Suspend { account: who, by, why } => {
            anyhow::ensure!(
                !by.trim().is_empty() && !why.trim().is_empty(),
                "--by and --why are for the record: say both"
            );
            let id = account(c, &who)?;
            rows::set_account(c, &id, "suspended", Some(&format!("{}: {}", by.trim(), why.trim())), now)?;
            format!(
                "suspended {id} ({soon}): it can't sign in, its relay sockets hang up, and the platform stops the \
                 boxes of the cloud workspaces it made\n"
            )
        }
        What::Unsuspend { account: who } => {
            let id = account(c, &who)?;
            rows::set_account(c, &id, "suspended", None, now)?;
            format!("{id} is no longer suspended ({soon}); its stopped boxes wake when opened\n")
        }
    })
}

#[cfg(test)]
#[path = "switches_tests.rs"]
mod tests;
