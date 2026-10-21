//! Billing (M22), only when control has a Stripe key: self-hosted control
//! has none and never bills.
//!
//! - **Personal** is free: one person, any number of daemons, relay
//!   traffic up to a monthly allowance.
//! - **Team** is per seat (each member in the signed roster).
//! - **Hosted sandboxes** are by the minute, on a paid plan.
//!
//! Upgrading goes through Stripe Checkout; Stripe tells control what
//! happened through signed webhooks. Seat counts follow the roster;
//! sandbox minutes go to Stripe as meter events. Over the relay allowance
//! a free account sees a warning, and past twice that its relayed traffic
//! slows down. With billing on, hosted sandboxes need a paid plan, and
//! running ones are never stopped for it.
//!
//! The platform's since M139 (#1156): plans and what's been reported are
//! its own tables (`db::billing`); teams, and the relay and sandbox meters,
//! come from the hub through [`HubPort`](crate::HubPort). The hub asks
//! whether someone pays through `PlatformPort::active_plans`, and tells it
//! a roster changed through `PlatformPort::roster_changed`.

use std::{collections::HashMap, sync::Arc};

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use tracing::{info, warn};

use crate::{Platform, auth::Person};

type R = Result<Json<Value>, ApiError>;

/// Checkout pages asked for, per account.
const CHECKOUTS: (&str, usize) = ("checkout", 20);
/// Usage reports asked for, per account and all together.
const REPORTS: (&str, usize) = ("report", 6);
const ALL_REPORTS: (&str, usize) = ("report-all", 30);

/// Billing's flags and their env vars. They moved here from control's
/// with the same names (M140), so a deploy's secrets don't change: control
/// takes them (`#[command(flatten)]`; the hub only needs to know billing is
/// on), and so does the platform's own binary.
#[derive(clap::Args, Debug, Clone)]
#[group(id = "billing")]
pub struct Args {
    /// Billing (M22): a Stripe secret key turns it on (STRIPE_SECRET_KEY);
    /// webhooks are checked with STRIPE_WEBHOOK_SECRET.
    #[arg(long, env = "STRIPE_SECRET_KEY", hide_env_values = true)]
    pub stripe_secret: Option<String>,
    #[arg(long, env = "STRIPE_WEBHOOK_SECRET", hide_env_values = true)]
    pub stripe_webhook_secret: Option<String>,
    #[arg(long, default_value = "https://api.stripe.com", env = "ARUGULA_STRIPE_API")]
    pub stripe_api: String,
    /// The per-seat price, and the metered price and meter event for
    /// sandbox minutes.
    #[arg(long, env = "ARUGULA_STRIPE_SEAT_PRICE", default_value = "")]
    pub stripe_seat_price: String,
    #[arg(long, env = "ARUGULA_STRIPE_MINUTES_PRICE", default_value = "")]
    pub stripe_minutes_price: String,
    #[arg(long, env = "ARUGULA_STRIPE_MINUTES_EVENT", default_value = "sandbox_minutes")]
    pub stripe_minutes_event: String,
}

impl Args {
    /// Billing, if there's a Stripe key: never without the webhook secret,
    /// which is all that tells Stripe's events from anyone's.
    pub fn stripe(self) -> anyhow::Result<Option<Stripe>> {
        let set = |v: Option<String>| v.filter(|s| !s.is_empty());
        let Some(secret) = set(self.stripe_secret) else { return Ok(None) };
        let Some(webhook_secret) = set(self.stripe_webhook_secret) else {
            anyhow::bail!("STRIPE_SECRET_KEY is set but STRIPE_WEBHOOK_SECRET isn't: billing needs both");
        };
        Ok(Some(Stripe {
            api: self.stripe_api.trim_end_matches('/').to_owned(),
            secret,
            webhook_secret,
            seat_price: self.stripe_seat_price,
            minutes_price: self.stripe_minutes_price,
            minutes_event: self.stripe_minutes_event,
        }))
    }
}

/// The hourly usage report, while billing is on, except while background
/// work is held ([`Platform::paused`]).
pub fn start(p: &Arc<Platform>) {
    if p.stripe.is_none() {
        return;
    }
    let p = p.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            if !p.is_paused() {
                report_usage(&p).await;
            }
        }
    });
}

