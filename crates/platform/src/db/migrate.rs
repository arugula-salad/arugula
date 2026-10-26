//! The platform's schema as numbered migrations (M142), the only way its
//! tables are made or changed. The hub keeps its own schema code.
//!
//! - Each migration is a file, `migrations/<dialect>/NNNN_name.sql` in this
//!   crate, listed below. A version means the same change in every
//!   dialect: SQLite here (in process, self-hosted, tests), and Postgres
//!   (M143: hosted control), whose set is written where the two must
//!   differ (FTS5 against `tsvector`). [`pending`] is the same for both;
//!   only running them is the dialect's ([`run`], [`run_postgres`]).
//! - `platform_migrations` records each one applied, with its checksum and
//!   whether it's additive. A shipped migration is never edited: a changed
//!   checksum stops the platform at start.
//! - All of a start's migrations run in one write transaction: they all
//!   land, or nothing does.
//!
//! # Additive migrations, and older builds
//!
//! A migration is **additive** when it only adds what older code ignores:
//! tables, columns with a default or that may be NULL, indexes, or rows.
//! Each declares it ([`Migration::additive`]), and the flag is recorded
//! when it runs. An older build (a rollback) starts on a newer database
//! when every applied migration it doesn't know is recorded as additive,
//! and refuses otherwise, rather than run on a schema it wasn't built for.
//! So a migration that isn't additive (a rename, a drop, a new `NOT NULL`
//! column without a default, a changed meaning) can't be rolled back past:
//! ship it in an announced window, or in two deploys, the first teaching
//! the code both shapes and the second changing the schema.
//!
//! # Adopting a database made before migrations
//!
//! Until M142 each feature made its tables with `CREATE TABLE IF NOT EXISTS`
//! and patched old ones with column probes, inside the hub's file, so
//! production already has the platform's tables and no
//! `platform_migrations`. On such a database [`run`] finds the first version
//! whose schema the tables match exactly ([`Shape`]: columns, `CHECK`s,
//! foreign keys, indexes, triggers and FTS5 tables, compared table by
//! table), records it and the ones before it as applied without running
//! them (`adopted = 1`), and runs the rest. A database with none of the
//! platform's tables is new, and runs them all.
//!
//! A self-hosted control may upgrade straight from any older release, so
//! when the tables match no version, the old schema code runs first, kept
//! frozen in [`super::legacy`] (`CREATE TABLE IF NOT EXISTS` and the column
//! probes, as main last had them), and the tables are matched again. Only
//! if they still match no version is the database refused, naming each
//! difference, and left as it was: the platform won't guess at a schema.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, bail};
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

/// One numbered change to the platform's schema.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
    /// It only adds what older code ignores (see the module's docs), so a
    /// build from before it may still start on a database that has it.
    pub additive: bool,
}

impl Migration {
    /// What `platform_migrations` records, so an edited migration is
    /// caught: SHA-256 of its text, without carriage returns.
    pub fn checksum(&self) -> String {
        hex::encode(Sha256::digest(self.sql.replace('\r', "").as_bytes()))
    }

    /// Its file's name, `NNNN_name.sql`.
    pub fn file(&self) -> String {
        format!("{:04}_{}.sql", self.version, self.name)
    }
}

/// Which SQL a set of migrations is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

impl Dialect {
    pub fn migrations(self) -> &'static [Migration] {
        match self {
            Dialect::Sqlite => SQLITE,
            Dialect::Postgres => POSTGRES,
        }
    }

    /// Where its files are, under this crate.
    pub fn dir(self) -> &'static str {
        match self {
            Dialect::Sqlite => "migrations/sqlite",
            Dialect::Postgres => "migrations/postgres",
        }
    }
}

