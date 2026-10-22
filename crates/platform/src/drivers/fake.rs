//! The fake driver (M219): for tests and testnet only. Its plan names the
//! source and the environment; its apply on a box (`arugulad --hosted`,
//! with `ARUGULA_HOSTED_FAKE_DRIVER=1`) says what it deploys, uses each of
//! the environment's secrets (printing them, which the box's redaction
//! must catch), and fails part way when the source's reference starts with
//! `fail`. It verifies by its run's exit, and rolls back.

use super::{Driver, Verdict, exited_0};
use crate::db::{
    deploys::{Deploy, Environment},
    runs::Run,
};

/// Its name, as environments name it.
pub const NAME: &str = "fake";

pub struct Fake;

impl Driver for Fake {
    fn name(&self) -> &'static str {
        NAME
    }

    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, _: &serde_json::Value) -> Verdict {
        exited_0(run)
    }

    fn rolls_back(&self) -> bool {
        true
    }
}
