//! Each account's passkey log (DK3, #384; aru-126): every passkey added to
//! or removed from it, as `arugula_e2e::passkey` events, hash-chained and
//! append-only. Triggers refuse an update or a delete, as `deploy_events`'
//! do (aru-103), and an account's deletion keeps them, so an approval made
//! before it still checks out. The `passkeys` table stays what sign-in
//! reads (and its counters and notes, which change); a passkey goes in
//! both, in one transaction.
//!
//! Served at `GET /api/accounts/{id}/passkeys/log`, and to the platform as
//! `HubPort::passkey_log`.

use arugula_e2e::passkey::{Assertion, Change, Draft, Event, Log, NO_AAGUID};
use rusqlite::{Connection, OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS passkey_log (
    account TEXT NOT NULL,
    seq INTEGER NOT NULL,
    prev TEXT NOT NULL,
    kind TEXT NOT NULL,
    credential TEXT NOT NULL,
    rp TEXT NOT NULL,
    alg INTEGER NOT NULL,
    key_format TEXT NOT NULL,
    public_key TEXT NOT NULL,
    aaguid TEXT NOT NULL,
    at INTEGER NOT NULL,
    by_what TEXT NOT NULL,
    proof TEXT,
    hash TEXT NOT NULL,
    PRIMARY KEY (account, seq)
);
CREATE INDEX IF NOT EXISTS passkey_log_credential ON passkey_log (credential);
CREATE TRIGGER IF NOT EXISTS passkey_log_append_only BEFORE UPDATE ON passkey_log BEGIN
    SELECT RAISE(ABORT, 'passkey_log is append-only');
END;
CREATE TRIGGER IF NOT EXISTS passkey_log_kept BEFORE DELETE ON passkey_log BEGIN
    SELECT RAISE(ABORT, 'passkey_log is append-only, and outlives its account');
END;
-- Only the account's next event goes in: its seq the count so far, its prev
-- the hash of the one before (64 zeros first). So `INSERT OR REPLACE`,
-- which deletes without firing the delete trigger, can't rewrite one.
CREATE TRIGGER IF NOT EXISTS passkey_log_next BEFORE INSERT ON passkey_log
WHEN NEW.seq != (SELECT COUNT(*) FROM passkey_log WHERE account = NEW.account)
    OR NEW.prev != COALESCE(
        (SELECT hash FROM passkey_log WHERE account = NEW.account AND seq = NEW.seq - 1),
        '0000000000000000000000000000000000000000000000000000000000000000')
BEGIN
    SELECT RAISE(ABORT, 'passkey_log takes only its next event');
END;
";

/// The setting that says when this hub began keeping passkey logs: the
/// first backfill's time. Removals before then are in no log.
const BEGAN: &str = "passkey_log_began";

/// Why an event didn't go in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAppended {
    /// It was made for a place in the log (a signed one) that isn't the
    /// next any more: another event came first. Ask again.
    Moved,
    /// The log wouldn't hold together with it (`check_chain`): a proof
    /// that isn't a live passkey's over it, say. One such event would break
    /// every check against the log for good, so it never goes in.
    Breaks,
}

const COLUMNS: &str =
    "account, seq, prev, kind, credential, rp, alg, key_format, public_key, aaguid, at, by_what, proof, hash";

fn event_of(r: &rusqlite::Row) -> rusqlite::Result<Event> {
    let kind: String = r.get(3)?;
    let proof: Option<String> = r.get(12)?;
    Ok(Event {
        account: r.get(0)?,
        seq: r.get::<_, i64>(1)? as u64,
        prev: r.get(2)?,
        event: if kind == "removed" { Change::Removed } else { Change::Added },
        credential: r.get(4)?,
        rp: r.get(5)?,
        alg: r.get(6)?,
        key_format: r.get(7)?,
        key: r.get(8)?,
        aaguid: r.get(9)?,
        at: r.get::<_, i64>(10)? as u64,
        by: r.get(11)?,
        proof: proof.and_then(|p| serde_json::from_str::<Assertion>(&p).ok()),
        hash: r.get(13)?,
    })
}

/// The account's last event.
fn head(c: &Connection, account: &str) -> anyhow::Result<Option<Event>> {
    Ok(c.query_row(
        &format!("SELECT {COLUMNS} FROM passkey_log WHERE account = ?1 ORDER BY seq DESC LIMIT 1"),
        params![account],
        event_of,
    )
    .optional()?)
}