/// SQLite's: the baseline is production's schema as v0.53.2 made it.
const SQLITE: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("../../migrations/sqlite/0001_baseline.sql"),
        additive: false,
    },
    Migration {
        version: 2,
        name: "hub_feed",
        sql: include_str!("../../migrations/sqlite/0002_hub_feed.sql"),
        additive: true,
    },
    Migration {
        version: 3,
        name: "personal_projects",
        sql: include_str!("../../migrations/sqlite/0003_personal_projects.sql"),
        additive: true,
    },
    Migration {
        version: 4,
        name: "chat_events",
        sql: include_str!("../../migrations/sqlite/0004_chat_events.sql"),
        additive: true,
    },
    Migration {
        version: 5,
        name: "forge_mirror",
        sql: include_str!("../../migrations/sqlite/0005_forge_mirror.sql"),
        additive: true,
    },
    Migration {
        version: 6,
        name: "forge_access",
        sql: include_str!("../../migrations/sqlite/0006_forge_access.sql"),
        additive: true,
    },
    Migration {
        version: 7,
        name: "forge_poller",
        sql: include_str!("../../migrations/sqlite/0007_forge_poller.sql"),
        additive: true,
    },
    Migration {
        version: 8,
        name: "forge_content",
        sql: include_str!("../../migrations/sqlite/0008_forge_content.sql"),
        additive: true,
    },
    Migration {
        version: 9,
        name: "issues",
        sql: include_str!("../../migrations/sqlite/0009_issues.sql"),
        additive: true,
    },
    Migration {
        version: 10,
        name: "object_threads",
        sql: include_str!("../../migrations/sqlite/0010_object_threads.sql"),
        additive: true,
    },
    Migration {
        version: 11,
        name: "agent_authors",
        sql: include_str!("../../migrations/sqlite/0011_agent_authors.sql"),
        additive: true,
    },
    Migration {
        version: 12,
        name: "issue_home",
        sql: include_str!("../../migrations/sqlite/0012_issue_home.sql"),
        additive: true,
    },
    Migration {
        version: 13,
        name: "hosted_workspaces",
        sql: include_str!("../../migrations/sqlite/0013_hosted_workspaces.sql"),
        additive: true,
    },
    Migration {
        version: 14,
        name: "hosted_usage",
        sql: include_str!("../../migrations/sqlite/0014_hosted_usage.sql"),
        additive: true,
    },
    Migration {
        version: 15,
        name: "platform_jobs",
        sql: include_str!("../../migrations/sqlite/0015_platform_jobs.sql"),
        additive: true,
    },
    Migration { version: 16, name: "runs", sql: include_str!("../../migrations/sqlite/0016_runs.sql"), additive: true },
    Migration {
        version: 17,
        name: "deploys",
        sql: include_str!("../../migrations/sqlite/0017_deploys.sql"),
        additive: true,
    },
    Migration {
        version: 18,
        name: "hosted_inference",
        sql: include_str!("../../migrations/sqlite/0018_hosted_inference.sql"),
        additive: true,
    },
    Migration {
        version: 19,
        name: "run_grants",
        sql: include_str!("../../migrations/sqlite/0019_run_grants.sql"),
        additive: true,
    },
    Migration {
        version: 20,
        name: "run_triggers",
        sql: include_str!("../../migrations/sqlite/0020_run_triggers.sql"),
        additive: true,
    },
    Migration {
        version: 21,
        name: "project_keys",
        sql: include_str!("../../migrations/sqlite/0021_project_keys.sql"),
        additive: true,
    },
    Migration {
        version: 22,
        name: "hosted_setup",
        sql: include_str!("../../migrations/sqlite/0022_hosted_setup.sql"),
        additive: true,
    },
    Migration {
        version: 23,
        name: "hosted_transcripts",
        sql: include_str!("../../migrations/sqlite/0023_hosted_transcripts.sql"),
        additive: true,
    },
    Migration {
        version: 24,
        name: "public_previews",
        sql: include_str!("../../migrations/sqlite/0024_public_previews.sql"),
        additive: true,
    },
    Migration {
        version: 25,
        name: "managoat_moves",
        sql: include_str!("../../migrations/sqlite/0025_managoat_moves.sql"),
        additive: true,
    },
    Migration {
        version: 26,
        name: "run_logs",
        sql: include_str!("../../migrations/sqlite/0026_run_logs.sql"),
        additive: true,
    },
    Migration {
        version: 27,
        name: "deploy_approvals",
        sql: include_str!("../../migrations/sqlite/0027_deploy_approvals.sql"),
        additive: true,
    },
    Migration {
        version: 28,
        name: "deploy_runs",
        sql: include_str!("../../migrations/sqlite/0028_deploy_runs.sql"),
        additive: true,
    },
    Migration {
        version: 29,
        name: "environment_settings",
        sql: include_str!("../../migrations/sqlite/0029_environment_settings.sql"),
        additive: true,
    },
    Migration {
        version: 30,
        name: "drop_managoat",
        sql: include_str!("../../migrations/sqlite/0030_drop_managoat.sql"),
        additive: false,
    },
    Migration {
        version: 31,
        name: "agent_tokens",
        sql: include_str!("../../migrations/sqlite/0031_agent_tokens.sql"),
        additive: true,
    },
    Migration {
        version: 32,
        name: "inference_broker",
        sql: include_str!("../../migrations/sqlite/0032_inference_broker.sql"),
        additive: true,
    },
    Migration {
        version: 33,
        name: "product_posts",
        sql: include_str!("../../migrations/sqlite/0033_product_posts.sql"),
        additive: true,
    },
    Migration {
        version: 34,
        name: "clients",
        sql: include_str!("../../migrations/sqlite/0034_clients.sql"),
        additive: true,
    },
    Migration {
        version: 35,
        name: "planter_imports",
        sql: include_str!("../../migrations/sqlite/0035_planter_imports.sql"),
        additive: true,
    },
    Migration {
        version: 36,
        name: "deploy_artifacts",
        sql: include_str!("../../migrations/sqlite/0036_deploy_artifacts.sql"),
        additive: true,
    },
    Migration {
        version: 37,
        name: "redone",
        sql: include_str!("../../migrations/sqlite/0037_redone.sql"),
        additive: true,
    },
    Migration {
        version: 38,
        name: "chat_alerts",
        sql: include_str!("../../migrations/sqlite/0038_chat_alerts.sql"),
        additive: true,
    },
    Migration {
        version: 39,
        name: "link_previews",
        sql: include_str!("../../migrations/sqlite/0039_link_previews.sql"),
        additive: true,
    },
    Migration {
        version: 40,
        name: "chat_reactions",
        sql: include_str!("../../migrations/sqlite/0040_chat_reactions.sql"),
        additive: true,
    },
    Migration {
        version: 41,
        name: "chat_people",
        sql: include_str!("../../migrations/sqlite/0041_chat_people.sql"),
        additive: true,
    },
    Migration {
        version: 42,
        name: "chat_notify",
        sql: include_str!("../../migrations/sqlite/0042_chat_notify.sql"),
        additive: true,
    },
    Migration {
        version: 43,
        name: "chat_sidebars",
        sql: include_str!("../../migrations/sqlite/0043_chat_sidebars.sql"),
        additive: true,
    },
    Migration {
        version: 44,
        name: "chat_saved",
        sql: include_str!("../../migrations/sqlite/0044_chat_saved.sql"),
        additive: true,
    },
    Migration {
        version: 45,
        name: "chat_files",
        sql: include_str!("../../migrations/sqlite/0045_chat_files.sql"),
        additive: true,
    },
    Migration {
        version: 46,
        name: "chat_topics_pins",
        sql: include_str!("../../migrations/sqlite/0046_chat_topics_pins.sql"),
        additive: true,
    },
    Migration {
        version: 47,
        name: "chat_threads",
        sql: include_str!("../../migrations/sqlite/0047_chat_threads.sql"),
        additive: true,
    },
    Migration {
        version: 48,
        name: "chat_channel_members",
        sql: include_str!("../../migrations/sqlite/0048_chat_channel_members.sql"),
        additive: false,
    },
    Migration {
        version: 49,
        name: "hosted_waking",
        sql: include_str!("../../migrations/sqlite/0049_hosted_waking.sql"),
        additive: true,
    },
    Migration {
        version: 50,
        name: "agent_sessions",
        sql: include_str!("../../migrations/sqlite/0050_agent_sessions.sql"),
        additive: true,
    },
    Migration {
        version: 51,
        name: "forge_content_association",
        sql: include_str!("../../migrations/sqlite/0051_forge_content_association.sql"),
        additive: true,
    },
    Migration {
        version: 52,
        name: "session_actions",
        sql: include_str!("../../migrations/sqlite/0052_session_actions.sql"),
        additive: true,
    },
    Migration {
        version: 53,
        name: "pr_drafts",
        sql: include_str!("../../migrations/sqlite/0053_pr_drafts.sql"),
        additive: true,
    },
    Migration {
        version: 54,
        name: "hosted_snapshots",
        sql: include_str!("../../migrations/sqlite/0054_hosted_snapshots.sql"),
        additive: true,
    },
    Migration {
        version: 55,
        name: "session_feedback",
        sql: include_str!("../../migrations/sqlite/0055_session_feedback.sql"),
        additive: true,
    },
    Migration {
        version: 56,
        name: "session_tries",
        sql: include_str!("../../migrations/sqlite/0056_session_tries.sql"),
        additive: true,
    },
    Migration {
        version: 57,
        name: "session_merged",
        sql: include_str!("../../migrations/sqlite/0057_session_merged.sql"),
        additive: true,
    },
    Migration {
        version: 58,
        name: "boards",
        sql: include_str!("../../migrations/sqlite/0058_boards.sql"),
        additive: true,
    },
    Migration {
        version: 59,
        name: "board_row_agent",
        sql: include_str!("../../migrations/sqlite/0059_board_row_agent.sql"),
        additive: true,
    },
];