pub struct Stripe {
    pub api: String,
    pub secret: String,
    pub webhook_secret: String,
    /// The per-seat price (a team's subscription).
    pub seat_price: String,
    /// The metered price for sandbox minutes.
    pub minutes_price: String,
    /// The meter's event name for sandbox minutes.
    pub minutes_event: String,
}

/// Free relay traffic per account per month.
pub fn relay_allowance(p: &Platform) -> u64 {
    p.cfg.relay_free_bytes
}

impl Stripe {
    async fn post(&self, http: &reqwest::Client, path: &str, form: &[(String, String)]) -> anyhow::Result<Value> {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let r = http
            .post(format!("{}{path}", self.api))
            .bearer_auth(&self.secret)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Stripe {path}: {status} {}", v["error"]["message"].as_str().unwrap_or(""));
        }
        Ok(v)
    }

    async fn get(&self, http: &reqwest::Client, path: &str) -> anyhow::Result<Value> {
        let r = http.get(format!("{}{path}", self.api)).bearer_auth(&self.secret).send().await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Stripe {path}: {status}");
        }
        Ok(v)
    }
}

fn stripe(p: &Platform) -> Result<&Stripe, ApiError> {
    p.stripe.as_ref().ok_or_else(|| err(StatusCode::NOT_FOUND, "billing isn't set up on this control"))
}

/// Who pays for something an account does: its team's plan if it's in an
/// active one, else its own.
pub async fn paid(p: &Platform, account: &str) -> anyhow::Result<bool> {
    Ok(payer(p, account).await?.is_some())
}

/// Relay standing for a free account, from the bytes it relayed this
/// month: (those bytes, over the allowance, past twice it).
async fn relay_standing(p: &Platform, account: &str, used: u64) -> anyhow::Result<(u64, bool, bool)> {
    if p.stripe.is_none() || paid(p, account).await? {
        return Ok((used, false, false));
    }
    let cap = relay_allowance(p);
    Ok((used, used > cap, used > cap * 2))
}

/// `GET /api/billing`: my plan and my teams', and this month's usage.
pub async fn status(State(p): State<Arc<Platform>>, s: Person) -> R {
    let on = p.stripe.is_some();
    let now = now_ms();
    let since = month_start(now);
    // Teams, rosters and meters as the cache keeps them (`cache::DELAY`
    // for a roster, a cache tick for usage): no hub call once it's warm.
    let ids = p.cache.teams_of(&*p.hub, &s.account).await?;
    let mut found = p.cache.teams(&*p.hub, &ids).await?;
    let teams: Vec<_> = ids.iter().filter_map(|id| found.remove(id)).collect();
    // Every meter at once: mine, and my teams' members'.
    let mut accounts = vec![s.account.clone()];
    accounts.extend(teams.iter().flat_map(|t| t.members.iter().map(|m| m.account.clone())));
    accounts.sort();
    accounts.dedup();
    let meters: HashMap<String, (u64, u64)> = p
        .cache
        .meters(&*p.hub, &accounts, since, now)
        .await?
        .into_iter()
        .map(|(a, m)| (a, (m.relay_bytes, m.sandbox_minutes)))
        .collect();
    let meter = |a: &str| meters.get(a).copied().unwrap_or_default();
    let (relay, warn_relay, slow) = relay_standing(&p, &s.account, meter(&s.account).0).await?;
    let mine = p.db.billing(&format!("account:{}", s.account))?;
    let mut out = Vec::new();
    for t in teams {
        let b = p.db.billing(&format!("team:{}", t.id))?;
        let owner = t.role(&s.account) == Some(TeamRole::Owner);
        let minutes: u64 = t.members.iter().map(|m| meter(&m.account).1).sum();
        out.push(json!({
            "team": t.id, "name": t.name, "owner": owner, "seats": t.members.len(),
            "plan": if b.as_ref().is_some_and(|b| b.active()) { "team" } else { "free" },
            "status": b.map(|b| b.status), "sandbox_minutes": minutes,
        }));
    }
    Ok(Json(json!({
        "billing": on,
        "plan": if mine.as_ref().is_some_and(|b| b.active()) { "paid" } else { "personal" },
        "relay": { "bytes": relay, "allowance": relay_allowance(&p), "warning": warn_relay, "slowed": slow },
        "sandbox_minutes": meter(&s.account).1,
        "teams": out,
    })))
}

