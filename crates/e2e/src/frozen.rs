//! The domains signed or hashed into what devices, daemons and control
//! keep (certificates, rosters, proofs, the key file, the channel's
//! prologue) stay "illogical …" after the rename to Arugula (#504): a
//! changed byte orphans every stored signature and key. The expected
//! bytes are hex, so a find-and-replace across the repo can't change them
//! along with the code. Block grants (M50) came after the rename and say
//! "arugula …"; they're pinned here too, since browsers sign the same bytes,
//! and so are run events (M210) and session events (M254), which daemons
//! sign and anyone may check.

use serde_json::json;

use crate::{
    block::Grant,
    cert::{Cert, Revocation, join_code, join_proof_body, request_auth, verify_hex},
    channel::prologue,
    deploy::{Approval, Decision, Operation},
    keys::{DeviceKeys, HEADER, device_id},
    push::PushSub,
    team::{Invite, Member, Move, Roster, TeamLock, TeamPin},
};

fn text(hex: &str) -> String {
    String::from_utf8(hex::decode(hex).unwrap()).unwrap()
}

fn first_line(body: String) -> String {
    body.lines().next().unwrap().to_owned()
}

#[test]
fn signed_domains_never_change() {
    let zero = "00".repeat(32);
    let cert: Cert = serde_json::from_value(json!({
        "v": 1, "account": "a", "device": "d", "kind": "browser", "name": "n",
        "noise": zero, "sign": zero, "created": 1, "approver": "d",
    }))
    .unwrap();
    let revocation: Revocation =
        serde_json::from_value(json!({ "v": 1, "account": "a", "device": "d", "at": 1, "by": "d", "sig": "" }))
            .unwrap();
    let invite: Invite = serde_json::from_value(
        json!({ "team": "t", "role": "editor", "expires": 1, "key": "k", "by": "d", "sig": "" }),
    )
    .unwrap();
    let member: Member =
        serde_json::from_value(json!({ "account": "a", "root": "r", "role": "editor", "name": "n" })).unwrap();
    let roster: Roster = serde_json::from_value(
        json!({ "v": 1, "team": "t", "name": "n", "version": 1, "at": 1, "members": [], "by": "d", "sig": "" }),
    )
    .unwrap();
    let push: PushSub = serde_json::from_value(json!({
        "v": 1, "account": "a", "device": "d", "endpoint": "e", "p256dh": "p", "auth": "x", "at": 1, "sig": "",
    }))
    .unwrap();
    let grant = Grant {
        daemon: "d".into(),
        block: "b".into(),
        key: zero.clone(),
        expires: 1,
        by: "d".into(),
        sig: String::new(),
    };
    let public = crate::preview::PublicGrant {
        daemon: "d".into(),
        port: 1,
        preview: "p".into(),
        key: zero.clone(),
        expires: 1,
        sig: String::new(),
    };
    let run = crate::run::Event {
        run: "r",
        seq: 1,
        from: None,
        to: crate::run::State::Pending,
        at: 1,
        detail: "{}",
        daemon: "d",
    };
    let session = crate::session::Event {
        session: "s",
        n: 1,
        kind: crate::session::Kind::State,
        from: crate::session::State::Queued,
        to: crate::session::State::Working,
        at: 1,
        detail: "{}",
        daemon: "d",
    };
    let op = Operation {
        project: "p".into(),
        environment: "e".into(),
        environment_name: "n".into(),
        plan: "p".into(),
        source_kind: "commit".into(),
        source_ref: "r".into(),
        source_where: "w".into(),
        planned: "h".into(),
    };
    let approval = Approval {
        deploy: "d".into(),
        op: "o".into(),
        account: "a".into(),
        decision: Decision::Once,
        expires: 1,
        by: "d".into(),
        sig: String::new(),
    };
    let project_grant: crate::project_grant::ProjectGrant = serde_json::from_value(json!({
        "id": "i", "project": "p", "owner": "account:a", "delegation": "d", "caps": ["runs.start"], "made": 1, "by": "d",
    }))
    .unwrap();
    let grant_revoke = crate::project_grant::GrantRevoke {
        project: "p".into(),
        delegation: "d".into(),
        running: crate::project_grant::Running::Finish,
        at: 1,
        by: "d".into(),
        sig: String::new(),
    };
    let pin = TeamPin { team: "t".into(), founder: "a".into(), founder_root: "r".into() };
    let lock = TeamLock { team: "t".into(), locked: true, version: 1, at: 1, by: "d".into(), sig: String::new() };

    let cases = [
        (first_line(cert.body()), "696c6c6f676963616c20646576696365207631"),
        (first_line(revocation.body()), "696c6c6f676963616c207265766f6b65207631"),
        (first_line(join_proof_body(&cert, 1)), "696c6c6f676963616c206a6f696e2070726f6f66207631"),
        (first_line(invite.body()), "696c6c6f676963616c207465616d20696e76697465207631"),
        (first_line(invite.redeem_body(1, &member)), "696c6c6f676963616c207465616d2072656465656d207631"),
        (first_line(pin.join_body("d")), "696c6c6f676963616c207465616d206a6f696e207631"),
        (first_line(Move::body("d", None, 1)), "696c6c6f676963616c206d616368696e65206d6f7665207631"),
        (first_line(roster.body()), "696c6c6f676963616c207465616d207631"),
        (first_line(push.body()), "696c6c6f676963616c2070757368207631"),
        (HEADER.to_owned(), "696c6c6f676963616c2d6465766963652d6b65792031"),
        // Born after the rename (M50), so "arugula"; frozen all the same.
        (first_line(grant.body()), "61727567756c6120626c6f636b206772616e74207631"),
        // Public previews' grants (M187), signed with control's hosted key.
        (first_line(public.body()), "61727567756c61207075626c69632070726576696577206772616e74207631"),
        // Run events (M210), which hosted daemons sign.
        (first_line(run.body()), "61727567756c612072756e206576656e74207631"),
        // A team's lock, signed by an owner's page.
        (first_line(lock.body()), "61727567756c61207465616d206c6f636b207631"),
        // Agent sessions' events (M254), which hosted daemons sign.
        (first_line(session.body()), "61727567756c612073657373696f6e206576656e74207631"),
        // M218's deploy operation and approval.
        (first_line(op.body()), "61727567756c61206465706c6f79206f70207631"),
        (first_line(approval.body()), "61727567756c61206465706c6f7920617070726f76616c207631"),
        // The standing project grant and its revoke (M211; the revoke is v2).
        (first_line(project_grant.body()), "61727567756c612070726f6a656374206772616e74207631"),
        (first_line(grant_revoke.body()), "61727567756c612070726f6a656374206772616e74207265766f6b65207632"),
    ];
    for (got, want) in cases {
        assert_eq!(got, text(want), "a frozen domain changed (#504)");
    }
    assert_eq!(prologue("d"), [hex::decode("696c6c6f676963616c2f310a").unwrap(), b"d\n".to_vec()].concat());
    // `illogical device id` and `illogical join` are hashed in.
    assert_eq!(device_id(&[0; 32], &[0; 32]), "9b018a3082c1388a");
    assert_eq!(join_code(&cert), "0AERR-RNGF7");
}

#[test]
fn signed_requests_keep_their_domain() {
    let keys = DeviceKeys::generate();
    let auth = request_auth(&keys, "GET", "/x", b"");
    let [v, _id, ms, nonce, sig] = auth.split(' ').collect::<Vec<_>>()[..] else { panic!("{auth}") };
    assert_eq!(v, "v2");
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b""));
    let domain = text("696c6c6f676963616c206461656d6f6e20617574682076320a");
    let msg = format!("{domain}GET\n/x\n{ms}\n{nonce}\n{digest}\n");
    assert!(
        verify_hex(&hex::encode(keys.sign_public()), msg.as_bytes(), sig),
        "`illogical daemon auth v2` changed (#504)"
    );
}
