//! The daemon's integration tests, as one binary: one per file linked 48
//! times over (125-200 MB each in a debug build, #466). Each file is a
//! module; `cargo test -p arugulad --test integration attach::` runs one,
//! nextest's `-E 'binary(integration) & test(/^attach::/)'` too.

mod agentd;
#[cfg(unix)]
mod replay;
#[cfg(unix)]
mod testnet;

mod agent_screens;
mod agents;
mod agents_real;
mod api;
#[cfg(feature = "labs")]
mod apps;
#[cfg(unix)]
mod arrange;
mod attach;
mod attention;
mod block_channels;
mod blocks;
#[cfg(all(unix, feature = "labs"))]
mod board_state;
mod chat_blocks;
#[cfg(unix)]
mod client_fixtures;
mod control_moves;
mod control_state;
mod conversations;
mod crash;
mod dialout;
mod doc;
mod doc_library;
mod doc_share_ask;
#[cfg(feature = "editor")]
mod editor_swarm;
#[cfg(feature = "editor")]
mod editors;
// Reader lists follow control's chat setting, not a Labs flag (native #203).
#[cfg(unix)]
mod chat_readers;
// #347: needs the forge too (Forgejo's block is one of its gates).
#[cfg(all(feature = "labs", feature = "forge"))]
mod flag_gates;
#[cfg(feature = "labs")]
mod forge;
// M170: drafts on a linked repo go up to the platform.
#[cfg(all(unix, feature = "forge"))]
mod forge_drafts;
#[cfg(feature = "forge")]
mod forge_github;
#[cfg(feature = "labs")]
mod forge_gitlab;
#[cfg(feature = "labs")]
mod forge_issues;
#[cfg(feature = "forge")]
mod forge_labs_off;
#[cfg(feature = "forge")]
mod forge_live;
#[cfg(all(unix, feature = "forge"))]
mod forge_pins;
// M203: a project's own issue as a pinned issue block.
#[cfg(all(unix, feature = "forge"))]
mod issue_pins;
// M168: control asks a machine to send a person's draft.
#[cfg(all(unix, feature = "forge"))]
mod forge_send_op;
#[cfg(feature = "forge")]
mod forges_github_real;
#[cfg(feature = "labs")]
mod forges_real;
mod fs;
#[cfg(feature = "labs")]
mod guest_ssh;
mod hand;
#[cfg(unix)]
mod hint_facts;
#[cfg(unix)]
mod hosted_sessions;
#[cfg(unix)]
mod hosted_work;
mod hosts;
#[cfg(feature = "editor")]
mod ide;
mod invite;
mod local_auth;
#[cfg(feature = "labs")]
mod machines;
mod mcp;
mod memory;
mod pipe;
mod program_status;
mod prompt;
mod questions;
mod reboot;
#[cfg(feature = "labs")]
mod recipes;
mod resident;
mod resume;
mod review;
mod session_history;
// Stand-in shell scripts for Claude Code, Node and npm.
#[cfg(unix)]
mod setup;
mod share;
mod sites;
mod ssh;
mod summaries;
mod team_answers;
mod telemetry;
mod upgrade;
mod version;
mod vm_layout;
#[cfg(feature = "labs")]
mod vm_reboot;
mod waits;
mod windows;
#[cfg(feature = "labs")]
mod workspace;
