//! What the hub asks of billing (M22), which is the platform's since M139
//! (#1156): whether an account pays, for hosted sandboxes, and the relay's
//! standing against the free allowance. Plans are the platform's
//! (`PlatformPort::active_plans`); the relay's bytes are the hub's own
//! (`usage`, which the relay writes).

use arugula_e2e::now_ms;
use arugula_platform::PlatformPort;

use crate::App;

/// Free relay traffic per account per month.
pub fn relay_allowance(app: &App) -> u64 {
    app.cfg.relay_free_bytes
}

/// The month an instant falls in, `YYYY-MM`.
fn month(ms: u64) -> String {
    crate::day(ms)[..7].to_owned()
}

/// Who pays for something an account does: its team's plan if it's in an
/// active one, else its own. Any active plan of the account's or one of
/// its teams' will do.
pub async fn paid(app: &App, account: &str) -> anyhow::Result<bool> {
    let mut owners = vec![format!("account:{account}")];
    for body in app.db.teams_of(account)? {
        owners.push(format!("team:{}", crate::teams::parse(&body)?.team));
    }
    Ok(!app.platform.active_plans(&owners).await?.is_empty())
}

/// Relay standing for a free account: (bytes this month, over the
/// allowance, past twice it).
pub async fn relay_standing(app: &App, account: &str) -> anyhow::Result<(u64, bool, bool)> {
    let used = app.db.relay_bytes_month(account, &month(now_ms()))?;
    if !app.cfg.billing || paid(app, account).await? {
        return Ok((used, false, false));
    }
    let cap = relay_allowance(app);
    Ok((used, used > cap, used > cap * 2))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arugula_e2e::{
        DeviceKeys,
        team::{Member, Roster, TeamRole},
    };
    use axum::Router;
    use serde_json::{Value, json};
    use sha2::Sha256;

    use super::*;
    use crate::routing_wire::{control, person, session};

    #[test]
    fn months() {
        assert_eq!(month(1_790_000_000_000), "2026-09"); // 2026-09-21
    }

    /// A fake Stripe, as `web/fixtures/fakes.ts` has it: customers,
    /// checkout, a subscription with a seat item. It keeps what it's asked.
    async fn fake_stripe() -> (String, Arc<std::sync::Mutex<Vec<(String, String)>>>) {
        let calls: Arc<std::sync::Mutex<Vec<(String, String)>>> = Default::default();
        let keep = calls.clone();
        let svc = Router::new().fallback(move |uri: axum::http::Uri, body: String| {
            let keep = keep.clone();
            async move {
                let path = uri.path().to_owned();
                keep.lock().unwrap().push((path.clone(), body));
                axum::Json(match path.as_str() {
                    "/v1/customers" => json!({ "id": "cus_1" }),
                    "/v1/checkout/sessions" => json!({ "id": "cs_1", "url": "https://checkout.stripe.test/cs_1" }),
                    "/v1/subscriptions/sub_1" => json!({ "id": "sub_1", "items": { "data": [
                        { "id": "si_seat", "price": { "id": "price_seat" } },
                        { "id": "si_min", "price": { "id": "price_min" } },
                    ] } }),
                    _ => json!({}),
                })
            }
        });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, svc).await });
        (at, calls)
    }

    fn signed(body: &str) -> String {
        use hmac::{KeyInit, Mac};
        let t = now_ms() / 1000;
        let mut mac = hmac::Hmac::<Sha256>::new_from_slice(b"whsec_test").unwrap();
        mac.update(format!("{t}.{body}").as_bytes());
        format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn roster(team: &str, version: u64, members: &[(&str, &DeviceKeys, TeamRole)], by: &DeviceKeys) -> Value {
        let mut r = Roster {
            v: 1,
            team: team.into(),
            name: "Acme".into(),
            version,
            at: version,
            members: members
                .iter()
                .map(|(a, k, role)| Member { account: (*a).into(), root: k.id(), role: *role, name: (*a).into() })
                .collect(),
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(by);
        serde_json::to_value(&r).unwrap()
    }

    /// Billing through the in-process port, as `control-smoke` drives it
    /// against the real binary: the platform answers billing's routes
    /// behind the hub's gate, Stripe's webhook records a team's plan in the
    /// platform's tables, the hub's `paid` sees it through
    /// `PlatformPort::active_plans`, and a roster the hub takes moves the
    /// seats through `PlatformPort::roster_changed`.
    #[tokio::test]
    async fn billing_through_the_port() {
        let (stripe, calls) = fake_stripe().await;
        let c = control(|app| {
            app.cfg.billing = true;
            let p = Arc::get_mut(&mut app.platform).expect("not started yet");
            p.stripe = Some(arugula_platform::billing::Stripe {
                api: stripe.clone(),
                secret: "sk_test".into(),
                webhook_secret: "whsec_test".into(),
                seat_price: "price_seat".into(),
                minutes_price: "price_min".into(),
                minutes_event: "sandbox_minutes".into(),
            });
        })
        .await;
        let me = person(&c.app, "owner", "acct-own");
        let them = person(&c.app, "mate", "acct-mate");
        let cookie = session(&c.app, "acct-own");
        let team = "0123456789abcdef";
        let v1 = roster(team, 1, &[("acct-own", &me, TeamRole::Owner)], &me);
        let (s, _) = c.as_person(&cookie, "POST", "/api/teams", Some(json!({ "roster": v1 }))).await;
        assert_eq!(s, 200);

        // Free: over the allowance (none here), warned and slowed.
        c.app.db.add_relay_bytes("acct-own", &crate::day(now_ms()), 10).unwrap();
        let (s, b) = c.as_person(&cookie, "GET", "/api/billing", None).await;
        assert_eq!(s, 200, "{b}");
        assert_eq!((b["billing"].as_bool(), b["plan"].as_str()), (Some(true), Some("personal")));
        assert_eq!(b["relay"], json!({ "bytes": 10, "allowance": 0, "warning": true, "slowed": true }));
        assert_eq!(b["teams"][0]["plan"], "free");
        assert!(!paid(&c.app, "acct-own").await.unwrap());
        assert!(relay_standing(&c.app, "acct-own").await.unwrap().2);
        let r = c.http.get(format!("{}/api/billing", c.base)).send().await.unwrap();
        assert_eq!(r.status(), 401, "signed in only");

        // The team's checkout: one seat, and metered minutes.
        let (s, b) = c.as_person(&cookie, "POST", "/api/billing/checkout", Some(json!({ "team": team }))).await;
        assert_eq!((s, b["url"].as_str()), (200, Some("https://checkout.stripe.test/cs_1")));
        let form = calls.lock().unwrap().iter().find(|(p, _)| p == "/v1/checkout/sessions").unwrap().1.clone();
        assert!(form.contains("line_items%5B0%5D%5Bquantity%5D=1") && form.contains("price_min"), "{form}");
        let theirs = session(&c.app, "acct-mate");
        let (s, _) = c.as_person(&theirs, "POST", "/api/billing/checkout", Some(json!({ "team": team }))).await;
        assert_eq!(s, 403, "a team's owners pay for it");

        // Stripe says it's done.
        let hook = |body: Value| {
            let body = body.to_string();
            c.http
                .post(format!("{}/api/stripe/webhook", c.base))
                .header("stripe-signature", signed(&body))
                .body(body)
                .send()
        };
        let r = c
            .http
            .post(format!("{}/api/stripe/webhook", c.base))
            .header("stripe-signature", "t=1,v1=00")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "an unsigned webhook is refused");
        let done = json!({ "type": "checkout.session.completed", "data": { "object": {
            "customer": "cus_1", "subscription": "sub_1", "metadata": { "owner": format!("team:{team}") } } } });
        assert_eq!(hook(done).await.unwrap().status(), 200);
        assert!(c.app.platform.db.billing(&format!("team:{team}")).unwrap().unwrap().active());
        assert!(paid(&c.app, "acct-own").await.unwrap(), "the hub sees the team's plan");
        assert!(!relay_standing(&c.app, "acct-own").await.unwrap().2, "a paying account isn't slowed");
        let (_, b) = c.as_person(&cookie, "GET", "/api/billing", None).await;
        assert_eq!((b["teams"][0]["plan"].as_str(), b["relay"]["warning"].as_bool()), (Some("team"), Some(false)));

        // A second member, who asked to join: the seats follow the roster.
        let ask = crate::db::TeamRequest {
            account: "acct-mate".into(),
            root: them.id(),
            name: "mate".into(),
            role: "editor".into(),
            created: 1,
            invite: None,
            proof: None,
        };
        c.app.db.add_request(team, &ask).unwrap();
        let v2 = roster(team, 2, &[("acct-own", &me, TeamRole::Owner), ("acct-mate", &them, TeamRole::Editor)], &me);
        let path = format!("/api/teams/{team}/roster");
        let (s, b) = c.as_person(&cookie, "POST", &path, Some(json!({ "roster": v2 }))).await;
        assert_eq!(s, 200, "{b}");
        let seats = calls.lock().unwrap().iter().rev().find(|(p, _)| p == "/v1/subscription_items/si_seat").cloned();
        assert!(seats.is_some_and(|(_, f)| f.contains("quantity=2")), "adding a member adds a seat");

        // Without a Stripe key, billing's routes say so.
        let off = control(|_| {}).await;
        let r = off.http.post(format!("{}/api/stripe/webhook", off.base)).body("{}").send().await.unwrap();
        assert_eq!(r.status(), 404);
        person(&off.app, "solo", "acct-solo");
        let cookie = session(&off.app, "acct-solo");
        let (s, b) = off.as_person(&cookie, "GET", "/api/billing", None).await;
        assert_eq!((s, b["billing"].as_bool()), (200, Some(false)));
    }
}
