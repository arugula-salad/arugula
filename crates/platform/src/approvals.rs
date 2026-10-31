//! M218's verifier: the only maker of a [`VerifiedApproval`], beside the
//! type in this crate so the compiler keeps it so (#314: its constructor is
//! `pub(crate)`, and deploys, which call [`verify`], are a crate of their
//! own). Also the checks a run's approval shares (M211, M212): the
//! approving device and an owner a browser made.

use arugula_control_common::{ApiError, err, refusal};
use arugula_e2e::{
    Trust,
    deploy::{Approval, MAX_LIFE_MS},
};
use axum::http::StatusCode;

use crate::{
    Platform,
    auth::NOT_A_DEVICE,
    db::deploys::{PasskeyProof, VerifiedApproval},
    hub::PasskeyAssertion,
};

/// Why an approval was refused, for clients to act on: an approver who
/// isn't one of the project's owners.
pub const NOT_AN_OWNER: &str = "not_an_owner";
/// An approval of another operation than the deploy's.
pub const OTHER_OPERATION: &str = "other_operation";
/// Why an approval was refused: no passkey, or one the hub didn't accept.
pub const PASSKEY: &str = "passkey";

fn no_deploy() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such deploy")
}

/// The WebAuthn challenge an approval's passkey signs: SHA-256 of the
/// approval's bytes, base64url without padding.
pub fn challenge_of(a: &Approval) -> String {
    a.passkey_challenge()
}

/// The device of `account` that may sign an approval (a deploy's here; a
/// run's for M211 and M212, over their own bytes): one of its devices that
/// counts, by what the hub keeps of them, evaluated here
/// (`arugula_e2e::Trust::evaluate`: chained to the account's root, not
/// revoked), and a browser's (a browser's, a phone's or the desktop app's
/// key, held non-extractable or derived from a passkey). Never a CLI's,
/// whose key is a file an agent on its machine can read, a machine's or a
/// recovery code. Its certificate, whose `sign` key checks the signature.
pub async fn approving_device(app: &Platform, account: &str, device: &str) -> Result<arugula_e2e::Cert, ApiError> {
    let not_a_device = |why: &str| refusal(StatusCode::FORBIDDEN, NOT_A_DEVICE, why);
    let Some(t) = app.hub.device_trust(account).await? else {
        return Err(not_a_device("your account has no devices yet: sign in on your phone or in a browser first"));
    };
    let trusted = Trust { account: account.to_owned(), root: t.root.clone() }.evaluate(&t.certs, &t.revocations);
    let Some(cert) = trusted.get(device) else {
        return Err(not_a_device(
            "that isn't one of your devices (or it was removed). Approve from a browser or a phone you enrolled by \
             signing in with a passkey",
        ));
    };
    if !browsers_all_the_way(account, &t.root, &t.certs, device, &t.legacy_app_children) {
        return Err(not_a_device(
            "this device can't approve deploys: it, or a device that approved it on the way back to your account's \
             first device, is a CLI's or the desktop app's, whose keys a program on its computer can read. Approve \
             from a browser or a phone you enrolled by signing in with a passkey",
        ));
    }
    Ok(cert.clone())
}

/// Whether `account`'s owner role in `team` was given by a browser: the
/// roster version that made it an owner (the latest run of versions where
/// it is one) is signed, over its bytes, by a device of an owner of the
/// version before (of the founder's, for the first), and that device is a
/// browser certified by browsers to that owner's root as the roster pins it
/// ([`browsers_all_the_way`]). An owner a CLI's or the app's key made can't
/// approve: those keys may be a program's.
pub async fn owner_made_by_a_browser(app: &Platform, team: &str, account: &str) -> Result<(), ApiError> {
    use arugula_e2e::team::TeamRole as R;
    let refuse = |why: &str| refusal(StatusCode::FORBIDDEN, NOT_AN_OWNER, why);
    let rosters = app.hub.team_rosters(team).await?;
    let owner = |r: &arugula_e2e::team::Roster| r.member(account).is_some_and(|m| m.role == R::Owner);
    if !rosters.last().is_some_and(owner) {
        return Err(refuse("only the team's owners approve or deny its deploys"));
    }
    let mut i = rosters.len() - 1;
    while i > 0 && owner(&rosters[i - 1]) {
        i -= 1;
    }
    let made = &rosters[i];
    // Who may have signed it: the owners before it, or (the team's first
    // version) its own owners, the founder among them.
    let before = if i == 0 { made } else { &rosters[i - 1] };
    let body = made.body();
    for m in before.members.iter().filter(|m| m.role == R::Owner) {
        let Some(t) = app.hub.device_trust(&m.account).await? else { continue };
        let trusted = Trust { account: m.account.clone(), root: m.root.clone() }.evaluate(&t.certs, &t.revocations);
        let Some(signer) = trusted.get(&made.by) else { continue };
        if !arugula_e2e::cert::verify_hex(&signer.sign, body.as_bytes(), &made.sig) {
            continue;
        }
        return match browsers_all_the_way(&m.account, &m.root, &t.certs, &made.by, &t.legacy_app_children) {
            true => Ok(()),
            false => Err(refuse(
                "your owner role in this team was given by a CLI's or the app's key, which a program could hold, so it \
                 doesn't approve deploys: ask an owner to give it again from a browser or a phone they enrolled by \
                 signing in with a passkey",
            )),
        };
    }
    Err(refuse("the roster that made you an owner isn't signed by a device of an owner before it"))
}