/// Postgres's (M143): the same versions, names and `additive` flags as
/// [`SQLITE`]. Its baseline is SQLite's in Postgres's words, and a version
/// with nothing to do on Postgres is a file of comments.
const POSTGRES: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("../../migrations/postgres/0001_baseline.sql"),
        additive: false,
    },
    Migration {
        version: 2,
        name: "hub_feed",
        sql: include_str!("../../migrations/postgres/0002_hub_feed.sql"),
        additive: true,
    },
    Migration {
        version: 3,
        name: "personal_projects",
        sql: include_str!("../../migrations/postgres/0003_personal_projects.sql"),
        additive: true,
    },
    Migration {
        version: 4,
        name: "chat_events",
        sql: include_str!("../../migrations/postgres/0004_chat_events.sql"),
        additive: true,
    },
    Migration {
        version: 5,
        name: "forge_mirror",
        sql: include_str!("../../migrations/postgres/0005_forge_mirror.sql"),
        additive: true,
    },
    Migration {
        version: 6,
        name: "forge_access",
        sql: include_str!("../../migrations/postgres/0006_forge_access.sql"),
        additive: true,
    },
    Migration {
        version: 7,
        name: "forge_poller",
        sql: include_str!("../../migrations/postgres/0007_forge_poller.sql"),
        additive: true,
    },
    Migration {
        version: 8,
        name: "forge_content",
        sql: include_str!("../../migrations/postgres/0008_forge_content.sql"),
        additive: true,
    },
    Migration {
        version: 9,
        name: "issues",
        sql: include_str!("../../migrations/postgres/0009_issues.sql"),
        additive: true,
    },
    Migration {
        version: 10,
        name: "object_threads",
        sql: include_str!("../../migrations/postgres/0010_object_threads.sql"),
        additive: true,
    },
    Migration {
        version: 11,
        name: "agent_authors",
        sql: include_str!("../../migrations/postgres/0011_agent_authors.sql"),
        additive: true,
    },
    Migration {
        version: 12,
        name: "issue_home",
        sql: include_str!("../../migrations/postgres/0012_issue_home.sql"),
        additive: true,
    },
    Migration {
        version: 13,
        name: "hosted_workspaces",
        sql: include_str!("../../migrations/postgres/0013_hosted_workspaces.sql"),
        additive: true,
    },
    Migration {
        version: 14,
        name: "hosted_usage",
        sql: include_str!("../../migrations/postgres/0014_hosted_usage.sql"),
        additive: true,
    },
    Migration {
        version: 15,
        name: "platform_jobs",
        sql: include_str!("../../migrations/postgres/0015_platform_jobs.sql"),
        additive: true,
    },
    Migration {
        version: 16,
        name: "runs",
        sql: include_str!("../../migrations/postgres/0016_runs.sql"),
        additive: true,
    },
    Migration {
        version: 17,
        name: "deploys",
        sql: include_str!("../../migrations/postgres/0017_deploys.sql"),
        additive: true,
    },
    Migration {
        version: 18,
        name: "hosted_inference",
        sql: include_str!("../../migrations/postgres/0018_hosted_inference.sql"),
        additive: true,
    },
    Migration {
        version: 19,
        name: "run_grants",
        sql: include_str!("../../migrations/postgres/0019_run_grants.sql"),
        additive: true,
    },
    Migration {
        version: 20,
        name: "run_triggers",
        sql: include_str!("../../migrations/postgres/0020_run_triggers.sql"),
        additive: true,
    },
    Migration {
        version: 21,
        name: "project_keys",
        sql: include_str!("../../migrations/postgres/0021_project_keys.sql"),
        additive: true,
    },
    Migration {
        version: 22,
        name: "hosted_setup",
        sql: include_str!("../../migrations/postgres/0022_hosted_setup.sql"),
        additive: true,
    },
    Migration {
        version: 23,
        name: "hosted_transcripts",
        sql: include_str!("../../migrations/postgres/0023_hosted_transcripts.sql"),
        additive: true,
    },
    Migration {
        version: 24,
        name: "public_previews",
        sql: include_str!("../../migrations/postgres/0024_public_previews.sql"),
        additive: true,
    },
    Migration {
        version: 25,
        name: "managoat_moves",
        sql: include_str!("../../migrations/postgres/0025_managoat_moves.sql"),
        additive: true,
    },
    Migration {
        version: 26,
        name: "run_logs",
        sql: include_str!("../../migrations/postgres/0026_run_logs.sql"),
        additive: true,
    },
    Migration {
        version: 27,
        name: "deploy_approvals",
        sql: include_str!("../../migrations/postgres/0027_deploy_approvals.sql"),
        additive: true,
    },
    Migration {
        version: 28,
        name: "deploy_runs",
        sql: include_str!("../../migrations/postgres/0028_deploy_runs.sql"),
        additive: true,
    },
    Migration {
        version: 29,
        name: "environment_settings",
        sql: include_str!("../../migrations/postgres/0029_environment_settings.sql"),
        additive: true,
    },
    Migration {
        version: 30,
        name: "drop_managoat",
        sql: include_str!("../../migrations/postgres/0030_drop_managoat.sql"),
        additive: false,
    },
    Migration {
        version: 31,
        name: "agent_tokens",
        sql: include_str!("../../migrations/postgres/0031_agent_tokens.sql"),
        additive: true,
    },
    Migration {
        version: 32,
        name: "inference_broker",
        sql: include_str!("../../migrations/postgres/0032_inference_broker.sql"),
        additive: true,
    },
    Migration {
        version: 33,
        name: "product_posts",
        sql: include_str!("../../migrations/postgres/0033_product_posts.sql"),
        additive: true,
    },
    Migration {
        version: 34,
        name: "clients",
        sql: include_str!("../../migrations/postgres/0034_clients.sql"),
        additive: true,
    },
    Migration {
        version: 35,
        name: "planter_imports",
        sql: include_str!("../../migrations/postgres/0035_planter_imports.sql"),
        additive: true,
    },
    Migration {
        version: 36,
        name: "deploy_artifacts",
        sql: include_str!("../../migrations/postgres/0036_deploy_artifacts.sql"),
        additive: true,
    },
    Migration {
        version: 37,
        name: "redone",
        sql: include_str!("../../migrations/postgres/0037_redone.sql"),
        additive: true,
    },
    Migration {
        version: 38,
        name: "chat_alerts",
        sql: include_str!("../../migrations/postgres/0038_chat_alerts.sql"),
        additive: true,
    },
    Migration {
        version: 39,
        name: "link_previews",
        sql: include_str!("../../migrations/postgres/0039_link_previews.sql"),
        additive: true,
    },
    Migration {
        version: 40,
        name: "chat_reactions",
        sql: include_str!("../../migrations/postgres/0040_chat_reactions.sql"),
        additive: true,
    },
    Migration {
        version: 41,
        name: "chat_people",
        sql: include_str!("../../migrations/postgres/0041_chat_people.sql"),
        additive: true,
    },
    Migration {
        version: 42,
        name: "chat_notify",
        sql: include_str!("../../migrations/postgres/0042_chat_notify.sql"),
        additive: true,
    },
    Migration {
        version: 43,
        name: "chat_sidebars",
        sql: include_str!("../../migrations/postgres/0043_chat_sidebars.sql"),
        additive: true,
    },
    Migration {
        version: 44,
        name: "chat_saved",
        sql: include_str!("../../migrations/postgres/0044_chat_saved.sql"),
        additive: true,
    },
    Migration {
        version: 45,
        name: "chat_files",
        sql: include_str!("../../migrations/postgres/0045_chat_files.sql"),
        additive: true,
    },
    Migration {
        version: 46,
        name: "chat_topics_pins",
        sql: include_str!("../../migrations/postgres/0046_chat_topics_pins.sql"),
        additive: true,
    },
    Migration {
        version: 47,
        name: "chat_threads",
        sql: include_str!("../../migrations/postgres/0047_chat_threads.sql"),
        additive: true,
    },
    Migration {
        version: 48,
        name: "chat_channel_members",
        sql: include_str!("../../migrations/postgres/0048_chat_channel_members.sql"),
        additive: false,
    },
    Migration {
        version: 49,
        name: "hosted_waking",
        sql: include_str!("../../migrations/postgres/0049_hosted_waking.sql"),
        additive: true,
    },
    Migration {
        version: 50,
        name: "agent_sessions",
        sql: include_str!("../../migrations/postgres/0050_agent_sessions.sql"),
        additive: true,
    },
    Migration {
        version: 51,
        name: "forge_content_association",
        sql: include_str!("../../migrations/postgres/0051_forge_content_association.sql"),
        additive: true,
    },
    Migration {
        version: 52,
        name: "session_actions",
        sql: include_str!("../../migrations/postgres/0052_session_actions.sql"),
        additive: true,
    },
    Migration {
        version: 53,
        name: "pr_drafts",
        sql: include_str!("../../migrations/postgres/0053_pr_drafts.sql"),
        additive: true,
    },
    Migration {
        version: 54,
        name: "hosted_snapshots",
        sql: include_str!("../../migrations/postgres/0054_hosted_snapshots.sql"),
        additive: true,
    },
    Migration {
        version: 55,
        name: "session_feedback",
        sql: include_str!("../../migrations/postgres/0055_session_feedback.sql"),
        additive: true,
    },
    Migration {
        version: 56,
        name: "session_tries",
        sql: include_str!("../../migrations/postgres/0056_session_tries.sql"),
        additive: true,
    },
    Migration {
        version: 57,
        name: "session_merged",
        sql: include_str!("../../migrations/postgres/0057_session_merged.sql"),
        additive: true,
    },
    Migration {
        version: 58,
        name: "boards",
        sql: include_str!("../../migrations/postgres/0058_boards.sql"),
        additive: true,
    },
    Migration {
        version: 59,
        name: "board_row_agent",
        sql: include_str!("../../migrations/postgres/0059_board_row_agent.sql"),
        additive: true,
    },
];

