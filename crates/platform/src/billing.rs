//! Billing (M22): plans, usage and the Stripe webhook. Dormant on the
//! hosted control, which has no Stripe key. M139 moves it here from
//! `crates/control/src/billing.rs`, reaching the hub only through
//! [`HubPort`](crate::HubPort): teams, and the relay and sandbox meters.
