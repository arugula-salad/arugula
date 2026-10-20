//! Chat (aru-051): rooms, members, messages, reads, readers, search, import
//! and the live event hub. M138 moves it here from
//! `crates/control/src/chat.rs` (with its queries from `db/chat.rs`),
//! reaching the hub only through [`HubPort`](crate::HubPort): team members,
//! batched reachability, Labs flags, push for mentions, and the change feed
//! for roster changes.
