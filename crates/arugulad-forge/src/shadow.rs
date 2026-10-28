//! M171's shadow period: for two weeks the platform's inbox is held up
//! against forge blocks' attention on opted-in machines. With
//! `ARUGULA_INBOX_SHADOW` set (`1`, `on` or `true` for every 15 minutes,
//! or a number of minutes), this machine sends control what its open
//! GitHub blocks on linked repos want of its owner, every so often: each
//! item (repo and number), the reasons its attention rules find whether or
//! not the person looked, and the platform's drafts it shows. Never a
//! title or any text. The platform compares and logs (`POST
//! /api/inbox/shadow`, docs/control-ops.md). Off unless set.

use std::{collections::HashSet, time::Duration};

use arugula_control_wire::projects::{INBOX_SHADOW, InboxShadow, ShadowBlock};
use reqwest::Method;
use tracing::{debug, info};

use super::live;

/// How often to report, from `ARUGULA_INBOX_SHADOW`'s value: `None` is off.
pub fn every(v: Option<&str>) -> Option<Duration> {
    let v = v?.trim().to_ascii_lowercase();
    match v.as_str() {
        "1" | "on" | "true" | "yes" => Some(Duration::from_secs(15 * 60)),
        _ => v.parse::<u64>().ok().filter(|m| *m > 0).map(|m| Duration::from_secs(m * 60)),
    }
}

/// Report every so often, while it's set (module docs).
pub fn start() {
    let Some(every) = every(std::env::var("ARUGULA_INBOX_SHADOW").ok().as_deref()) else { return };
    let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
    info!(minutes = every.as_secs() / 60, "inbox shadow: reporting forge blocks' attention to control");
    crate::task::spawn_on(&rt, async move {
        loop {
            tokio::time::sleep(every).await;
            report().await;
        }
    });
}

/// The open blocks' reports, one per item.
fn blocks() -> Vec<ShadowBlock> {
    let mut seen = HashSet::new();
    live::blocks()
        .iter()
        .filter_map(|b| b.shadow())
        .filter(|s| seen.insert((s.repo.to_ascii_lowercase(), s.number)))
        .collect()
}

async fn report() {
    let Some(control) = live::control() else { return };
    let blocks = blocks();
    if blocks.is_empty() {
        return;
    }
    let n = blocks.len();
    let body = serde_json::to_value(InboxShadow { blocks }).unwrap_or_default();
    match control.cloud_now(Method::POST, INBOX_SHADOW, Some(&body)).await {
        Ok(v) => info!(blocks = n, compared = %v["compared"], agree = %v["agree"], "inbox shadow: reported"),
        // A control from before M171, or one that's away: next time.
        Err(e) => debug!(error = %e, "inbox shadow: not reported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag() {
        assert_eq!(every(None), None);
        assert_eq!(every(Some("")), None);
        assert_eq!(every(Some("0")), None);
        assert_eq!(every(Some("off")), None);
        assert_eq!(every(Some("On")), Some(Duration::from_secs(900)));
        assert_eq!(every(Some("1")), Some(Duration::from_secs(900)));
        assert_eq!(every(Some("5")), Some(Duration::from_secs(300)));
    }
}
