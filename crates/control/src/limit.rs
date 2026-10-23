//! Rate limits: for what anyone can do without signing in (start a
//! daemon's join, make an account with a passkey, finish a GitHub sign-in),
//! per client address, and for what a signed-in account or an enrolled
//! daemon can make control do (look people up, make teams and invites, ask
//! GitHub, push), per account or per daemon.
//!
//! The counting is [`Limits`] (`arugula_control_common::limit`), shared
//! with the platform; the limits are here. A feature's limits go with it
//! when it moves to the platform, which counts them itself.

pub use arugula_control_common::limit::{Limits, bucket};

/// How many per hour.
pub const JOINS: (&str, usize) = ("join", 30);
pub const ACCOUNTS: (&str, usize) = ("account", 10);
pub const SIGN_INS: (&str, usize) = ("sign-in", 60);
/// The issuer's `/oauth/*` (M228), per client address: a product's
/// server swaps codes and reads userinfo for everyone it signs in, so it's
/// generous; a code's guessing is out of reach at any rate.
pub const ISSUER: (&str, usize) = ("issuer", 6_000);
pub const LINKS: (&str, usize) = ("link", 240);
/// Block relay sockets opened, per client address (M50): a dev page's
/// worker and shim open one each per load, and again after the browser
/// stops an idle worker.
pub const BLOCK_RELAYS: (&str, usize) = ("block-relay", 3_000);
/// Requests to public previews, per client address (M187): a page and its
/// assets, many times over.
pub const PUBLIC_PREVIEWS: (&str, usize) = ("public-preview", 20_000);
/// Invite previews on the signed-out page (#103).
pub const INVITES: (&str, usize) = ("invite", 240);
/// Notifications relayed, all daemons together, per hour: a brake.
pub const PUSHES: (&str, usize) = ("push", 100_000);
/// Notifications one daemon relays per hour.
pub const DAEMON_PUSHES: (&str, usize) = ("daemon-push", 2_000);
/// Looking people up to share with, per account.
pub const PEOPLE: (&str, usize) = ("people", 120);
/// Devices asking into an account, per account.
pub const ENROLLS: (&str, usize) = ("enroll", 30);
/// Teams made, per account.
pub const TEAMS: (&str, usize) = ("team", 20);
/// Invite links made, per account.
pub const TEAM_INVITES: (&str, usize) = ("team-invite", 120);
/// `forge.watch` messages, per daemon.
pub const FORGE_WATCHES: (&str, usize) = ("forge-watch", 120);
// Repositories looked up at GitHub for the first time are counted by the
// platform since M161 (`arugula_platform::forge::FORGE_LOOKUPS`).
/// Shares offered to other accounts, per daemon (each shows a prompt).
pub const OFFERS: (&str, usize) = ("offer", 120);
/// Telemetry reports (M100), per client address: many installs can share
/// one (an office, a NAT).
pub const TELEMETRY: (&str, usize) = ("telemetry", 600);
/// Telemetry reports per install id, or per daemon when signed: an
/// install sends one when it starts and one a day, so this is plenty.
pub const INSTALL_REPORTS: (&str, usize) = ("install-report", 30);
/// Control's page's reports (M104), per account: one a minute from each
/// open page, and one as it closes, so a few tabs' worth.
pub const PAGE_REPORTS: (&str, usize) = ("page-report", 300);