/// The runner's own table, made before anything else.
pub const TABLE: &str = "platform_migrations";

const MAKE_TABLE: &str = "CREATE TABLE IF NOT EXISTS platform_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at INTEGER NOT NULL,
    adopted INTEGER NOT NULL DEFAULT 0,
    additive INTEGER NOT NULL DEFAULT 0
)";

/// [`MAKE_TABLE`] on Postgres.
const MAKE_TABLE_POSTGRES: &str = "CREATE TABLE IF NOT EXISTS platform_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at BIGINT NOT NULL,
    adopted BOOLEAN NOT NULL DEFAULT FALSE,
    additive BOOLEAN NOT NULL DEFAULT FALSE
)";

/// A migration a database records as applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub version: u32,
    pub checksum: String,
    pub additive: bool,
}

/// The migrations still to run on a database that has applied `applied`
/// (in version order), or why this build can't run on it. Applied
/// migrations past the ones this build knows are fine when each is
/// additive: the build is older than the database, and ignores them.
pub fn pending<'a>(set: &'a [Migration], applied: &[Applied]) -> anyhow::Result<&'a [Migration]> {
    let known = set.last().map_or(0, |m| m.version);
    for (i, a) in applied.iter().enumerate() {
        let Some(m) = set.get(i) else {
            if let Some(a) = applied[i..].iter().find(|a| !a.additive) {
                bail!(
                    "the platform's database is at migration {} and this build knows only up to {known}, \
                     and migration {} isn't additive: run a build that has it, or restore the database \
                     from before it",
                    applied.last().map_or(0, |a| a.version),
                    a.version
                );
            }
            break;
        };
        if m.version != a.version {
            bail!("the platform's database records migration {} where this build has {}", a.version, m.version);
        }
        if m.checksum() != a.checksum {
            bail!(
                "migration {} has changed since it ran on this database: a shipped migration is never edited, \
                 add the next number instead",
                m.file()
            );
        }
    }
    Ok(&set[applied.len().min(set.len())..])
}

