//! arugulad-review: M11's views for checking an agent's work, the **diff
//! block** ([`diff`]: what changed in a git repository) and the **file
//! block** ([`file`]: one file, followed live, at a line). What they share
//! (where their commands run, live only while drawn, their log) is the
//! base's `review` module, which the forge and Labs use too; see its docs.
//! The binary calls [`register`], and the daemon (`arugulad-base`) reaches
//! these only through the block kinds it adds, so an edit here rebuilds this
//! crate and the binary, not the daemon (#232).

pub mod diff;
pub mod file;

use arugula_proto::BlockType;

// What the code here names as `crate::…` and `super::…`: the mux's modules
// and the daemon's, and what the views share, as they named them when they
// were the daemon's `review/`.
use arugula_mux::{block, store};
use arugulad_base::review::{Live, Runner, SANDBOX_GONE, every, log, project};
use arugulad_base::{fs, lab_hooks, task};

/// The diff and file blocks.
struct Review;

/// Puts the diff and file blocks in the daemon. Call it before
/// `arugulad_base::main`.
pub fn register() {
    arugulad_base::features::register(Box::new(Review));
}

impl arugulad_base::features::Feature for Review {
    fn kinds(&self, kinds: &mut block::BlockKinds, _cx: &arugulad_base::features::KindsCx) {
        kinds.add(BlockType::Diff, block::Make(diff::Diff::create));
        kinds.add(BlockType::File, block::Make(file::FileView::create));
    }
}