fn month_start(ms: u64) -> u64 {
    let days = ms / 86_400_000;
    // Walk back to the 1st of this month.
    let mut d = days;
    while arugula_control_common::day(d * 86_400_000)[8..] != *"01" {
        d -= 1;
    }
    d * 86_400_000
}

#[derive(Deserialize)]
pub struct Checkout {
    /// A team (its owners pay per seat); none for a personal plan.
    #[serde(default)]
    team: Option<String>,
}

/// `POST /api/billing/checkout`: a Stripe Checkout page to upgrade.
pub async fn checkout(State(p): State<Arc<Platform>>, headers: HeaderMap, s: Person, Json(b): Json<Checkout>) -> R {
    let st = stripe(&p)?;
    p.limits.check_account(CHECKOUTS, &s.account)?;
    let (owner, seats) = match &b.team {
        Some(t) => {
            // The cache's team, asked again before refusing.
            let owns = |team: &Option<crate::hub::Team>| {
                team.as_ref().is_some_and(|team| team.role(&s.account) == Some(TeamRole::Owner))
            };
            let mut team = p.cache.team(&*p.hub, t).await?;
            if !owns(&team) {
                team = p.cache.team_now(&*p.hub, t).await?;
            }
            let team = team.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such team"))?;
            if team.role(&s.account) != Some(TeamRole::Owner) {
                return Err(err(StatusCode::FORBIDDEN, "a team's owners pay for it"));
            }
            (format!("team:{t}"), team.members.len())
        }
        None => (format!("account:{}", s.account), 0),
    };
    let customer = match p.db.billing(&owner)?.and_then(|b| b.customer) {
        Some(c) => c,
        None => {
            let c = st.post(&p.http, "/v1/customers", &[("metadata[owner]".into(), owner.clone())]).await?;
            let id = c["id"].as_str().unwrap_or_default().to_owned();
            p.db.set_billing(&owner, Some(&id), None, "none", now_ms())?;
            id
        }
    };
    let mut form: Vec<(String, String)> = vec![
        ("mode".into(), "subscription".into()),
        ("customer".into(), customer),
        // Back to the site it was started on (#507).
        ("success_url".into(), format!("{}/#billing=done", p.cfg.site(&headers))),
        ("cancel_url".into(), format!("{}/#billing=cancelled", p.cfg.site(&headers))),
        ("metadata[owner]".into(), owner.clone()),
        ("subscription_data[metadata][owner]".into(), owner.clone()),
    ];
    let mut i = 0;
    if seats > 0 {
        form.push((format!("line_items[{i}][price]"), st.seat_price.clone()));
        form.push((format!("line_items[{i}][quantity]"), seats.to_string()));
        i += 1;
    }
    form.push((format!("line_items[{i}][price]"), st.minutes_price.clone()));
    let session = st.post(&p.http, "/v1/checkout/sessions", &form).await?;
    let props = json!({ "plan": if b.team.is_some() { "team" } else { "personal" }, "seats": seats });
    match &b.team {
        Some(t) => p.analytics.capture_team(&s.account, t, "checkout started", props),
        None => p.analytics.capture(&s.account, "checkout started", props),
    }
    Ok(Json(json!({ "url": session["url"] })))
}

type HmacSha256 = hmac::Hmac<Sha256>;

