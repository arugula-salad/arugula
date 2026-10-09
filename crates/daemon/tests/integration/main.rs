//! The daemon's integration tests, as one binary: one per file linked 48
//! times over (125-200 MB each in a debug build, #466). Each file is a
//! module; `cargo test -p arugulad --test integration tmux::` runs one,
//! nextest's `-E 'binary(integration) & test(/^tmux::/)'` too.

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
mod attach;
mod attention;
mod blocks;
#[cfg(feature = "labs")]
mod calls;
#[cfg(unix)]
mod client_fixtures;
mod control_moves;
mod control_state;
mod conversations;
mod dialout;
mod editor_swarm;
mod editors;
#[cfg(feature = "labs")]
mod forge;
mod forge_github;
#[cfg(feature = "labs")]
mod forge_gitlab;
#[cfg(feature = "labs")]
mod forge_issues;
mod forge_labs_off;
mod forge_live;
mod forges_github_real;
#[cfg(feature = "labs")]
mod forges_real;
#[cfg(feature = "labs")]
mod fountain;
mod fs;
#[cfg(feature = "labs")]
mod guest_ssh;
mod hand;
mod hosts;
mod ide;
mod invite;
#[cfg(not(feature = "labs"))]
mod labs_off;
mod local_auth;
#[cfg(feature = "labs")]
mod machines;
mod mcp;
mod memory;
mod pipe;
mod prompt;
mod questions;
mod reboot;
#[cfg(feature = "labs")]
mod recipes;
mod resident;
mod resume;
mod review;
mod share;
mod sites;
mod ssh;
mod summaries;
mod team_answers;
#[cfg(feature = "labs")]
mod threads;
mod tmux;
mod upgrade;
mod vm_layout;
#[cfg(feature = "labs")]
mod vm_reboot;
mod windows;
#[cfg(feature = "labs")]
mod workspace;