/// What a start did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The version an existing schema was adopted at; 0 for none.
    pub adopted: u32,
    /// Whether adopting it took the old schema code first ([`super::legacy`]).
    pub upgraded: bool,
    /// The versions run.
    pub ran: Vec<u32>,
}

/// Brings the SQLite database on `conn` up to the latest migration.
pub fn run(conn: &mut Connection) -> anyhow::Result<Report> {
    run_to(conn, u32::MAX)
}

/// [`run`], stopping after version `to` (tests: a database as an older
/// build left it).
pub(crate) fn run_to(conn: &mut Connection, to: u32) -> anyhow::Result<Report> {
    run_set(conn, Dialect::Sqlite.migrations(), to)
}

/// [`run_to`] with the migrations `set` (tests: an older build's).
fn run_set(conn: &mut Connection, set: &[Migration], to: u32) -> anyhow::Result<Report> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(MAKE_TABLE)?;
    let mut report = Report::default();
    if applied(&tx)?.is_empty() {
        let (n, upgraded) = adopt(&tx, set)?;
        report.upgraded = upgraded;
        for m in &set[..n] {
            record(&tx, m, true)?;
            report.adopted = m.version;
        }
    }
    for m in pending(set, &applied(&tx)?)?.iter().take_while(|m| m.version <= to) {
        tx.execute_batch(m.sql).with_context(|| format!("running migration {}", m.file()))?;
        record(&tx, m, false)?;
        report.ran.push(m.version);
    }
    tx.commit()?;
    Ok(report)
}

