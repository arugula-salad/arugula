//! Who may share a Doc (#1066): the API's check on a Doc block's sharing
//! methods (`api::call`) and the Docs' own (`arugulad-docs`, `docs/link.rs`)
//! say the same, so the words are here, where both reach them.

/// What an agent is told, and the owner's CLI when an agent runs it.
pub const AGENT_ASKS: &str = "an agent doesn't share a Doc: ask with the doc tool's share";
/// Said to someone who isn't the owner, and to a team's other owner: the
/// link's key is the sharing account's.
pub const OWNER_ONLY: &str = "sharing a Doc is the machine's own account's";
/// The block methods that publish, change or read back a link.
pub const METHODS: [&str; 4] = ["doc_share", "doc_share_update", "doc_unshare", "doc_shares"];
