//! Named flags in the daemon's config (#464, #665): what a machine has
//! turned on that a stranger doesn't get, one flag to a Labs feature.
//!
//! They live in one file in the state dir, `flags.json`:
//!
//! ```json
//! { "flags": { "vms": true } }
//! ```
//!
//! - **The registry is [`FLAGS`].** A flag is a row there: its name, a
//!   sentence about it and its default. A name that isn't a row is an error
//!   to [`set`] and off to [`get`]. A feature of #347 adds a row and gates
//!   itself on `get`; nothing here changes.
//! - **Reads are never cached.** [`get`] reads the file on every call, so a
//!   flag flips with no restart, whoever changed it (the daemon for the web's
//!   switch, `arugulad flags` from another process). The file is small and
//!   the callers are a request, not a loop.
//! - **A write keeps what it doesn't know.** [`set`] reads the file as it is
//!   now, changes one flag and renames a new file over it, so keys and flags
//!   an older or newer daemon wrote survive. Two writers at once can lose the
//!   one that renames first; they are a person at a switch and a person at a
//!   terminal, and the loser sees the flag as it now is.
//! - **A file that doesn't parse is not an emergency.** It is logged once and
//!   read as "every flag at its default"; only an explicit [`set`] replaces
//!   it, and keeps the old one beside it as `flags.json.bad`.
//! - **`labs` is every flag.** Before each feature had a flag there was one
//!   switch, `labs` in `flags.json` (#464). It still means "on" for every
//!   flag the file doesn't itself name, so a machine that had Labs keeps all
//!   of it, and turning one feature off there leaves the rest. Nothing
//!   offers `labs` as a flag any more. (The empty `labs` file of #385 before
//!   it was moved into `flags.json` by daemons 0.29 to 0.44, and is no longer
//!   read.)
//! - **The file's presence is the owner asking.** A machine with a
//!   `flags.json` has asked for Developer settings
//!   ([`unlocked`]), and its menus offer them; a new install's don't. So
//!   has one control sends flags to.
//! - **Control pushes, the machine overrides (#347).** A machine joined to
//!   control keeps what control says for its account and its team in a
//!   second file, `flags.control.json` ([`CONTROL_FILE`]), which only the
//!   daemon writes, from control's answer ([`write_control`]), and removes
//!   when it leaves control. A flag is, first match wins: what `flags.json`
//!   says of it (by name, then `labs`); what control
//!   says for the machine's account; what control says for the machine's
//!   team; its default. Account before team because it's the narrower say:
//!   the account owns the machine, a team only reaches the machines in it.
//!   So each flag is three-way in `flags.json`: on, off, or not named,
//!   which follows control ([`unset`]). A [`Flag::local_only`] one
//!   (`agent-delegation`) ignores control: only `flags.json` sets it.
//! - **A feature that leaves Labs takes its row with it.** Its name moves to
//!   [`RETIRED`]: a file or a control push that still names it is read as
//!   it always was, as a name that isn't a flag, and nothing follows, while
//!   [`set`] and [`unset`] say why it's gone. `fountain` was the first
//!   (#942, F5).
//! - **A renamed flag answers to its old name forever.** [`ALIASES`] maps
//!   each old name to the new (aru-117): in `flags.json`, in a control push,
//!   and to [`get`], [`set`] and [`unset`] (so `PUT /api/flags/vms` and
//!   `arugulad flags vms on` work), so no machine loses a setting. A flag's
//!   own name wins over an old one where both are said. Only new names are
//!   listed anywhere.

