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
//! - [`tries`]: Try 3, a session tried three ways and one kept (M264).
//! - [`review`]: `@codex review`, a read-only session that reviews one's
//!   pushed diff and posts what it found in the thread (M264).
//! - [`answer`]: each turn's last message, posted in its thread.
//! - [`typed`]: a turn someone typed in its block, said in its thread
//!   (Arugula #215).
//! - [`merged`]: its PR merged, so its thread is done (M265).
//! - [`model`]: which model its agent uses (Arugula #215).
//! - [`pool`]: which of the project's workspaces it runs on (Arugula
//!   #214), and [`moves`], Move to… another.
//! - [`asks`]: what still needs its asker, as a form and a mention of them
//!   (Arugula #214).
//! - [`wave`]: a project's sessions in one list, a session's last answer
//!   whole, and a wait for any of them to finish (Fleet 5).
//!
//! **How sessions end besides** (each `stopped` by control, with a
//! `reason` in its event's detail):
//! - its workspace is deleted (`workspace_gone`): when it's asked
//!   ([`crate::hosted::routes::delete`]), and its job sees it too;
//! - its asker leaves the project (`asker_left`): when the team's roster
//!   comes in (`Db::project_roster`);
//! - its PR merges (`merged`, M265): when the mirror sees it, or once the
//!   turn under way is done ([`merged`], [`job`]);
//! - its project goes (only with its account or team): the session goes
//!   with it (`db::sessions::erase_account`).

pub mod answer;
pub mod asks;
pub mod feedback;
pub mod hydrate;
pub mod issues;
pub mod job;
pub mod merged;
pub mod model;
pub mod moves;
pub mod pool;
pub mod pr;
pub mod push;
pub mod review;
pub mod tries;
pub mod typed;
pub mod wave;
