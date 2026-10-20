//! Cloud workspaces (aru-046): workspaces, tabs, the leased worker, secrets
//! and Managoat. M137 moves them here from `crates/control/src/cloud/`
//! (with their queries from `db/cloud.rs`), reaching the hub only through
//! [`HubPort`](crate::HubPort): the principal, account names, and push for
//! approvals.