/// Brings the Postgres database on `c` up to the latest migration (M143),
/// in one transaction holding the `migrations` lock, so two machines
/// starting together (M144) take turns. The versions run. Nothing is
/// adopted: Postgres starts empty, and M145 copies rows into it.
pub fn run_postgres(c: &mut super::Conn) -> anyhow::Result<Vec<u32>> {
    use super::sql::{Sql as _, params};
    let set = Dialect::Postgres.migrations();
    let tx = c.transaction()?;
    tx.lock("migrations")?;
    tx.execute_batch(MAKE_TABLE_POSTGRES)?;
    let applied =
        tx.query_map("SELECT version, checksum, additive FROM platform_migrations ORDER BY version", params![], |r| {
            Ok(Applied { version: r.get(0)?, checksum: r.get(1)?, additive: r.get(2)? })
        })?;
    let mut ran = Vec::new();
    for m in pending(set, &applied)? {
        tx.execute_batch(m.sql).with_context(|| format!("running migration {} on Postgres", m.file()))?;
        tx.execute(
            "INSERT INTO platform_migrations (version, name, checksum, applied_at, adopted, additive)
             VALUES (?1, ?2, ?3, ?4, FALSE, ?5)",
            params![m.version, m.name, m.checksum(), arugula_e2e::now_ms(), m.additive],
        )?;
        ran.push(m.version);
    }
    tx.commit()?;
    Ok(ran)
}

fn applied(c: &Connection) -> rusqlite::Result<Vec<Applied>> {
    let mut q = c.prepare("SELECT version, checksum, additive FROM platform_migrations ORDER BY version")?;
    q.query_map([], |r| Ok(Applied { version: r.get(0)?, checksum: r.get(1)?, additive: r.get(2)? }))?.collect()
}

fn record(c: &Connection, m: &Migration, adopted: bool) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO platform_migrations (version, name, checksum, applied_at, adopted, additive)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![m.version, m.name, m.checksum(), arugula_e2e::now_ms(), adopted, m.additive],
    )?;
    Ok(())
}