use std::{
    collections::BTreeMap,
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The file in the state dir that holds the flags.
pub const FILE: &str = "flags.json";

/// What control says for this machine (#347), written by the daemon from
/// control's answer. The owner's word is [`FILE`]; nobody edits this one.
pub const CONTROL_FILE: &str = "flags.control.json";

/// The key that meant all of Labs before each feature had a flag (#464):
/// in the file it still stands for every flag the file doesn't name. Not a
/// flag in [`FLAGS`].
pub const LABS: &str = "labs";

/// VM tabs and panes, machines and the sandboxes page, through the
/// machine's own sandbox provider. Was `vms`.
pub const SANDBOX_PROVIDERS: &str = "sandbox-providers";
/// Studio app blocks.
pub const STUDIO: &str = "studio";
/// Chant workspace blocks. Was `workspaces`.
pub const CHANT_WORKSPACES: &str = "chant-workspaces";
/// Forgejo and GitLab blocks, beside GitHub's. Was `forges`.
pub const ADDITIONAL_FORGES: &str = "additional-forges";
/// Agent recipes offered from this machine, the team's agent catalog and
/// A2A tasks. Was `agents`.
pub const AGENT_DELEGATION: &str = "agent-delegation";
/// Boards: a project's boards and epics, which are control's.
pub const BOARDS: &str = "boards";
/// Live presence: teammates' live faces on their avatars across Arugula,
/// from arugula.stream (Arugula #341), which needs control.
pub const LIVE_PRESENCE: &str = "live-presence";

/// What every agent is told about Docs (#1486), in one wording: the MCP
/// server's instructions carry it, and so does the context `arugula hook`
/// adds on `SessionStart`. Docs have no flag since they left Labs (native
/// #204), so neither does this. Claude Code's own Artifact tool
/// invites it to publish anything meant for other people, so this names the
/// competitor. The plugin's skill says the same in its voice.
pub const DOCS_STEERING: &str = "In Arugula, a report, plan or write-up longer than a screen goes in a Doc (the arugula `doc` tool), not an Artifact or another tool's page: the user watches it fill in beside your pane. When they want to show it to someone, ask with `doc share`: the user decides, and you get the link back.";

/// What every agent is told about filing issues, in one wording: the MCP
/// server's instructions carry it, `arugula hook` adds it on `SessionStart`
/// and says it again before a `gh issue create`. Native issues (M200) have
/// no flag, so neither does this. The plugin's skill says the same in its
/// voice.
pub const ISSUES_STEERING: &str = "In Arugula, a new issue (a bug, a follow-up, a ticket for later) goes in Arugula's own issues with issue_write (kind create), not `gh issue create` or a GitHub draft, unless the user asks for GitHub; forge_read (kind native_issues) lists them.";

/// What agents are told about boards, in one wording: the MCP server's
/// instructions carry it (`arugulad-mcp`'s `instructions()`). Boards are a
/// Labs flag per account, so it is conditional on purpose and says nothing
/// useful, and does no harm, where they're off. `arugula hook` doesn't give
/// it. The plugin's skill says the same in its voice.
pub const BOARDS_STEERING: &str = "When you plan work of more than one ticket in a project, run it on a board: make one with board_write (with a goal) unless a fitting one is there (board_read lists them), add each ticket as a row, and put rows in groups when the work has phases or waves. Don't set statuses or add a row for a PR: a row's lane follows its work, and a PR that names the issue (Arugula #N, Closes #N, (#N)) attaches to the issue's row, and `Closes Arugula #N` (or Fixes, Resolves) in a PR's title or body closes a native issue when the PR merges, where a bare `Arugula #N` only links; attach an agent block, run or proof with board_write attach. Keep a row's note as its one-line status. When the goal is met, close the board with a note saying what a person can now do.";

/// One flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    /// What `arugulad flags` and the file call it.
    pub name: &'static str,
    /// What Developer settings calls it.
    pub title: &'static str,
    /// One sentence, for the list in `arugulad flags` and Settings.
    pub about: &'static str,
    /// What it is until someone sets it.
    pub default: bool,
    /// Only this machine's `flags.json` sets it (#347): control's word on
    /// it, for the account or the team, is ignored. For what opens a way
    /// in or acts for the machine (offering agents), which the
    /// machine's owner turns on there or not at all. (Guest ssh was one;
    /// it is a sharing setting now, `crate::sharing`, native #205.)
    pub local_only: bool,
}

const fn row(name: &'static str, title: &'static str, about: &'static str) -> Flag {
    Flag { name, title, about, default: false, local_only: false }
}

const fn local_row(name: &'static str, title: &'static str, about: &'static str) -> Flag {
    Flag { local_only: true, ..row(name, title, about) }
}

/// Every flag there is. A new one is a row here.
pub const FLAGS: &[Flag] = &[
    row(
        SANDBOX_PROVIDERS,
        "External sandbox providers",
        "VM tabs and panes, machines and the sandboxes page, from your own sandbox provider. Cloud workspaces don't need it.",
    ),
    row(STUDIO, "Studio apps", "Studio app blocks. Needs a linked studio."),
    row(
        CHANT_WORKSPACES,
        "Chant workspaces",
        "Chant workspace blocks: a workspace's members, relationships and gates.",
    ),
    row(ADDITIONAL_FORGES, "GitLab and Forgejo", "PR and issue blocks for GitLab and Forgejo, beside GitHub's."),
    local_row(
        AGENT_DELEGATION,
        "Local agent delegation",
        "Agent recipes, offering them from this machine, your team's agent catalog and tasks between machines.",
    ),
    row(BOARDS, "Boards", "A project's boards and epics, which need control."),
    row(
        LIVE_PRESENCE,
        "Live presence",
        "Your teammates' live faces on their avatars across Arugula, and yours when you choose Live. Needs control.",
    ),
];

/// Flags that were, and why they aren't: a feature that left Labs. A file
/// or a control push may still name one (an older daemon wrote it, a
/// machine turned it off); it's ignored, as any name not in [`FLAGS`] is,
/// and kept. Only [`set`] and [`unset`] say anything: this.
pub const RETIRED: &[(&str, &str)] = &[
    (
        "fountain",
        "Direct Managoat execution is retired: agents in the cloud run on Cloud workspaces in your Arugula projects",
    ),
    ("huddles", "Huddles (voice calls on a session) were removed from Arugula"),
    // Graduated (native #203): chat is control's, on where a team's owners
    // turn it on there; a machine's old threads are no longer brought in.
    ("chat", "Chat is no longer in Labs: it's on wherever a team's owners turn it on in control (Teams)"),
    // Graduated, not removed: every swarm offers its four views now.
    ("swarm-themes", "The swarm's views are no longer in Labs: every swarm offers them under Theme"),
    // Graduated (native #204): Doc blocks, the doc tool and the Docs page.
    // Sharing one is still the owner's tap, or a card that asks them.
    ("docs", "Docs are no longer in Labs: every machine has Doc blocks, the doc tool and the Docs page"),
    // Graduated to a sharing setting (native #205), which reads this name
    // once to keep a machine's choice ([`said_here`], `crate::sharing`).
    (
        "guest-ssh",
        "Guest SSH access is a sharing setting now, off until the machine's owner turns it on: `arugulad sharing guest-ssh on`, or Guest SSH access… in the machine menu",
    ),
    // Never a flag, but the name it would have now.
    (
        "managoat",
        "Direct Managoat execution is retired: agents in the cloud run on Cloud workspaces in your Arugula projects",
    ),
];

