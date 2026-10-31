//! M218's verifier: the only maker of a [`VerifiedApproval`], beside the
//! type in this crate so the compiler keeps it so (#314: its constructor is
//! `pub(crate)`, and deploys, which call [`verify`], are a crate of their
//! own).
//!
//! Since DK4 (#385, aru-126) a deploy's approval is v2: one user-verified
//! passkey assertion over its bytes, from a passkey in the account's
//! published log, and no device's signature. Which device the person used,
//! and which devices certified it, play no part: the ancestry checks
//! (`approving_device`, `owner_made_by_a_browser`, `browsers_all_the_way`)
//! are gone from here, having no other caller.

use arugula_control_common::{ApiError, err, refusal};
use arugula_e2e::{
    deploy::{Approval, MAX_LIFE_MS, V2},
    team::TeamRole,
};
use axum::http::StatusCode;

use crate::{
    Platform,
    db::{
        deploys::{PasskeyProof, VerifiedApproval},
        issues::IssueProject,
    },
    hub::PasskeyAssertion,
};

/// Why an approval was refused, for clients to act on: an approver who
/// isn't one of the project's owners.
pub const NOT_AN_OWNER: &str = "not_an_owner";
/// An approval of another operation than the deploy's.
pub const OTHER_OPERATION: &str = "other_operation";
/// Why an approval was refused: no passkey, or one the account's passkey
/// log doesn't bear out.
pub const PASSKEY: &str = "passkey";
/// An approval in a format the platform no longer takes (v1: a device's
/// signature beside the passkey), from a page loaded before DK4.
pub const OLD_APPROVAL: &str = "old_approval";

fn no_deploy() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such deploy")
}

/// The WebAuthn challenge an approval's passkey signs: SHA-256 of the
/// approval's bytes, base64url without padding.
pub fn challenge_of(a: &Approval) -> String {
    a.passkey_challenge()
}

/// `pk` as the shared crate's assertion.
pub fn assertion_of(pk: &PasskeyAssertion) -> arugula_e2e::passkey::Assertion {
    arugula_e2e::passkey::Assertion {
        id: pk.id.clone(),
        client_data_json: pk.client_data_json.clone(),
        authenticator_data: pk.authenticator_data.clone(),
        signature: pk.signature.clone(),
    }
}

/// Check a v2 approval (or deny) of a deploy in `project` as `a.account`'s,
/// at the platform's `now`:
/// - good for now and for 7 days at most;
/// - its account an owner of the project by the roster read now;
/// - over the deploy's operation as the log has it now;
/// - its passkey's assertion checked against the account's passkey log as
///   the hub publishes it (DK3), with the check anyone makes
///   (`arugula_e2e::passkey::verify`): the passkey in the log at `now` (the
///   platform's own record of when it came, never a time the approval
///   says), from one of control's sites or the platform page
///   (`Config::passkey_origins`), a `webauthn.get` over exactly these
///   bytes, the person verified, the signature good.
///
/// No device signs it, and nothing about the person's devices is looked
/// at (aru-126). An agent can't make one: it holds no passkey. The only
/// place a [`VerifiedApproval`] is made.
pub async fn verify(
    app: &Platform,
    a: &Approval,
    project: &IssueProject,
    passkey: Option<&PasskeyAssertion>,
    now: u64,
) -> Result<VerifiedApproval, ApiError> {
    if a.v != V2 || !a.well_formed() {
        return Err(refusal(
            StatusCode::BAD_REQUEST,
            OLD_APPROVAL,
            "deploys are approved with a passkey alone now: reload the page and approve again",
        ));
    }
    if a.expires <= now {
        return Err(err(StatusCode::BAD_REQUEST, "the approval has expired: approve it again"));
    }
    if a.expires > now + MAX_LIFE_MS {
        return Err(err(StatusCode::BAD_REQUEST, "an approval is good for 7 days at most"));
    }
    // The roster now, not a kept one: an owner who was just made an editor
    // doesn't approve.
    if crate::project_roles::role(app, &a.account, project, true).await? != Some(TeamRole::Owner) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            NOT_AN_OWNER,
            "only the project's owners approve or deny its deploys",
        ));
    }
    let (_, op) = app.db.deploy_operation(&a.deploy)?.ok_or_else(no_deploy)?;
    if op.hash() != a.op {
        return Err(refusal(
            StatusCode::CONFLICT,
            OTHER_OPERATION,
            "the approval signs another operation than this deploy's: look at it again",
        ));
    }
    // The person: a passkey's assertion over this approval's bytes, with
    // user verification. Whatever drives a browser can't make one.
    let no_passkey = |why: &str| refusal(StatusCode::FORBIDDEN, PASSKEY, why);
    let Some(pk) = passkey else {
        return Err(no_passkey("a deploy is approved or denied with a passkey: confirm it with one"));
    };
    // The hub gives the log, not a verdict. Its signature counter isn't
    // checked here: offline verifiers can't keep one, and synced passkeys
    // (most of them) report 0 anyway.
    let log = app.hub.passkey_log(&a.account).await?;
    if log.events.is_empty() {
        return Err(no_passkey("your account has no passkey: add one, then approve with it"));
    }
    let origins = app.cfg.passkey_origins();
    if let Err(r) = a.passkey_checks(&assertion_of(pk), &log, now, &origins) {
        return Err(no_passkey(&format!("the passkey didn't confirm this approval: {r}")));
    }
    let proof = PasskeyProof {
        id: pk.id.clone(),
        client_data_json: pk.client_data_json.clone(),
        authenticator_data: pk.authenticator_data.clone(),
        signature: pk.signature.clone(),
    };
    Ok(VerifiedApproval::from_checked(a, &op, proof))
}