/// How many of `set` a database made before migrations already has (0
/// when it has none of their tables), and whether the old schema code had
/// to run first to get there. See the module's docs.
fn adopt(c: &Connection, set: &[Migration]) -> anyhow::Result<(usize, bool)> {
    let steps = steps(set)?;
    let known: BTreeSet<&String> = steps.iter().flat_map(|s| s.0.keys()).collect();
    let here = |c: &Connection| -> rusqlite::Result<Shape> {
        let present: Vec<String> = tables(c)?.into_iter().filter(|t| known.contains(t)).collect();
        Shape::of(c, &present)
    };
    let first = here(c)?;
    if first.0.is_empty() {
        return Ok((0, false));
    }
    if let Some(i) = steps.iter().position(|s| *s == first) {
        return Ok((i + 1, false));
    }
    // An older release's tables: what its own code would have done next.
    super::legacy::upgrade(c, arugula_e2e::now_ms()).context("the platform's schema code from before migrations")?;
    let upgraded = here(c)?;
    if let Some(i) = steps.iter().position(|s| *s == upgraded) {
        return Ok((i + 1, true));
    }
    let (i, diff) =
        steps.iter().map(|s| upgraded.diff(s)).enumerate().min_by_key(|(_, d)| d.len()).expect("a baseline");
    bail!(
        "the platform's tables are here but match no migration, even after the schema code from before \
         migrations ran, so nothing was changed. Against {} (the closest), they differ:\n  {}\n\
         Bring them to a migration's schema by hand, or restore the database.",
        set[i].file(),
        diff.join("\n  ")
    )
}

/// The schema after each of `set`, built in memory.
fn steps(set: &[Migration]) -> anyhow::Result<Vec<Shape>> {
    let c = Connection::open_in_memory()?;
    let mut out = Vec::new();
    for m in set {
        c.execute_batch(m.sql).with_context(|| format!("building migration {}", m.file()))?;
        out.push(Shape::of(&c, &tables(&c)?)?);
    }
    Ok(out)
}

/// Every table and virtual table any SQLite migration has made, whether a
/// later one dropped it or not: the only tables `drop-copies` drops
/// (`window::drop_copies`), so never one of the hub's.
pub(crate) fn ever_made() -> anyhow::Result<BTreeSet<String>> {
    let c = Connection::open_in_memory()?;
    let mut out = BTreeSet::new();
    for m in Dialect::Sqlite.migrations() {
        c.execute_batch(m.sql).with_context(|| format!("building migration {}", m.file()))?;
        out.extend(tables(&c)?);
    }
    Ok(out)
}

/// The tables and virtual tables in `main`: not SQLite's own, not an FTS5
/// table's shadow tables, and not the runner's.
pub(crate) fn tables(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut q = c.prepare(
        "SELECT name FROM pragma_table_list WHERE schema = 'main' AND type IN ('table', 'virtual')
             AND name NOT LIKE 'sqlite_%' AND name != 'platform_migrations' ORDER BY name",
    )?;
    q.query_map([], |r| r.get(0))?.collect()
}

/// A schema, normalised so that two databases the same queries run the same
/// on compare equal, however each got there: table by table, each column
/// (by name, whatever its position: a probe's `ADD COLUMN` puts it last),
/// `CHECK`, foreign key, index (a `UNIQUE` or primary key's by its columns)
/// and trigger, and a virtual table's definition, with comments, quoting
/// and spacing ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shape(BTreeMap<String, BTreeMap<String, String>>);