/// Flags that were renamed (aru-117, 2026-10-26): the old name, then the
/// new. An old name is read forever, wherever a name comes in (the file, a
/// control push, [`get`], [`set`], [`unset`]), and never listed.
pub const ALIASES: &[(&str, &str)] = &[
    ("vms", SANDBOX_PROVIDERS),
    ("workspaces", CHANT_WORKSPACES),
    ("forges", ADDITIONAL_FORGES),
    ("agents", AGENT_DELEGATION),
];

/// A flag's name now: `name`, unless it's an old name in [`ALIASES`].
pub fn canonical(name: &str) -> &str {
    ALIASES.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| *new)
}

/// The names a flag is known by: its own, then its old ones.
fn names_of(name: &'static str) -> impl Iterator<Item = &'static str> {
    std::iter::once(name).chain(ALIASES.iter().filter(move |(_, new)| *new == name).map(|(old, _)| *old))
}

/// What to say of `name` if it is a [`RETIRED`] flag.
pub fn retired(name: &str) -> Option<String> {
    RETIRED.iter().find(|(n, _)| *n == name).map(|(_, why)| format!("{name} isn't a flag any more: {why}"))
}

/// The flags that are on, from one read: what a listing or a `--help` is
/// built from, where [`get`] would read the file once per flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct On(u32);

impl On {
    pub fn none() -> On {
        On(0)
    }

    pub fn all() -> On {
        On((1 << FLAGS.len()) - 1)
    }

    /// Whether `name` (or a flag it's an old name of) is on; a name that
    /// isn't in [`FLAGS`] is off.
    pub fn has(self, name: &str) -> bool {
        let name = canonical(name);
        FLAGS.iter().position(|f| f.name == name).is_some_and(|i| self.0 & (1 << i) != 0)
    }

    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// Their names, in [`FLAGS`]' order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        FLAGS.iter().enumerate().filter(move |(i, _)| self.0 & (1 << i) != 0).map(|(_, f)| f.name)
    }
}

/// The flags that are on in `state_dir`. Read on every call.
pub fn on(state_dir: &Path) -> On {
    let bits = explain(state_dir).iter().enumerate().filter(|(_, (_, s))| s.on).fold(0, |b, (i, _)| b | (1 << i));
    On(bits)
}

/// Whether `state_dir`'s owner has asked for Developer settings: it has a
/// flags file. Or control sends it flags
/// (#347), so its owner can see where they come from and override them.
pub fn unlocked(state_dir: &Path) -> bool {
    state_dir.join(FILE).exists() || !read_control(state_dir).is_empty()
}

/// What control says for this machine (#347): its account's flags and,
/// for a team's machine, its team's, by name. A name that isn't in
/// [`FLAGS`] is kept and means nothing here; an old name in [`ALIASES`]
/// speaks for its flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pushed {
    #[serde(default)]
    pub account: BTreeMap<String, bool>,
    #[serde(default)]
    pub team: BTreeMap<String, bool>,
}

impl Pushed {
    pub fn is_empty(&self) -> bool {
        self.account.is_empty() && self.team.is_empty()
    }
}

/// Where a flag's state comes from (#347), first match wins: this
/// machine's `flags.json`, control for the machine's account, control for
/// its team, else the flag's default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "FlagSource"))]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Default,
    Team,
    Account,
    Machine,
}

/// One flag's state, and each say in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub on: bool,
    pub source: Source,
    /// This machine's word (`flags.json` by name, `labs` there, or the old
    /// marker); `None` follows control.
    pub local: Option<bool>,
    /// Control's word for the machine's account.
    pub account: Option<bool>,
    /// Control's word for the machine's team.
    pub team: Option<bool>,
}

/// Every flag with its state and where it comes from, from one read of
/// each file.
pub fn explain(state_dir: &Path) -> Vec<(Flag, State)> {
    let doc = read(state_dir);
    let pushed = read_control(state_dir);
    FLAGS.iter().map(|f| (*f, doc.state(&pushed, f))).collect()
}