fn events(c: &Connection, account: &str) -> anyhow::Result<Vec<Event>> {
    let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM passkey_log WHERE account = ?1 ORDER BY seq"))?;
    let rows = q.query_map(params![account], event_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Append `e` to its account's log. `pinned`: it was made (and signed)
/// for the place after the head it names, which must still be the head;
/// otherwise it goes after the head there is now, no earlier than it.
///
/// Whatever it is, the log must hold together with it (`check_chain`, its
/// proof checked against the log as it was), or it doesn't go in.
pub(super) fn append(c: &Connection, e: Event, pinned: bool) -> anyhow::Result<Result<Event, NotAppended>> {
    let last = head(c, &e.account)?;
    let e = if pinned {
        let (seq, prev) = last.as_ref().map_or((0, arugula_e2e::passkey::ZERO), |l| (l.seq + 1, l.hash.as_str()));
        if e.seq != seq || e.prev != prev || last.as_ref().is_some_and(|l| e.at < l.at) {
            return Ok(Err(NotAppended::Moved));
        }
        anyhow::ensure!(e.hash == e.compute_hash(), "a passkey event whose hash isn't its own");
        e
    } else {
        // A clock that went back doesn't make the log go back.
        let at = last.as_ref().map_or(e.at, |l| e.at.max(l.at));
        Event { at, ..e }.placed_after(last.as_ref())
    };
    anyhow::ensure!(arugula_e2e::passkey::By::parse(&e.by).is_some(), "a passkey event by {:?}", e.by);
    let mut after = Log { account: e.account.clone(), events: events(c, &e.account)?, began: None };
    after.events.push(e.clone());
    if arugula_e2e::passkey::check_chain(&after).is_err() {
        return Ok(Err(NotAppended::Breaks));
    }
    let proof = e.proof.as_ref().map(serde_json::to_string).transpose()?;
    c.execute(
        &format!(
            "INSERT INTO passkey_log ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
        ),
        params![
            e.account,
            e.seq as i64,
            e.prev,
            e.event.as_str(),
            e.credential,
            e.rp,
            e.alg,
            e.key_format,
            e.key,
            e.aaguid,
            e.at as i64,
            e.by,
            proof,
            e.hash
        ],
    )?;
    Ok(Ok(e))
}

/// The `added` events of the account's passkeys that are live now, oldest
/// first.
pub(super) fn live(c: &Connection, account: &str) -> anyhow::Result<Vec<Event>> {
    let mut live: Vec<Event> = vec![];
    for e in events(c, account)? {
        match e.event {
            Change::Added => live.push(e),
            Change::Removed => live.retain(|l| l.credential != e.credential),
        }
    }
    Ok(live)
}

/// Log the removal of `credential` from `account`, `by` this, at `at`:
/// whether it was live in the log.
pub(super) fn remove(c: &Connection, account: &str, credential: &str, at: u64, by: &str) -> anyhow::Result<bool> {
    let Some(added) = live(c, account)?.into_iter().find(|e| e.credential == credential) else { return Ok(false) };
    append(c, added.removal(at, by), false)?.map_err(|_| anyhow::anyhow!("an unpinned event can't move"))?;
    Ok(true)
}

/// The AAGUID behind a provider's name as `passkey::provider` gives it,
/// when only one AAGUID has that name; zeros otherwise.
fn aaguid_of(provider: Option<&str>) -> &'static str {
    match provider {
        Some("Google Password Manager") => "ea9b8d664d011d213ce4b6b48cb575d4",
        Some("Chrome on Mac") => "adce000235bcc60a648b0b25f1f05503",
        Some("1Password") => "bada5566a7aa401fbd9645619a55120d",
        Some("Bitwarden") => "d548826e79b4db40a3d811116f7e8349",
        // iCloud Keychain and Windows Hello each have more than one.
        _ => NO_AAGUID,
    }
}

impl Db {
    /// The account's passkey log, oldest first, and when this hub began
    /// keeping logs. Empty for an account with none, or that isn't: the
    /// same answer, so it says nothing of which accounts exist.
    pub fn passkey_log(&self, account: &str) -> anyhow::Result<Log> {
        let c = self.c();
        let events = events(&c, account)?;
        let began = c
            .query_row("SELECT v FROM settings WHERE k = ?1", params![BEGAN], |r| r.get::<_, String>(0))
            .optional()?
            .and_then(|v| v.parse().ok());
        Ok(Log { account: account.to_owned(), events, began })
    }

    /// Whether the account's log has any event: it has had a passkey since
    /// this hub began keeping logs, or had one then (DK10: a first passkey
    /// is immediate only for an account that never had one). An account
    /// whose passkeys were all removed before the log began has an empty
    /// one: [`Log::began`] says when that was, to compare with when the
    /// account was made.
    pub fn ever_had_passkey(&self, account: &str) -> anyhow::Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT 1 FROM passkey_log WHERE account = ?1 LIMIT 1", params![account], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// A new passkey: its row, with its public key's bytes, and its `added`
    /// event, in one transaction. `pinned`: `e` was made and signed for its
    /// place (a step-up's), which must still be the next; else it goes
    /// next. The event as logged, or why not.
    pub fn add_passkey_logged(
        &self,
        public: &[u8],
        e: Event,
        pinned: bool,
    ) -> anyhow::Result<Result<Event, NotAppended>> {
        anyhow::ensure!(e.event == Change::Added, "a new passkey's event is an add");
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO passkeys (id, account, alg, public, sign_count, created) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![e.credential, e.account, e.alg, public, e.at as i64],
        )?;
        let logged = append(&tx, e, pinned)?;
        if logged.is_ok() {
            tx.commit()?;
        }
        Ok(logged)
    }

    /// The account's next event, as it would be placed now: what a step-up
    /// signs before the passkey is added ([`Db::add_passkey_logged`], pinned).
    pub fn passkey_next(&self, d: Draft) -> anyhow::Result<Event> {
        let c = self.c();
        let last = head(&c, &d.account)?;
        let at = last.as_ref().map_or(d.at, |l| d.at.max(l.at));
        Ok(Draft { at, ..d }.after(last.as_ref()))
    }

    /// The removal of `account`'s live passkey `credential` now, `by` this
    /// (`passkey` for DK10's signed removal), as the log's next event:
    /// what another passkey signs (its [`Event::challenge`]) before
    /// [`Db::drop_passkey_as`] takes it with the proof. `None` when that
    /// passkey isn't live.
    pub fn passkey_next_removal(&self, account: &str, credential: &str, by: &str) -> anyhow::Result<Option<Event>> {
        let c = self.c();
        let Some(added) = live(&c, account)?.into_iter().find(|e| e.credential == credential) else {
            return Ok(None);
        };
        let last = head(&c, account)?;
        let at = last.as_ref().map_or(0, |l| l.at).max(arugula_e2e::now_ms());
        Ok(Some(added.removal(at, by).placed_after(last.as_ref())))
    }

    /// The passkeys made before the log (DK3), written into it as `added`
    /// events at the time each was made, `by backfill <now>`: for each
    /// account with passkeys and no log yet, so it runs once per account
    /// and never again once its log has begun. `rp` is the RP to give them:
    /// the hub didn't keep which site a passkey was made at, so each of
    /// its sites' hosts, space-separated, when it has more than one. The
    /// AAGUID comes from the provider name it kept, where the name has only
    /// one ([`aaguid_of`]), else zeros. The first run notes when the log
    /// began. How many it wrote.
    pub fn backfill_passkey_log(&self, rp: &str, now: u64) -> anyhow::Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let by = arugula_e2e::passkey::By::Backfill(now).text();
        let drafts: Vec<Draft> = {
            let mut q = tx.prepare(
                "SELECT id, account, alg, public, created, provider FROM passkeys
                 WHERE account NOT IN (SELECT account FROM passkey_log)
                 ORDER BY account, created, id",
            )?;
            q.query_map([], |r| {
                Ok(Draft {
                    credential: r.get(0)?,
                    account: r.get(1)?,
                    alg: r.get(2)?,
                    key: r.get(3)?,
                    at: r.get::<_, i64>(4)?.max(0) as u64,
                    aaguid: aaguid_of(r.get::<_, Option<String>>(5)?.as_deref()).to_owned(),
                    event: Change::Added,
                    rp: rp.to_owned(),
                    by: by.clone(),
                })
            })?
            .collect::<Result<_, _>>()?
        };
        let rows = drafts.len();
        for d in drafts {
            append(&tx, d.after(None), false)?.map_err(|_| anyhow::anyhow!("an unpinned event can't move"))?;
        }
        tx.execute("INSERT OR IGNORE INTO settings (k, v) VALUES (?1, ?2)", params![BEGAN, now.to_string()])?;
        tx.commit()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use arugula_e2e::passkey::{EDDSA, check_chain};

    use super::*;

    fn draft(account: &str, id: &str, at: u64, by: &str) -> Draft {
        Draft {
            account: account.into(),
            event: Change::Added,
            credential: id.into(),
            rp: "control.test".into(),
            alg: EDDSA,
            key: vec![7; 32],
            aaguid: NO_AAGUID.into(),
            at,
            by: by.into(),
        }
    }

    #[test]
    fn the_log_takes_only_appends() {
        let db = Db::memory();
        db.add_passkey_logged(&[7; 32], draft("a1", "pk1", 10, "signup").after(None), false).unwrap().unwrap();
        let c = db.c();
        for sql in ["UPDATE passkey_log SET at = 1", "DELETE FROM passkey_log", "DELETE FROM passkey_log WHERE seq = 0"]
        {
            let e = c.execute(sql, []).unwrap_err().to_string();
            assert!(e.contains("append-only"), "{sql}: {e}");
        }
        // REPLACE deletes without the delete trigger: the insert trigger
        // takes only the next event, so it can't rewrite one.
        let replace = format!(
            "INSERT OR REPLACE INTO passkey_log ({COLUMNS}) SELECT account, seq, prev, kind, credential, rp, alg,
             key_format, public_key, aaguid, 1, by_what, proof, hash FROM passkey_log WHERE seq = 0"
        );
        for sql in [replace.as_str(), &replace.replace("INSERT OR REPLACE", "REPLACE")] {
            let e = c.execute(sql, []).unwrap_err().to_string();
            assert!(e.contains("only its next event"), "{sql}: {e}");
        }
        // Nor skip a place, or name another prev.
        let skip = format!(
            "INSERT INTO passkey_log ({COLUMNS}) SELECT account, 2, prev, kind, credential, rp, alg, key_format,
             public_key, aaguid, at, by_what, proof, hash FROM passkey_log WHERE seq = 0"
        );
        assert!(c.execute(&skip, []).unwrap_err().to_string().contains("only its next event"));
        let other_prev = format!(
            "INSERT INTO passkey_log ({COLUMNS}) SELECT account, 1, prev, kind, credential, rp, alg, key_format,
             public_key, aaguid, at, by_what, proof, hash FROM passkey_log WHERE seq = 0"
        );
        assert!(c.execute(&other_prev, []).unwrap_err().to_string().contains("only its next event"));
        let at: i64 = c.query_row("SELECT at FROM passkey_log WHERE seq = 0", [], |r| r.get(0)).unwrap();
        assert_eq!(at, 10, "unchanged");
    }

    #[test]
    fn a_pinned_event_waits_for_its_place() {
        let db = Db::memory();
        let first = db.passkey_next(draft("a1", "pk1", 10, "github")).unwrap();
        let also = db.passkey_next(draft("a1", "pk2", 11, "github")).unwrap();
        assert_eq!((first.seq, also.seq), (0, 0), "both made for seq 0");
        db.add_passkey_logged(&[7; 32], first, true).unwrap().unwrap();
        assert_eq!(db.add_passkey_logged(&[7; 32], also, true).unwrap(), Err(NotAppended::Moved), "another came first");
        assert_eq!(db.passkey_count("a1").unwrap(), 1, "and its row went with it");
        // Unpinned, it goes next, not back in time.
        let late =
            db.add_passkey_logged(&[7; 32], draft("a1", "pk3", 5, "github").after(None), false).unwrap().unwrap();
        assert_eq!((late.seq, late.at), (1, 10));
        assert_eq!(check_chain(&db.passkey_log("a1").unwrap()), Ok(()));
    }

    #[test]
    fn the_backfill_writes_each_account_once() {
        let db = Db::memory();
        {
            let c = db.c();
            for (id, account, created, provider) in [
                ("pk1", "a1", 30, Some("1Password")),
                ("pk2", "a1", 20, Some("iCloud Keychain")),
                ("pk3", "a2", 5, None),
            ] {
                c.execute(
                    "INSERT INTO passkeys (id, account, alg, public, sign_count, created, provider) VALUES (?1, ?2, -8, ?3, 0, ?4, ?5)",
                    params![id, account, vec![7u8; 32], created, provider],
                )
                .unwrap();
            }
        }
        assert_eq!(db.backfill_passkey_log("control.test old.test", 1000).unwrap(), 3);
        let log = db.passkey_log("a1").unwrap();
        assert_eq!(log.began, Some(1000));
        let got: Vec<_> = log.events.iter().map(|e| (e.credential.as_str(), e.at, e.aaguid.as_str())).collect();
        assert_eq!(got, [("pk2", 20, NO_AAGUID), ("pk1", 30, "bada5566a7aa401fbd9645619a55120d")], "oldest first");
        assert!(log.events.iter().all(|e| e.by == "backfill 1000" && e.rp == "control.test old.test"));
        assert_eq!(check_chain(&log), Ok(()));
        // Again: nothing new, and the log's start stays the first one's.
        assert_eq!(db.backfill_passkey_log("control.test", 2000).unwrap(), 0);
        assert_eq!(db.passkey_log("a1").unwrap().began, Some(1000));
        assert!(db.ever_had_passkey("a2").unwrap() && !db.ever_had_passkey("a3").unwrap());
    }

    /// DK10's seam: a removal another passkey signs goes in as made, with
    /// its proof, and the log still holds together.
    #[test]
    fn a_signed_removal_goes_in_with_its_proof() {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
        use ed25519_dalek::Signer as _;
        use sha2::{Digest as _, Sha256};
        let db = Db::memory();
        let (k1, k2) =
            (ed25519_dalek::SigningKey::from_bytes(&[1; 32]), ed25519_dalek::SigningKey::from_bytes(&[2; 32]));
        for (id, k) in [("pk1", &k1), ("pk2", &k2)] {
            let d = Draft { key: k.verifying_key().to_bytes().to_vec(), ..draft("a1", id, 10, "signup") };
            db.add_passkey_logged(&d.key.clone(), d.after(None), false).unwrap().unwrap();
        }
        assert!(db.passkey_next_removal("a1", "pk3", "passkey").unwrap().is_none(), "not live");
        let e = db.passkey_next_removal("a1", "pk1", "passkey").unwrap().unwrap();
        let cd = serde_json::to_vec(&serde_json::json!({
            "type": "webauthn.get", "challenge": e.challenge(), "origin": "https://control.test",
        }))
        .unwrap();
        let mut data = Sha256::digest(b"control.test").to_vec();
        data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
        let mut msg = data.clone();
        msg.extend_from_slice(&Sha256::digest(&cd));
        let proof = Assertion {
            id: "pk2".into(),
            client_data_json: B64.encode(&cd),
            authenticator_data: B64.encode(&data),
            signature: B64.encode(k2.sign(&msg).to_bytes()),
        };
        // A proof that isn't over this event, or isn't a live passkey's,
        // never goes in: it would break the log for good.
        let mut forged = proof.clone();
        forged.signature = B64.encode([0u8; 64]);
        assert!(db.drop_passkey_as("a1", "pk1", Some(e.clone().with_proof(forged))).unwrap().is_err());
        let unknown = Assertion { id: "pk9".into(), ..proof.clone() };
        assert!(db.drop_passkey_as("a1", "pk1", Some(e.clone().with_proof(unknown))).unwrap().is_err());
        assert_eq!(db.passkey_count("a1").unwrap(), 2, "nothing removed");
        assert_eq!(check_chain(&db.passkey_log("a1").unwrap()), Ok(()));
        db.drop_passkey_as("a1", "pk1", Some(e.with_proof(proof))).unwrap().unwrap();
        let log = db.passkey_log("a1").unwrap();
        assert_eq!(log.events.last().map(|e| (e.event, e.by.as_str())), Some((Change::Removed, "passkey")));
        assert_eq!(check_chain(&log), Ok(()));
        assert_eq!(db.passkey_count("a1").unwrap(), 1);
    }
}