/// Whether `device` is a browser's or a phone's own (`Cert::is_browser`:
/// not a CLI's, not the desktop app's, new kind or old), and every device
/// that certified it back to the account's root is too, the root included
/// (`Trust::chained` with `browser_link`, as the hub checks who may add an
/// owner). Recovery codes may be links on the way (the person's own, on
/// paper, made only by browsers), never the signing device or the root. A
/// CLI's or the app's keys are files a program on their computer can read,
/// so a device they let in, or that comes after one they let in, doesn't
/// count. Every certificate kept for an id on the way is checked; a link
/// with none, or a chain longer than 64, is refused. An app window from
/// before it had a kind counts just above a browser it approved back then
/// (`children`, [`DeviceTrust::legacy_app_children`]), the root included,
/// and nowhere else.
///
/// [`DeviceTrust::legacy_app_children`]: crate::hub::DeviceTrust::legacy_app_children
fn browsers_all_the_way(
    account: &str,
    root: &str,
    certs: &[arugula_e2e::Cert],
    device: &str,
    children: &[String],
) -> bool {
    arugula_e2e::deploy::approver_chained(account, root, certs, device, children)
}

/// Check an approval as `account`'s: good for now and not too long, signed
/// by their [`approving_device`] over exactly these bytes, and over the
/// deploy's operation as the log has it now. The only place a
/// [`VerifiedApproval`] is made; its device's name, for the record.
pub async fn verify(
    app: &Platform,
    a: &Approval,
    passkey: Option<&PasskeyAssertion>,
    now: u64,
) -> Result<(VerifiedApproval, String), ApiError> {
    if a.expires <= now {
        return Err(err(StatusCode::BAD_REQUEST, "the approval has expired: approve it again"));
    }
    if a.expires > now + MAX_LIFE_MS {
        return Err(err(StatusCode::BAD_REQUEST, "an approval is good for 7 days at most"));
    }
    let cert = approving_device(app, &a.account, &a.by).await?;
    if !a.signed_by(&cert.sign) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "bad_signature",
            "the signature isn't that device's over this deploy's operation",
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
    // The person, not only their browser: a passkey's assertion over this
    // approval's bytes, with user verification. A browser's stored key
    // signs without one, so whatever drives the browser (automation, its
    // profile on disk) can't approve alone.
    let no_passkey = |why: &str| refusal(StatusCode::FORBIDDEN, PASSKEY, why);
    let Some(pk) = passkey else {
        return Err(no_passkey("a deploy is approved or denied with a passkey: confirm it with one"));
    };
    // Checked here against the account's passkey log as the hub publishes
    // it (DK3, aru-126), with the check anyone makes of the proof kept
    // below: the passkey live now (the platform's own time), its RP and
    // origin one of control's sites', UP and UV, the signature. The hub
    // gives the log, not a verdict.
    let log = app.hub.passkey_log(&a.account).await?;
    if log.account != a.account {
        return Err(no_passkey("the hub sent another account's passkey log"));
    }
    let assertion = arugula_e2e::passkey::Assertion {
        id: pk.id.clone(),
        client_data_json: pk.client_data_json.clone(),
        authenticator_data: pk.authenticator_data.clone(),
        signature: pk.signature.clone(),
    };
    if let Err(r) = arugula_e2e::passkey::verify(&log, &assertion, &challenge_of(a), now, &app.cfg.passkey_origins()) {
        return Err(no_passkey(&format!("the passkey didn't confirm this approval: {r}")));
    }
    let proof = PasskeyProof {
        id: pk.id.clone(),
        client_data_json: pk.client_data_json.clone(),
        authenticator_data: pk.authenticator_data.clone(),
        signature: pk.signature.clone(),
    };
    Ok((VerifiedApproval::from_checked(a, &op, proof), cert.name))
}
