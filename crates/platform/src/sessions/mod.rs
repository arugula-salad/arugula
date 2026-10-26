//! Thread sessions (M256, thread = session, Arugula #129): what follows a
//! session once `@arugula` made one in a thread ([`crate::chat::sessions`]).
//! Its storage and the one place it moves are [`crate::db::sessions`].
//!
//! - [`hydrate`]: its first prompt, the thread it was asked in.
//! - [`job`]: its `session` job, which wakes its box while it waits and
//!   ends what nothing will take.
//! - [`push`]: the token its box pushes the session's branch with (M259).
//! - [`pr`]: its pull request, which its asker drafts and sends (M259).
//! - [`feedback`]: CI's failures and reviews on its branch, back to it as
//!   its next prompt (M260).
//!
//! **How sessions end besides** (each `stopped` by control, with a
//! `reason` in its event's detail):
//! - its workspace is deleted (`workspace_gone`): when it's asked
//!   ([`crate::hosted::routes::delete`]), and its job sees it too;
//! - its asker leaves the project (`asker_left`): when the team's roster
//!   comes in (`Db::project_roster`);
//! - its project goes (only with its account or team): the session goes
//!   with it (`db::sessions::erase_account`).

pub mod feedback;
pub mod hydrate;
pub mod job;
pub mod pr;
pub mod push;
