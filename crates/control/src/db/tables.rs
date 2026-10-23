//! The table manifest (`crates/control/tables.toml`, "Two planes, two
//! trust rules"): every table and view a fresh database has is listed, and
//! nothing else is, and "What control stores" in `docs/control-e2e.md` is
//! what the manifest says. `ARUGULA_WRITE_DOCS=1` (`just control-tables`)
//! writes that section instead of checking it.
use std::{collections::BTreeMap, fmt::Write as _, path::PathBuf};

use serde::Deserialize;

use super::Db;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    never: Vec<String>,
    parties: BTreeMap<String, Party>,
    table: Vec<Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Party {
    name: String,
    sees: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    name: String,
    plane: Plane,
    /// Not in the doc: which process holds a table is how control runs,
    /// not what it stores.
    process: Process,
    holds: Holds,
    what: String,
    #[serde(default)]
    why_asleep: String,
    with_account: WithAccount,
    #[serde(default)]
    note: String,
    seen_by: Vec<String>,
}

#[derive(Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Plane {
    Machine,
    Platform,
}

/// Which of control's two processes owns it (#1150).
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Process {
    Hub,
    Platform,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Holds {
    Metadata,
    Ciphertext,
    Plaintext,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum WithAccount {
    Deleted,
    Partly,
    Kept,
    None,
}

/// The parties in the order the doc names them.
const PARTIES: [&str; 8] = ["litestream", "run_logs", "e2b", "anthropic", "posthog", "github", "infisical", "stripe"];

const BEGIN: &str = "<!-- Written by `just control-tables` from crates/control/tables.toml. Edit that, not this. -->";
const END: &str = "<!-- End of what `just control-tables` writes. -->";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tables.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let m: Manifest = toml::from_str(&text).unwrap_or_else(|e| panic!("crates/control/tables.toml: {e}"));
    let mut names = std::collections::BTreeSet::new();
    for t in &m.table {
        let n = &t.name;
        assert!(names.insert(n.as_str()), "tables.toml lists {n} twice");
        match t.plane {
            Plane::Platform => {
                assert!(!t.why_asleep.is_empty(), "tables.toml: {n} is on the platform plane: say why_asleep")
            }
            Plane::Machine => {
                assert!(t.why_asleep.is_empty(), "tables.toml: {n} is on the machine plane: no why_asleep")
            }
        }
        if matches!(t.with_account, WithAccount::Partly | WithAccount::Kept) {
            assert!(!t.note.is_empty(), "tables.toml: {n}: a note says what stays, and why");
        }
        assert!(t.seen_by.iter().any(|p| p == "litestream"), "tables.toml: {n}: Litestream copies every table");
        for p in &t.seen_by {
            assert!(PARTIES.contains(&p.as_str()), "tables.toml: {n}: {p} isn't one of {PARTIES:?}");
        }
    }
    for p in PARTIES {
        assert!(m.parties.contains_key(p), "tables.toml: no [parties.{p}]");
    }
    m
}

/// Every table and view in a database made by `Db::open` and the
/// platform's `Db::open` beside it (as control opens both), but SQLite's
/// own and the shadow tables of an FTS5 table it lists.
fn schema() -> Vec<String> {
    let path = std::env::temp_dir().join(format!("arugula-tables-{}.db", hex::encode(arugula_e2e::random::<16>())));
    let names = {
        let db = Db::open(&path).unwrap();
        let _platform = arugula_platform::db::Db::open(&path).unwrap();
        let c = db.c();
        let rows: Vec<(String, String)> = c
            .prepare("SELECT name, sql FROM sqlite_master WHERE type IN ('table', 'view') AND substr(name, 1, 7) != 'sqlite_'")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let fts: Vec<&str> = rows
            .iter()
            .filter(|(_, sql)| sql.to_ascii_uppercase().contains("USING FTS5"))
            .map(|(n, _)| n.as_str())
            .collect();
        let shadow = |n: &str| {
            fts.iter().any(|f| {
                n.strip_prefix(f)
                    .and_then(|s| s.strip_prefix('_'))
                    .is_some_and(|s| ["data", "idx", "content", "docsize", "config"].contains(&s))
            })
        };
        rows.iter().map(|(n, _)| n.clone()).filter(|n| !shadow(n)).collect()
    };
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    names
}

#[test]
fn every_table_is_listed() {
    let m = manifest();
    let have = schema();
    let mut wrong = String::new();
    for n in have.iter().filter(|n| !m.table.iter().any(|t| &t.name == *n)) {
        let _ = write!(
            wrong,
            "\n{n} isn't listed. Add a row (the header says what each field means):\n\n\
             [[table]]\nname = \"{n}\"\nplane = \"machine or platform\"\nprocess = \"hub or platform\"\nholds = \"metadata, ciphertext or plaintext\"\n\
             what = \"...\"\nwhy_asleep = \"... (the platform plane only)\"\nwith_account = \"deleted, partly, kept or none\"\n\
             seen_by = [\"litestream\"]\n"
        );
    }
    for t in m.table.iter().filter(|t| !have.contains(&t.name)) {
        let _ = writeln!(
            wrong,
            "\n{} is listed, and a fresh database has no such table or view. Take its row out.",
            t.name
        );
    }
    assert!(
        wrong.is_empty(),
        "crates/control/tables.toml doesn't match control's database (\"Two planes, two trust rules\"). \
         Fix it as below, then run `just control-tables` to write the doc:\n{wrong}"
    );
}

/// The platform makes the tables listed as its own, and the hub makes
/// none of them (#1150): a feature's tables move with it.
#[test]
fn each_process_makes_its_own_tables() {
    let m = manifest();
    let names = |c: &rusqlite::Connection| -> Vec<String> {
        c.prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let mut listed: Vec<String> =
        m.table.iter().filter(|t| t.process == Process::Platform).map(|t| t.name.clone()).collect();
    listed.sort();
    // An FTS5 table's shadow tables (`chat_fts_data` ...) are the listed
    // table's.
    let shadow = |n: &String| {
        listed.iter().any(|l| {
            n.strip_prefix(l.as_str())
                .is_some_and(|s| ["_data", "_idx", "_content", "_docsize", "_config"].contains(&s))
        })
    };
    let mut platform = names(&arugula_platform::db::Db::memory().sqlite());
    platform.retain(|n| !shadow(n));
    assert_eq!(platform, listed, "tables.toml's `process = \"platform\"` rows are what the platform makes");
    let hub = names(&Db::memory().c());
    let both: Vec<&String> = platform.iter().filter(|n| hub.contains(n)).collect();
    assert!(both.is_empty(), "the hub still makes the platform's {both:?}");
}

/// No hub source names a platform table in SQL (#1150): the hub reaches
/// what the platform holds only through `PlatformPort`. Beside
/// [`each_process_makes_its_own_tables`], which proves the hub's database
/// has none of them, this catches a query no test runs. A name counts when
/// it follows `FROM`, `INTO`, `UPDATE`, `JOIN`, `TABLE` or `EXISTS`.
#[test]
fn the_hub_names_no_platform_table() {
    fn sources(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                sources(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let m = manifest();
    let platform: Vec<&str> =
        m.table.iter().filter(|t| t.process == Process::Platform).map(|t| t.name.as_str()).collect();
    let mut files = Vec::new();
    sources(&root().join("crates/control/src"), &mut files);
    let mut found = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for name in &platform {
            for (at, _) in text.match_indices(name) {
                let after = text[at + name.len()..].chars().next();
                let before = text[..at].trim_end();
                if after.is_some_and(ident) || before.len() == text[..at].len() {
                    continue;
                }
                let word: String =
                    before.chars().rev().take_while(|c| ident(*c)).collect::<Vec<_>>().into_iter().rev().collect();
                if ["FROM", "INTO", "UPDATE", "JOIN", "TABLE", "EXISTS"].contains(&word.as_str()) {
                    let line = text[..at].lines().count();
                    found.push(format!("{}:{line}: {word} {name}", f.strip_prefix(root()).unwrap().display()));
                }
            }
        }
    }
    assert!(found.is_empty(), "the hub's code names the platform's tables:\n{}", found.join("\n"));
}

/// What a window copies (`arugula_platform::window`, M141) is every
/// `process = "platform"` table but `platform_migrations`, which each
/// file's own migration runner keeps, and the FTS5 indexes (`chat_fts`,
/// `issues_fts`), made again from the copied rows.
#[test]
fn a_window_copies_every_platform_table() {
    let m = manifest();
    let mut listed: Vec<String> = m
        .table
        .iter()
        .filter(|t| t.process == Process::Platform && t.name != "platform_migrations" && !t.name.ends_with("_fts"))
        .map(|t| t.name.clone())
        .collect();
    listed.sort();
    let mut copied = arugula_platform::window::tables();
    copied.sort();
    assert_eq!(copied, listed);
}

/// After M141's window the hub's file keeps its copies of the platform's
/// tables, read-only, for 14 days as the rollback (#1158). The hub never
/// writes them: opening its database (its schema code and its start-up
/// sweeps) and deleting an account named all through them leave every
/// row as it was, by the window's own checksums.
#[test]
fn the_hub_leaves_the_platforms_old_copies_alone() {
    let path = std::env::temp_dir().join(format!("arugula-copies-{}.db", hex::encode(arugula_e2e::random::<16>())));
    {
        let _hub = Db::open(&path).unwrap();
        arugula_platform::window::sample(&arugula_platform::db::Db::open(&path).unwrap());
    }
    let before = arugula_platform::window::checksums(&path).unwrap();
    assert!(before.iter().map(|t| t.1).sum::<u64>() >= 10);
    {
        let hub = Db::open(&path).unwrap();
        hub.c().execute("INSERT INTO accounts (id, created) VALUES ('acct-a', 1)", []).unwrap();
        hub.delete_account(&super::Erase { account: "acct-a", disband: &[], retain_for: &[] }).unwrap();
    }
    drop(Db::open(&path).unwrap());
    assert_eq!(arugula_platform::window::checksums(&path).unwrap(), before);
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
}

fn party_names(m: &Manifest, t: &Table) -> String {
    let names: Vec<&str> = PARTIES
        .iter()
        .filter(|p| **p != "litestream" && t.seen_by.iter().any(|s| s == *p))
        .map(|p| m.parties[*p].name.as_str())
        .collect();
    names.join(", ")
}

fn with_account(t: &Table) -> String {
    let word = match t.with_account {
        WithAccount::Deleted => "yes",
        WithAccount::Partly => "partly",
        WithAccount::Kept => "kept",
        WithAccount::None => "nothing of one",
    };
    let mut note = t.note.chars();
    match note.next() {
        Some(first) => format!("{word}: {}{}", first.to_lowercase(), note.as_str()),
        None => word.into(),
    }
}

fn holds(t: &Table) -> &'static str {
    match t.holds {
        Holds::Metadata => "metadata",
        Holds::Ciphertext => "ciphertext",
        Holds::Plaintext => "**plaintext**",
    }
}

/// The section, between the markers.
fn render(m: &Manifest) -> String {
    let count = |p: Plane| m.table.iter().filter(|t| t.plane == p).count();
    let mut s = String::new();
    s.push_str(BEGIN);
    s.push_str("\n\n");
    let _ = writeln!(
        s,
        "Control's database has {} tables in two planes (\"Two planes, two trust rules\"):\n",
        m.table.len()
    );
    let _ = writeln!(
        s,
        "- **The machine plane** ({} tables) is what control needs to route and check: ids, names, \
         addresses, public keys and signed statements. What moves between a client and a machine \
         stays end to end, and control can't read it.",
        count(Plane::Machine)
    );
    let _ = writeln!(
        s,
        "- **The platform plane** ({} tables) is what has to exist while every machine is asleep: \
         billing and usage, chat and the threads on objects, Shared Docs, Cloud workspaces, linked repos' PRs and \
         issues, and projects' native issues, runs and deploys. Control reads most of it, \
         and says so here.\n",
        count(Plane::Platform)
    );
    s.push_str(
        "Every table is listed in `crates/control/tables.toml`, and a test fails when one isn't. \
         **Holds** is what's in it: metadata (ids, names, addresses, times, counts, public keys, \
         signed statements), ciphertext (encrypted to keys control never holds), or plaintext \
         (what people or their agents wrote, or a secret control can read). **Gone with the account** is what \
         deleting an account does to its rows.\n",
    );
    for (plane, title) in [(Plane::Machine, "The machine plane"), (Plane::Platform, "The platform plane")] {
        let _ = writeln!(s, "\n### {title}\n");
        if plane == Plane::Platform {
            s.push_str("| Table | Holds | What | Why it exists while machines sleep | Gone with the account | Also seen by |\n");
            s.push_str("|---|---|---|---|---|---|\n");
        } else {
            s.push_str("| Table | Holds | What | Gone with the account | Also seen by |\n");
            s.push_str("|---|---|---|---|---|\n");
        }
        for t in m.table.iter().filter(|t| t.plane == plane) {
            let why = if plane == Plane::Platform { format!(" {} |", t.why_asleep) } else { String::new() };
            let _ = writeln!(
                s,
                "| `{}` | {} | {} |{why} {} | {} |",
                t.name,
                holds(t),
                t.what,
                with_account(t),
                party_names(m, t)
            );
        }
    }
    s.push_str("\n### Who else sees it\n\n");
    for p in PARTIES {
        let _ = writeln!(s, "- **{}:** {}", m.parties[p].name, m.parties[p].sees);
    }
    s.push_str("\n### Never stored\n\n");
    for n in &m.never {
        let _ = writeln!(s, "- {n}");
    }
    s.push('\n');
    s.push_str(END);
    s
}

#[test]
fn control_e2e_is_current() {
    let path = root().join("docs/control-e2e.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    let (Some(a), Some(b)) = (doc.find(BEGIN), doc.find(END)) else {
        panic!("docs/control-e2e.md has no section between {BEGIN} and {END}");
    };
    let want = render(&manifest());
    let have = &doc[a..b + END.len()];
    if std::env::var_os("ARUGULA_WRITE_DOCS").is_some() {
        std::fs::write(&path, format!("{}{want}{}", &doc[..a], &doc[b + END.len()..])).unwrap();
        return;
    }
    assert!(
        have == want,
        "\"What control stores\" in docs/control-e2e.md isn't what crates/control/tables.toml says: \
         run `just control-tables`, and edit the manifest, not the doc"
    );
}