impl Shape {
    /// The shape of `tables` in the database on `c`.
    pub fn of(c: &Connection, tables: &[String]) -> rusqlite::Result<Self> {
        let mut out = BTreeMap::new();
        for t in tables {
            let mut facts = BTreeMap::new();
            let sql: String =
                c.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |r| r.get(0))?;
            let sql = normalise(&sql);
            if sql.to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE") {
                facts.insert("virtual table".into(), sql.splitn(4, ' ').nth(3).unwrap_or_default().to_owned());
            }
            for check in checks(&sql) {
                facts.insert(format!("check {check}"), String::new());
            }
            let mut q = c.prepare(r#"SELECT name, type, "notnull", dflt_value, pk FROM pragma_table_info(?1)"#)?;
            let cols = q.query_map([t], |r| {
                let (name, ty, notnull, default, pk): (String, String, bool, Option<String>, u32) =
                    (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
                let mut d = ty;
                if notnull {
                    d.push_str(" NOT NULL");
                }
                if let Some(v) = default {
                    d.push_str(&format!(" DEFAULT {v}"));
                }
                if pk > 0 {
                    d.push_str(&format!(" PRIMARY KEY #{pk}"));
                }
                Ok((format!("column {name}"), d))
            })?;
            for col in cols {
                let (k, v) = col?;
                facts.insert(k, v);
            }
            let mut q = c.prepare(
                r#"SELECT "from", "table", "to", on_update, on_delete, "match" FROM pragma_foreign_key_list(?1)"#,
            )?;
            let fks = q.query_map([t], |r| {
                let (from, table, to): (String, String, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?);
                let (up, del, m): (String, String, String) = (r.get(3)?, r.get(4)?, r.get(5)?);
                Ok(format!(
                    "foreign key {from} -> {table}({}) on update {up} on delete {del} match {m}",
                    to.unwrap_or_default()
                ))
            })?;
            for fk in fks {
                facts.insert(fk?, String::new());
            }
            let mut q = c.prepare(
                r#"SELECT l.name, l."unique", l.origin, m.sql FROM pragma_index_list(?1) l
                   LEFT JOIN sqlite_master m ON m.type = 'index' AND m.name = l.name"#,
            )?;
            let indexes = q
                .query_map([t], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get(3)?))
                })?
                .collect::<rusqlite::Result<Vec<(String, bool, String, Option<String>)>>>()?;
            for (name, unique, origin, sql) in indexes {
                match sql {
                    Some(sql) => facts.insert(format!("index {name}"), normalise(&sql)),
                    // A `UNIQUE` or primary key's own: its name is made up
                    // from the table's, so it's known by its columns.
                    None => {
                        let mut q = c.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
                        let cols: Vec<String> = q.query_map([&name], |r| r.get(0))?.collect::<Result<_, _>>()?;
                        let kind = if origin == "pk" { "primary key" } else { "unique" };
                        facts.insert(format!("{kind} ({})", cols.join(",")), format!("unique={unique}"))
                    }
                };
            }
            let mut q = c.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1")?;
            let triggers = q.query_map([t], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for trigger in triggers {
                let (name, sql) = trigger?;
                facts.insert(format!("trigger {name}"), normalise(&sql));
            }
            out.insert(t.clone(), facts);
        }
        Ok(Self(out))
    }

    /// How this differs from `want`, a line each.
    pub fn diff(&self, want: &Shape) -> Vec<String> {
        let mut out = Vec::new();
        let names: BTreeSet<&String> = self.0.keys().chain(want.0.keys()).collect();
        for t in names {
            let (have, wanted) = match (self.0.get(t), want.0.get(t)) {
                (Some(h), Some(w)) => (h, w),
                (None, _) => {
                    out.push(format!("table {t} is missing"));
                    continue;
                }
                (_, None) => {
                    out.push(format!("table {t} isn't in it"));
                    continue;
                }
            };
            let facts: BTreeSet<&String> = have.keys().chain(wanted.keys()).collect();
            for f in facts {
                match (have.get(f), wanted.get(f)) {
                    (Some(h), Some(w)) if h != w => out.push(format!("{t}: {f} is `{h}`, not `{w}`")),
                    (None, Some(w)) => out.push(format!("{t}: {f} is missing ({w})").replace(" ()", "")),
                    (Some(h), None) => out.push(format!("{t}: {f} isn't in it ({h})").replace(" ()", "")),
                    _ => {}
                }
            }
        }
        out
    }
}

/// `sql` with its comments, identifier quotes and spacing dropped: one
/// space between words, none before `(`, `)` or `,` or after `(` or `,`,
/// and no `;` at the end. String literals are kept as they are.
fn normalise(sql: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut chars = sql.chars().peekable();
    let end = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                let mut lit = String::from(ch);
                while let Some(s) = chars.next() {
                    lit.push(s);
                    if s == '\'' {
                        match chars.peek() {
                            Some('\'') => lit.push(chars.next().unwrap_or('\'')),
                            _ => break,
                        }
                    }
                }
                word.push_str(&lit);
            }
            '-' if chars.peek() == Some(&'-') => {
                end(&mut word, &mut tokens);
                chars.by_ref().find(|&s| s == '\n');
            }
            '"' | '`' | '[' | ']' => {}
            '(' | ')' | ',' | ';' => {
                end(&mut word, &mut tokens);
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => end(&mut word, &mut tokens),
            c => word.push(c),
        }
    }
    end(&mut word, &mut tokens);
    while tokens.last().is_some_and(|t| t == ";") {
        tokens.pop();
    }
    let mut out = String::new();
    for t in &tokens {
        let glued = out.is_empty() || out.ends_with(['(', ',']) || matches!(t.as_str(), "(" | ")" | "," | ";");
        if !glued {
            out.push(' ');
        }
        out.push_str(t);
    }
    out
}

/// The `CHECK (...)` constraints in a normalised `CREATE TABLE`.
fn checks(sql: &str) -> Vec<String> {
    let upper = sql.to_ascii_uppercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = upper[from..].find("CHECK(").map(|i| i + from) {
        let open = at + "CHECK".len();
        let mut depth = 0;
        let mut end = open;
        for (i, ch) in sql[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(sql[at..end].to_owned());
        from = end.max(open + 1);
    }
    out
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