/// Stripe's webhook signature: `t=<ts>,v1=<hex HMAC-SHA256 of "t.body">`,
/// within five minutes.
fn verify_signature(secret: &str, header: &str, body: &[u8], now_s: u64) -> bool {
    use hmac::{KeyInit, Mac};
    let mut t = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.split_once('=') {
            Some(("t", v)) => t = v.parse::<u64>().ok(),
            Some(("v1", v)) => sigs.push(v.to_owned()),
            _ => {}
        }
    }
    let Some(t) = t else { return false };
    if secret.is_empty() || now_s.abs_diff(t) > 300 {
        return false;
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
    mac.update(format!("{t}.").as_bytes());
    mac.update(body);
    let want = hex::encode(mac.finalize().into_bytes());
    sigs.iter().any(|s| s.len() == want.len() && s.bytes().zip(want.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0)
}

/// `POST /api/stripe/webhook`: what happened at Stripe.
pub async fn webhook(State(p): State<Arc<Platform>>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, ApiError> {
    let st = stripe(&p)?;
    let sig = headers.get("stripe-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !verify_signature(&st.webhook_secret, sig, &body, now_ms() / 1000) {
        return Err(err(StatusCode::BAD_REQUEST, "bad signature"));
    }
    let ev: Value = serde_json::from_slice(&body)?;
    let obj = &ev["data"]["object"];
    let owner = obj["metadata"]["owner"].as_str().unwrap_or_default().to_owned();
    match ev["type"].as_str().unwrap_or("") {
        "checkout.session.completed" if !owner.is_empty() => {
            let sub = obj["subscription"].as_str().unwrap_or_default();
            p.db.set_billing(&owner, obj["customer"].as_str(), Some(sub), "active", now_ms())?;
            info!(owner, "upgraded");
            note(&p, &owner, "upgraded", json!({})).await;
            // The seat item, for keeping its quantity with the roster.
            if let Ok(s) = st.get(&p.http, &format!("/v1/subscriptions/{sub}")).await {
                for item in s["items"]["data"].as_array().into_iter().flatten() {
                    if item["price"]["id"] == st.seat_price.as_str() {
                        p.db.set_seat_item(&owner, item["id"].as_str().unwrap_or_default())?;
                    }
                }
            }
        }
        "customer.subscription.updated" | "customer.subscription.deleted" if !owner.is_empty() => {
            let status = obj["status"].as_str().unwrap_or("canceled");
            p.db.set_billing(&owner, obj["customer"].as_str(), obj["id"].as_str(), status, now_ms())?;
            info!(owner, status, "subscription changed");
            note(&p, &owner, "subscription changed", json!({ "status": status })).await;
        }
        _ => {}
    }
    Ok(StatusCode::OK)
}

/// A billing event, by who pays (`account:<id>` or `team:<id>`): a team's
/// goes on its founder, grouped under the team.
async fn note(p: &Platform, owner: &str, event: &str, mut props: Value) {
    props["plan"] = if owner.starts_with("team:") { "team" } else { "personal" }.into();
    match owner.split_once(':') {
        Some(("account", a)) => p.analytics.capture(a, event, props),
        Some(("team", t)) => {
            let founder = p.cache.team(&*p.hub, t).await.ok().flatten().map(|t| t.founder);
            let who = founder.unwrap_or_else(|| owner.to_owned());
            p.analytics.capture_team(&who, t, event, props)
        }
        _ => {}
    }
}

/// A team's roster changed: its seats follow (M19 → M22), one per member
/// with an account.
pub async fn sync_seats(p: &Platform, team: &str) {
    let Some(st) = &p.stripe else { return };
    let owner = format!("team:{team}");
    let Ok(Some(b)) = p.db.billing(&owner) else { return };
    let (true, Some(item)) = (b.active(), b.seat_item) else { return };
    let seats = match p.hub.teams(&[team.to_owned()]).await {
        Ok(mut t) => match t.pop() {
            Some(t) => t.members.len(),
            None => return,
        },
        Err(e) => {
            warn!(team, error = %e, "can't count seats");
            return;
        }
    };
    let form =
        [("quantity".to_owned(), seats.to_string()), ("proration_behavior".to_owned(), "create_prorations".to_owned())];
    match st.post(&p.http, &format!("/v1/subscription_items/{item}"), &form).await {
        Ok(_) => info!(team, seats, "seats updated"),
        Err(e) => warn!(team, error = %e, "can't update seats"),
    }
}

/// Report sandbox minutes not yet reported, per paying owner: hourly.
pub async fn report_usage(p: &Platform) {
    let Some(st) = &p.stripe else { return };
    let now = now_ms();
    let accounts = p.hub.metered_accounts().await.unwrap_or_default();
    let meters = p.hub.meters(&accounts, 0, now).await.unwrap_or_default();
    let mut per_customer: HashMap<String, u64> = HashMap::new();
    for m in meters {
        let Ok(Some(customer_owner)) = payer(p, &m.account).await else { continue };
        let Ok(Some(b)) = p.db.billing(&customer_owner) else { continue };
        let active = b.active();
        let Some(customer) = b.customer.filter(|_| active) else { continue };
        let total = m.sandbox_minutes;
        let reported = p.db.reported_minutes(&m.account).unwrap_or(0);
        if total > reported {
            *per_customer.entry(customer).or_default() += total - reported;
            let _ = p.db.set_reported_minutes(&m.account, total);
        }
    }
    for (customer, minutes) in per_customer {
        let form = [
            ("event_name".to_owned(), st.minutes_event.clone()),
            ("payload[stripe_customer_id]".to_owned(), customer.clone()),
            ("payload[value]".to_owned(), minutes.to_string()),
            ("timestamp".to_owned(), (now / 1000).to_string()),
        ];
        match st.post(&p.http, "/v1/billing/meter_events", &form).await {
            Ok(_) => info!(customer, minutes, "sandbox minutes reported"),
            Err(e) => warn!(error = %e, "can't report usage"),
        }
    }
}

/// Who pays for an account's sandboxes: its own plan, else its team's.
async fn payer(p: &Platform, account: &str) -> anyhow::Result<Option<String>> {
    let mut owners = vec![format!("account:{account}")];
    owners.extend(p.cache.teams_of(&*p.hub, account).await?.into_iter().map(|t| format!("team:{t}")));
    let active = p.db.active_plans(&owners)?;
    Ok(owners.into_iter().find(|o| active.contains(o)))
}

/// `POST /api/billing/report` (owners of billing on this control): report
/// usage now, rather than at the next hour.
pub async fn report_now(State(p): State<Arc<Platform>>, s: Person) -> R {
    stripe(&p)?;
    p.limits.check_account(REPORTS, &s.account)?;
    p.limits.check_all(ALL_REPORTS)?;
    report_usage(&p).await;
    Ok(Json(json!({})))
}

#[cfg(test)]
mod tests {
    use hmac::{KeyInit, Mac};

    use super::*;

    #[test]
    fn billing_needs_its_webhook_secret() {
        let b = |s: Option<&str>, w: Option<&str>| {
            Args {
                stripe_secret: s.map(Into::into),
                stripe_webhook_secret: w.map(Into::into),
                stripe_api: "https://api.stripe.com/".into(),
                stripe_seat_price: String::new(),
                stripe_minutes_price: String::new(),
                stripe_minutes_event: String::new(),
            }
            .stripe()
        };
        assert!(b(None, None).unwrap().is_none(), "off without a key");
        assert!(b(Some(""), None).unwrap().is_none(), "an empty key is none");
        assert!(b(Some("sk_test"), None).is_err(), "a key alone doesn't start");
        let on = b(Some("sk_test"), Some("whsec")).unwrap().unwrap();
        assert_eq!((on.webhook_secret.as_str(), on.api.as_str()), ("whsec", "https://api.stripe.com"));
    }

    #[test]
    fn webhook_signatures() {
        let body = br#"{"type":"x"}"#;
        let mut mac = HmacSha256::new_from_slice(b"whsec_test").unwrap();
        mac.update(b"1000.");
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(verify_signature("whsec_test", &format!("t=1000,v1={sig}"), body, 1100));
        assert!(!verify_signature("whsec_test", &format!("t=1000,v1={sig}"), body, 2000), "too old");
        assert!(!verify_signature("whsec_other", &format!("t=1000,v1={sig}"), body, 1100));
        assert!(!verify_signature("whsec_test", &format!("t=1000,v1={sig}"), br#"{"type":"y"}"#, 1100));
        // Never against an empty secret.
        let mut mac = HmacSha256::new_from_slice(b"").unwrap();
        mac.update(b"1000.");
        mac.update(body);
        let empty = hex::encode(mac.finalize().into_bytes());
        assert!(!verify_signature("", &format!("t=1000,v1={empty}"), body, 1100));
    }

    #[test]
    fn months() {
        let t = 1_790_000_000_000; // 2026-09-21
        assert_eq!(arugula_control_common::day(month_start(t)), "2026-09-01");
    }
}
