//! Billing's tables (M22, the platform's since M139): who pays, and the
//! sandbox minutes already reported to Stripe. The meters themselves
//! (relayed bytes in `usage`, sandbox minutes from `sandboxes`) stay the
//! hub's, which writes them, and come through `HubPort::meters`. The
//! tables are made by the platform's migrations (`migrations/sqlite`).

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};
use crate::Deleted;

pub struct BillingRow {
    pub customer: Option<String>,
    pub subscription: Option<String>,
    pub status: String,
    pub seat_item: Option<String>,
}

impl BillingRow {
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "active" | "trialing" | "past_due")
    }
}

impl Db {
    pub fn billing(&self, owner: &str) -> anyhow::Result<Option<BillingRow>> {
        self.c()
            .query_row(
                "SELECT customer, subscription, status, seat_item FROM billing WHERE owner = ?1",
                params![owner],
                |r| {
                    Ok(BillingRow {
                        customer: r.get(0)?,
                        subscription: r.get(1)?,
                        status: r.get(2)?,
                        seat_item: r.get(3)?,
                    })
                },
            )
            .optional()
    }

    pub fn set_billing(
        &self,
        owner: &str,
        customer: Option<&str>,
        subscription: Option<&str>,
        status: &str,
        now: u64,
    ) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO billing (owner, customer, subscription, status, updated) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (owner) DO UPDATE SET customer = COALESCE(excluded.customer, billing.customer),
               subscription = COALESCE(excluded.subscription, billing.subscription), status = excluded.status, updated = excluded.updated",
            params![owner, customer, subscription, status, now],
        )?;
        Ok(())
    }

    pub fn set_seat_item(&self, owner: &str, item: &str) -> anyhow::Result<()> {
        self.c().execute("UPDATE billing SET seat_item = ?2 WHERE owner = ?1", params![owner, item])?;
        Ok(())
    }

    /// Of these owners (`account:ID`, `team:ID`), the ones whose plan is
    /// active, in the order asked.
    pub fn active_plans(&self, owners: &[String]) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::new();
        for o in owners {
            if self.billing(o)?.is_some_and(|b| b.active()) && !out.contains(o) {
                out.push(o.clone());
            }
        }
        Ok(out)
    }

    pub fn reported_minutes(&self, account: &str) -> anyhow::Result<u64> {
        Ok(self
            .c()
            .query_row("SELECT minutes FROM reported WHERE account = ?1", params![account], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    pub fn set_reported_minutes(&self, account: &str, minutes: u64) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO reported (account, minutes) VALUES (?1, ?2)
             ON CONFLICT (account) DO UPDATE SET minutes = excluded.minutes",
            params![account, minutes],
        )?;
        Ok(())
    }
}

/// An account is gone: its plan, the plans of the teams it disbanded, and
/// what was reported of its minutes. A plan still active never gets here:
/// the account cancels it first (`active_plans`).
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> anyhow::Result<()> {
    for t in &d.disbanded {
        c.execute("DELETE FROM billing WHERE owner = 'team:' || ?1", params![t])?;
    }
    c.execute("DELETE FROM billing WHERE owner = 'account:' || ?1", params![d.account])?;
    c.execute("DELETE FROM reported WHERE account = ?1", params![d.account])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn plans_are_active_while_stripe_says_so() {
        let db = Db::test();
        db.set_billing("account:a", Some("cus_a"), None, "none", 1).unwrap();
        assert!(db.active_plans(&ids(&["account:a"])).unwrap().is_empty(), "a customer isn't a plan");
        db.set_billing("account:a", None, Some("sub_a"), "active", 2).unwrap();
        db.set_billing("team:t", Some("cus_t"), Some("sub_t"), "past_due", 2).unwrap();
        db.set_billing("team:u", Some("cus_u"), Some("sub_u"), "canceled", 2).unwrap();
        let got = db.active_plans(&ids(&["team:u", "team:t", "account:a", "team:none"])).unwrap();
        assert_eq!(got, ids(&["team:t", "account:a"]));
        let b = db.billing("account:a").unwrap().unwrap();
        assert_eq!((b.customer.as_deref(), b.subscription.as_deref()), (Some("cus_a"), Some("sub_a")), "kept");
        db.set_seat_item("team:t", "si_1").unwrap();
        assert_eq!(db.billing("team:t").unwrap().unwrap().seat_item.as_deref(), Some("si_1"));
        assert_eq!(db.reported_minutes("a").unwrap(), 0);
        db.set_reported_minutes("a", 7).unwrap();
        assert_eq!(db.reported_minutes("a").unwrap(), 7);
    }

    /// An account that's gone takes its billing with it, and the plans of
    /// the teams it disbanded; twice changes nothing.
    #[test]
    fn an_account_takes_its_billing() {
        let db = Db::test();
        db.set_billing("account:a", Some("cus_a"), Some("sub_a"), "canceled", 1).unwrap();
        db.set_billing("team:t", Some("cus_t"), Some("sub_t"), "canceled", 1).unwrap();
        db.set_billing("team:kept", Some("cus_k"), Some("sub_k"), "active", 1).unwrap();
        db.set_billing("account:b", Some("cus_b"), Some("sub_b"), "active", 1).unwrap();
        db.set_reported_minutes("a", 3).unwrap();
        db.set_reported_minutes("b", 4).unwrap();
        let gone = Deleted { account: "a".into(), machines: vec![], disbanded: ids(&["t"]) };
        erase_account(&db.c(), &gone).unwrap();
        erase_account(&db.c(), &gone).unwrap();
        assert!(db.billing("account:a").unwrap().is_none());
        assert!(db.billing("team:t").unwrap().is_none());
        assert_eq!(db.reported_minutes("a").unwrap(), 0);
        // Nobody else's.
        assert!(db.billing("team:kept").unwrap().is_some());
        assert!(db.billing("account:b").unwrap().is_some());
        assert_eq!(db.reported_minutes("b").unwrap(), 4);
    }
}