/// [`CONTROL_FILE`] as it is: empty if there's none or it doesn't parse
/// (it's the daemon's alone, and the next refresh writes it again).
fn read_control(dir: &Path) -> Pushed {
    fs::read_to_string(dir.join(CONTROL_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// What control last said of `name` for this machine, flag or not: its
/// account's word, else its team's. For control's own settings that ride
/// along with the flags, as chat's on/off does (native #203); a machine's
/// `flags.json` has no say in them.
pub fn control_says(state_dir: &Path, name: &str) -> Option<bool> {
    let pushed = read_control(state_dir);
    pushed.account.get(name).or_else(|| pushed.team.get(name)).copied()
}

/// Keeps what control says for this machine (#347); `true` if it changed.
/// Nothing pushed removes the file, so a machine control says nothing to
/// is as it was before it joined.
pub fn write_control(state_dir: &Path, pushed: &Pushed) -> io::Result<bool> {
    if pushed.is_empty() {
        return forget_control(state_dir);
    }
    if read_control(state_dir) == *pushed {
        return Ok(false);
    }
    let mut text = serde_json::to_string_pretty(pushed).expect("maps of bools serialize");
    text.push('\n');
    write_atomic(&state_dir.join(CONTROL_FILE), text.as_bytes())?;
    Ok(true)
}

/// Drops what control said (#347): the machine left control, and only its
/// own `flags.json` counts again. `true` if there was a file.
pub fn forget_control(state_dir: &Path) -> io::Result<bool> {
    match fs::remove_file(state_dir.join(CONTROL_FILE)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The flag `name` names, by its name or an old one in [`ALIASES`].
pub fn lookup(name: &str) -> Option<&'static Flag> {
    let name = canonical(name);
    FLAGS.iter().find(|f| f.name == name)
}

/// `GET /api/flags`: one row per flag (the `flags.list` operation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FlagInfo {
    pub name: String,
    /// What Developer settings calls it. Absent from a daemon before #665.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<String>", optional))]
    pub title: String,
    pub about: String,
    /// Whether it is on here now.
    pub on: bool,
    pub default: bool,
    /// Whether this build has what the flag turns on. A build without Labs
    /// can set a flag, and nothing follows; Settings says so. Absent from
    /// daemons that don't report it, which have it.
    #[serde(default = "yes")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub built: bool,
    /// Where `on` comes from (#347). Absent from a daemon before it, whose
    /// flags are all this machine's or their defaults.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Option<Source>", optional))]
    pub source: Source,
    /// This machine's word on it; absent: it follows control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub local: Option<bool>,
    /// Control's word for the machine's account, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub account: Option<bool>,
    /// Control's word for the machine's team, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub team: Option<bool>,
    /// Only this machine sets it; control's word is ignored
    /// ([`Flag::local_only`]). Absent: control may set it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub local_only: bool,
}

fn yes() -> bool {
    true
}

/// `PUT /api/flags/{name}`: the body (the `flag.set` operation). `null`
/// takes this machine's word back, so the flag follows control (#347);
/// only a daemon that reports [`FlagInfo::source`] takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FlagSetRequest {
    pub on: Option<bool>,
}

/// The file as the last read found it.
enum Doc {
    /// No file, or an empty one.
    Missing,
    Ok(Map<String, Value>),
    /// Not JSON, or not an object, or `flags` isn't one.
    Malformed,
}

fn read(dir: &Path) -> Doc {
    let text = match fs::read_to_string(dir.join(FILE)) {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) | Err(_) => return Doc::Missing,
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) if m.get("flags").is_none_or(Value::is_object) => Doc::Ok(m),
        _ => {
            // Once per process: a request loop would say it every time.
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                tracing::warn!(file = %dir.join(FILE).display(), "flags.json isn't a flags file: every flag is at its default");
            }
            Doc::Malformed
        }
    }
}

impl Doc {
    /// What the file says about `name`, if it says.
    fn says(&self, name: &str) -> Option<bool> {
        match self {
            Doc::Ok(m) => m.get("flags")?.get(name)?.as_bool(),
            Doc::Missing | Doc::Malformed => None,
        }
    }

    /// This machine's word on a flag: the file's on it (by name, else by
    /// an old name), else its word on `labs` (which was all of them).
    fn local(&self, f: &Flag) -> Option<bool> {
        names_of(f.name).find_map(|n| self.says(n)).or_else(|| self.says(LABS))
    }

    /// A flag's state: this machine's word, else control's for the
    /// account, else control's for the team, else its default.
    fn state(&self, pushed: &Pushed, f: &Flag) -> State {
        let local = self.local(f);
        // A local-only flag doesn't hear control at all.
        let pushed_for =
            |m: &BTreeMap<String, bool>| names_of(f.name).find_map(|n| m.get(n).copied()).filter(|_| !f.local_only);
        let account = pushed_for(&pushed.account);
        let team = pushed_for(&pushed.team);
        let (on, source) = match (local, account, team) {
            (Some(on), ..) => (on, Source::Machine),
            (None, Some(on), _) => (on, Source::Account),
            (None, None, Some(on)) => (on, Source::Team),
            (None, None, None) => (f.default, Source::Default),
        };
        State { on, source, local, account, team }
    }
}

/// Whether the flag `name` (or an old name of one) is on in `state_dir`.
/// Read on every call; a name that isn't in [`FLAGS`] is off.
pub fn get(state_dir: &Path, name: &str) -> bool {
    lookup(name).is_some_and(|f| read(state_dir).state(&read_control(state_dir), f).on)
}

/// What this machine's `flags.json` says of `name`, by name or by `labs`
/// (which was every flag), whether or not it is a flag now: how a feature
/// that left Labs for a setting keeps the machine's choice (`guest-ssh`,
/// native #205). Control's word isn't read.
pub fn said_here(state_dir: &Path, name: &str) -> Option<bool> {
    let doc = read(state_dir);
    doc.says(name).or_else(|| doc.says(LABS))
}

/// Every flag with its state, from one read of each file.
pub fn all(state_dir: &Path) -> Vec<(Flag, bool)> {
    explain(state_dir).into_iter().map(|(f, s)| (f, s.on)).collect()
}

/// Turns the flag `name` on or off, keeping everything else in the file.
/// An old name sets the flag it now is (an old name already in the file is
/// written too, so an older daemon reading it agrees). An unknown name is
/// `InvalidInput`.
pub fn set(state_dir: &Path, name: &str, on: bool) -> io::Result<()> {
    write(state_dir, Some((known(name)?, Some(on))))
}

/// The flag `name` names, or why it doesn't.
fn known(name: &str) -> io::Result<&'static str> {
    if let Some(f) = lookup(name) {
        return Ok(f.name);
    }
    if let Some(why) = retired(name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    let known: Vec<_> = FLAGS.iter().map(|f| f.name).collect();
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("no flag named {name} (the flags are {})", known.join(", ")),
    ))
}

/// Takes this machine's word on `name` back (#347): the flag follows
/// control, or its default. Where `labs` (or the old marker) spoke for it,
/// `labs` is first spelled out as each flag the file didn't name, so the
/// others stay as they were. An old name takes back the flag it now is,
/// under both names. An unknown name is `InvalidInput`.
pub fn unset(state_dir: &Path, name: &str) -> io::Result<()> {
    write(state_dir, Some((known(name)?, None)))
}

/// Makes the flags file if there is none, with nothing in it: the owner
/// asking for Developer settings ([`unlocked`]) without turning anything on.
pub fn unlock(state_dir: &Path) -> io::Result<()> {
    if state_dir.join(FILE).exists() { Ok(()) } else { write(state_dir, None) }
}

/// The file written again with the flag `key` set (`None`: taken out), if
/// one is given.
fn write(state_dir: &Path, key: Option<(&'static str, Option<bool>)>) -> io::Result<()> {
    let mut doc = match read(state_dir) {
        Doc::Ok(m) => m,
        Doc::Missing => Map::new(),
        Doc::Malformed => {
            // An explicit set replaces it, with the old one kept.
            let _ = fs::copy(state_dir.join(FILE), state_dir.join(format!("{FILE}.bad")));
            Map::new()
        }
    };
    let flags = doc.entry("flags").or_insert_with(|| Value::Object(Map::new()));
    match (flags, key) {
        (Value::Object(flags), Some((name, Some(on)))) => {
            // An old name the file has is kept in step, for an older
            // daemon reading it; a new file gets the new name only.
            for old in names_of(name).skip(1).filter(|n| flags.contains_key(*n)).collect::<Vec<_>>() {
                flags.insert(old.to_owned(), Value::Bool(on));
            }
            flags.insert(name.to_owned(), Value::Bool(on));
        }
        (Value::Object(flags), Some((name, None))) => {
            // `labs` speaks for every flag the file doesn't name: name
            // them first, so taking one back leaves the rest as they were.
            let named = |flags: &Map<String, Value>, f: &'static str| names_of(f).any(|n| flags.contains_key(n));
            let labs = flags.get(LABS).and_then(Value::as_bool);
            if let Some(labs) = labs.filter(|_| !named(flags, name)) {
                for f in FLAGS {
                    if !named(flags, f.name) {
                        flags.insert(f.name.to_owned(), Value::Bool(labs));
                    }
                }
                flags.remove(LABS);
            }
            for n in names_of(name) {
                flags.remove(n);
            }
        }
        _ => {}
    }
    let mut text = serde_json::to_string_pretty(&doc).expect("a JSON object serializes");
    text.push('\n');
    write_atomic(&state_dir.join(FILE), text.as_bytes())
}

/// A new file renamed over the old, so a reader sees all of one or all of
/// the other. Private to the owner, as the state dir's other files are. The
/// temporary file's name has our pid, so two writers don't share one.
/// (`rename` replaces an existing file on Windows too.)
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new();
    file.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    let written = (|| {
        let mut f = file.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-flags-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(d: &Path) -> Vec<&'static str> {
        on(d).names().collect()
    }

    #[test]
    fn a_flag_is_at_its_default_until_set_and_then_follows_the_file() {
        let d = dir("set");
        assert!(!get(&d, ADDITIONAL_FORGES) && !on(&d).any() && !unlocked(&d));
        set(&d, ADDITIONAL_FORGES, true).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES) && unlocked(&d));
        assert_eq!(
            fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""),
            r#"{"flags":{"additional-forges":true}}"#
        );
        // One flag turns on alone.
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        set(&d, CHANT_WORKSPACES, true).unwrap();
        set(&d, ADDITIONAL_FORGES, false).unwrap();
        assert_eq!(names(&d), [CHANT_WORKSPACES]);
        assert_eq!(all(&d).iter().filter(|(_, on)| *on).map(|(f, _)| f.name).collect::<Vec<_>>(), [CHANT_WORKSPACES]);
        assert_eq!(all(&d).len(), FLAGS.len());
        // All off is still an answer: the file stays, and so does the asking.
        set(&d, CHANT_WORKSPACES, false).unwrap();
        assert!(!on(&d).any() && unlocked(&d));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn asking_makes_an_empty_file_and_leaves_a_file_alone() {
        let d = dir("unlock");
        unlock(&d).unwrap();
        assert!(unlocked(&d) && !on(&d).any());
        set(&d, ADDITIONAL_FORGES, true).unwrap();
        let before = fs::read_to_string(d.join(FILE)).unwrap();
        unlock(&d).unwrap();
        assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(), before);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_set_of_flags_knows_all_none_and_each_name() {
        assert_eq!(On::all().names().count(), FLAGS.len());
        assert!(On::all().any() && !On::none().any());
        for f in FLAGS {
            assert!(On::all().has(f.name) && !On::none().has(f.name), "{}", f.name);
            assert!(!f.default, "{} is on for a stranger", f.name);
        }
        assert!(!On::all().has(LABS) && !On::all().has("nope"));
    }

    #[test]
    fn an_unknown_flag_is_off_and_an_error_to_set() {
        let d = dir("unknown");
        assert!(!get(&d, "nope"));
        let e = set(&d, "nope", true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert!(e.to_string().contains("sandbox-providers"), "{e}");
        assert!(!d.join(FILE).exists());
        // `labs` was a flag once (#464); it isn't one to set now.
        assert_eq!(set(&d, LABS, true).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        // Even if the file names it.
        fs::write(d.join(FILE), r#"{"flags":{"nope":true}}"#).unwrap();
        assert!(!get(&d, "nope"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_write_keeps_the_keys_and_flags_it_doesnt_know() {
        let d = dir("keeps");
        fs::write(d.join(FILE), r#"{"other":{"a":1},"flags":{"future":true,"additional-forges":false}}"#).unwrap();
        set(&d, ADDITIONAL_FORGES, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"other":{"a":1},"flags":{"future":true,"additional-forges":true}}));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_malformed_file_reads_as_defaults_and_only_a_set_replaces_it() {
        let d = dir("bad");
        for bad in ["{not json", "[]", r#"{"flags":3}"#] {
            fs::write(d.join(FILE), bad).unwrap();
            assert!(!get(&d, ADDITIONAL_FORGES), "{bad}");
            assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(), bad, "a read left it alone");
        }
        fs::write(d.join(FILE), "{not json").unwrap();
        set(&d, ADDITIONAL_FORGES, true).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES));
        assert_eq!(fs::read_to_string(d.join(format!("{FILE}.bad"))).unwrap(), "{not json");
        fs::remove_dir_all(&d).unwrap();
    }

    /// What was one switch is every flag: `labs` in the flags file. A flag
    /// the file names for itself follows that. The old empty `labs` file of
    /// #385 means nothing now.
    #[test]
    fn labs_from_before_is_every_flag_the_file_doesnt_name() {
        let d = dir("legacy");
        fs::write(d.join(FILE), r#"{"flags":{"labs":true}}"#).unwrap();
        assert_eq!(on(&d), On::all());
        // One turned off there leaves the rest.
        set(&d, SANDBOX_PROVIDERS, false).unwrap();
        assert!(!get(&d, SANDBOX_PROVIDERS) && get(&d, ADDITIONAL_FORGES));
        assert_eq!(names(&d).len(), FLAGS.len() - 1);

        // On is all of them, off none.
        fs::write(d.join(FILE), r#"{"flags":{"labs":true,"studio":false}}"#).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES) && !get(&d, STUDIO));
        fs::write(d.join(FILE), r#"{"flags":{"labs":false,"forges":true}}"#).unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        fs::remove_dir_all(&d).unwrap();

        // The marker file alone is neither Labs nor Developer settings.
        let d = dir("marker");
        fs::write(d.join("labs"), "").unwrap();
        assert!(!on(&d).any() && !unlocked(&d));
        fs::remove_dir_all(&d).unwrap();
    }

    fn pushed(account: &[(&str, bool)], team: &[(&str, bool)]) -> Pushed {
        let m = |kv: &[(&str, bool)]| kv.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        Pushed { account: m(account), team: m(team) }
    }

    fn state_of(d: &Path, name: &str) -> State {
        explain(d).into_iter().find(|(f, _)| f.name == name).unwrap().1
    }

    /// #347: this machine's word, then control's for the account, then for
    /// the team, then the default.
    #[test]
    fn the_machine_beats_the_account_which_beats_the_team() {
        let d = dir("pushed");
        assert!(!write_control(&d, &Pushed::default()).unwrap(), "nothing to write, nothing written");
        assert!(!d.join(CONTROL_FILE).exists() && !unlocked(&d));

        // `vms` is the flag's old name, and speaks for it (aru-117).
        assert!(
            write_control(
                &d,
                &pushed(&[("vms", false)], &[(ADDITIONAL_FORGES, true), (SANDBOX_PROVIDERS, true), ("future", true)])
            )
            .unwrap()
        );
        assert!(
            !write_control(
                &d,
                &pushed(&[("vms", false)], &[(ADDITIONAL_FORGES, true), (SANDBOX_PROVIDERS, true), ("future", true)])
            )
            .unwrap()
        );
        // Control saying something offers Developer settings, to see it.
        assert!(unlocked(&d) && !d.join(FILE).exists());
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        assert_eq!(
            state_of(&d, ADDITIONAL_FORGES),
            State { on: true, source: Source::Team, local: None, account: None, team: Some(true) }
        );
        assert_eq!(state_of(&d, SANDBOX_PROVIDERS).source, Source::Account);
        assert!(!state_of(&d, SANDBOX_PROVIDERS).on);
        assert_eq!(state_of(&d, STUDIO).source, Source::Default);

        // The machine forces either way, and taking its word back follows
        // control again.
        set(&d, ADDITIONAL_FORGES, false).unwrap();
        set(&d, SANDBOX_PROVIDERS, true).unwrap();
        assert_eq!(names(&d), [SANDBOX_PROVIDERS]);
        assert_eq!(
            state_of(&d, ADDITIONAL_FORGES),
            State { on: false, source: Source::Machine, local: Some(false), account: None, team: Some(true) }
        );
        unset(&d, ADDITIONAL_FORGES).unwrap();
        unset(&d, SANDBOX_PROVIDERS).unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        assert_eq!(fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""), r#"{"flags":{}}"#);
        assert_eq!(unset(&d, "nope").unwrap_err().kind(), io::ErrorKind::InvalidInput);

        // Leaving control: only the machine's own word counts.
        assert!(forget_control(&d).unwrap());
        assert!(!on(&d).any() && !forget_control(&d).unwrap());
        // And a control that pushes nothing any more takes the file away.
        write_control(&d, &pushed(&[(ADDITIONAL_FORGES, true)], &[])).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES));
        assert!(write_control(&d, &Pushed::default()).unwrap());
        assert!(!d.join(CONTROL_FILE).exists() && !get(&d, ADDITIONAL_FORGES));
        fs::remove_dir_all(&d).unwrap();
    }

    /// `labs` stood for every flag the file doesn't name: taking one back
    /// spells the rest out, so they stay on.
    #[test]
    fn taking_one_back_from_labs_keeps_the_rest() {
        let d = dir("unset-labs");
        fs::write(d.join(FILE), r#"{"flags":{"labs":true,"studio":false}}"#).unwrap();
        write_control(&d, &pushed(&[], &[(ADDITIONAL_FORGES, false)])).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES) && state_of(&d, ADDITIONAL_FORGES).source == Source::Machine);
        unset(&d, ADDITIONAL_FORGES).unwrap();
        assert_eq!(state_of(&d, ADDITIONAL_FORGES).source, Source::Team);
        assert!(!get(&d, ADDITIONAL_FORGES) && get(&d, SANDBOX_PROVIDERS) && !get(&d, STUDIO));
        assert_eq!(names(&d).len(), FLAGS.len() - 2);
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert!(v["flags"].get(LABS).is_none() && v["flags"].get(ADDITIONAL_FORGES).is_none());
        fs::remove_dir_all(&d).unwrap();
    }

    /// The agent catalog is the machine's alone: control turns it neither
    /// on nor off. (Guest ssh was too; it's a sharing setting now, which
    /// control never writes, native #205.)
    #[test]
    fn control_has_no_say_in_a_local_only_flag() {
        let local: Vec<_> = FLAGS.iter().filter(|f| f.local_only).map(|f| f.name).collect();
        assert_eq!(local, [AGENT_DELEGATION]);
        let d = dir("local-only");
        write_control(&d, &pushed(&[(AGENT_DELEGATION, true)], &[(AGENT_DELEGATION, true), (ADDITIONAL_FORGES, true)]))
            .unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        assert_eq!(
            state_of(&d, AGENT_DELEGATION),
            State { on: false, source: Source::Default, local: None, account: None, team: None }
        );
        assert_eq!(state_of(&d, AGENT_DELEGATION).source, Source::Default);
        // Nor off: the machine's word is all there is.
        set(&d, AGENT_DELEGATION, true).unwrap();
        write_control(&d, &pushed(&[(AGENT_DELEGATION, false)], &[])).unwrap();
        assert!(get(&d, AGENT_DELEGATION) && state_of(&d, AGENT_DELEGATION).source == Source::Machine);
        fs::remove_dir_all(&d).unwrap();
    }

    /// `fountain` left Labs (#942, F5): a file that turned it on or off, or
    /// control saying so, still reads, means nothing, and survives a write;
    /// setting it says why it's gone.
    #[test]
    fn a_retired_flag_in_the_file_is_ignored_and_kept() {
        assert!(FLAGS.iter().all(|f| RETIRED.iter().all(|(n, _)| *n != f.name)));
        let d = dir("retired");
        fs::write(d.join(FILE), r#"{"flags":{"fountain":true,"forges":true}}"#).unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        assert!(!get(&d, "fountain"));
        assert_eq!(all(&d).len(), FLAGS.len());
        fs::write(d.join(FILE), r#"{"flags":{"fountain":false}}"#).unwrap();
        assert!(!on(&d).any());
        write_control(&d, &pushed(&[("fountain", true)], &[("fountain", false)])).unwrap();
        assert!(!on(&d).any());
        // A write keeps it, for an older daemon reading the same file.
        set(&d, STUDIO, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["flags"]["fountain"], false);
        assert_eq!(names(&d), [STUDIO]);
        for e in [set(&d, "fountain", true).unwrap_err(), unset(&d, "fountain").unwrap_err()] {
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
            assert!(
                e.to_string().contains("isn't a flag any more") && e.to_string().contains("Cloud workspaces"),
                "{e}"
            );
        }
        fs::remove_dir_all(&d).unwrap();
    }

    /// Huddles were removed (2026-10-18): a `flags.json` an older daemon
    /// wrote with `"huddles": true`, or control still pushing it, loads
    /// without error and turns nothing on.
    #[test]
    fn huddles_on_from_before_loads_and_means_nothing() {
        let d = dir("huddles");
        fs::write(d.join(FILE), r#"{"flags":{"huddles":true,"forges":true}}"#).unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        assert!(!get(&d, "huddles"));
        write_control(&d, &pushed(&[("huddles", true)], &[("huddles", true)])).unwrap();
        assert_eq!(names(&d), [ADDITIONAL_FORGES]);
        set(&d, SANDBOX_PROVIDERS, true).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["flags"]["huddles"], true, "kept for an older daemon");
        let e = set(&d, "huddles", true).unwrap_err();
        assert!(e.to_string().contains("isn't a flag any more") && e.to_string().contains("removed"), "{e}");
        fs::remove_dir_all(&d).unwrap();
    }

    /// The renamed flags (aru-117): each old name is the new flag, in the
    /// file and to every function that takes a name, and listed nowhere.
    #[test]
    fn an_old_name_in_the_file_is_its_flag() {
        for (old, new) in ALIASES {
            assert!(FLAGS.iter().all(|f| f.name != *old), "{old} is a flag's name and an old one");
            assert!(RETIRED.iter().all(|(n, _)| n != old), "{old} is retired and renamed");
            assert!(ALIASES.iter().all(|(o, _)| o != new), "{new} is an old name too");
            assert_eq!((canonical(old), lookup(old).map(|f| f.name)), (*new, Some(*new)));
            let d = dir(&format!("alias-{old}"));
            fs::write(d.join(FILE), format!(r#"{{"flags":{{"{old}":true}}}}"#)).unwrap();
            assert!(get(&d, new) && get(&d, old) && on(&d).has(old), "{old}");
            assert_eq!(names(&d), [*new], "listed by its new name only");
            assert_eq!(state_of(&d, new).local, Some(true));
            fs::remove_dir_all(&d).unwrap();
        }
        assert_eq!(canonical(CHANT_WORKSPACES), CHANT_WORKSPACES);
        assert_eq!(canonical("nope"), "nope");
        // Where both are said, the new name's word wins.
        let d = dir("alias-both");
        fs::write(d.join(FILE), r#"{"flags":{"vms":true,"sandbox-providers":false}}"#).unwrap();
        assert!(!get(&d, SANDBOX_PROVIDERS) && !get(&d, "vms"));
        // An unknown name's error lists the new names, not the old.
        let e = set(&d, "nope", true).unwrap_err().to_string();
        assert!(e.contains(SANDBOX_PROVIDERS) && !e.contains("vms"), "{e}");
        fs::remove_dir_all(&d).unwrap();
    }

    /// `arugulad flags vms on` and `PUT /api/flags/vms` set the flag by its
    /// new name; an old name already in the file is kept in step for an
    /// older daemon, and taking the flag back takes both.
    #[test]
    fn setting_by_an_old_name_sets_the_new_one() {
        let d = dir("alias-set");
        set(&d, "vms", true).unwrap();
        assert_eq!(
            fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""),
            r#"{"flags":{"sandbox-providers":true}}"#
        );
        assert_eq!(names(&d), [SANDBOX_PROVIDERS]);
        fs::write(d.join(FILE), r#"{"flags":{"forges":true}}"#).unwrap();
        set(&d, ADDITIONAL_FORGES, false).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert_eq!(v["flags"], serde_json::json!({"forges": false, "additional-forges": false}));
        set(&d, "forges", true).unwrap();
        assert!(get(&d, ADDITIONAL_FORGES));
        unset(&d, "forges").unwrap();
        assert_eq!(fs::read_to_string(d.join(FILE)).unwrap().replace(char::is_whitespace, ""), r#"{"flags":{}}"#);
        assert_eq!(state_of(&d, ADDITIONAL_FORGES).source, Source::Default);
        fs::remove_dir_all(&d).unwrap();
    }

    /// A control (aru-068) that pushes an old name still sets the flag;
    /// a local-only flag ignores it by either name; the new name wins.
    #[test]
    fn control_pushing_an_old_name_still_works() {
        let d = dir("alias-pushed");
        write_control(
            &d,
            &pushed(&[("workspaces", true), ("forges", true), (ADDITIONAL_FORGES, false)], &[("agents", true)]),
        )
        .unwrap();
        assert_eq!(state_of(&d, CHANT_WORKSPACES).source, Source::Account);
        assert_eq!(names(&d), [CHANT_WORKSPACES]);
        assert_eq!(
            state_of(&d, AGENT_DELEGATION),
            State { on: false, source: Source::Default, local: None, account: None, team: None }
        );
        fs::remove_dir_all(&d).unwrap();
    }

    /// `labs` spelled out by taking a flag back skips a flag the file names
    /// by its old name: that word stays the flag's.
    #[test]
    fn taking_one_back_from_labs_keeps_an_old_name_s_word() {
        let d = dir("alias-labs");
        fs::write(d.join(FILE), r#"{"flags":{"labs":true,"vms":false}}"#).unwrap();
        assert!(!get(&d, SANDBOX_PROVIDERS) && get(&d, CHANT_WORKSPACES));
        unset(&d, CHANT_WORKSPACES).unwrap();
        assert!(!get(&d, SANDBOX_PROVIDERS) && get(&d, ADDITIONAL_FORGES) && !get(&d, CHANT_WORKSPACES));
        let v: Value = serde_json::from_str(&fs::read_to_string(d.join(FILE)).unwrap()).unwrap();
        assert!(v["flags"].get(SANDBOX_PROVIDERS).is_none() && v["flags"]["vms"] == false, "{v}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_control_file_that_doesnt_parse_says_nothing() {
        let d = dir("pushed-bad");
        fs::write(d.join(CONTROL_FILE), "{not json").unwrap();
        assert!(!on(&d).any() && !unlocked(&d));
        assert!(write_control(&d, &pushed(&[(ADDITIONAL_FORGES, true)], &[])).unwrap());
        assert!(get(&d, ADDITIONAL_FORGES));
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        set(&d, ADDITIONAL_FORGES, true).unwrap();
        assert_eq!(fs::metadata(d.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        let tmps = fs::read_dir(&d)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmps, 0);
        fs::remove_dir_all(&d).unwrap();
    }
}
