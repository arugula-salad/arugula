//! What the generator measured: latency samples and counters by name,
//! drained into one JSON line per report window, and gauges of what's open
//! now.

use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::Instant,
};

use serde_json::{Value, json};

#[derive(Default)]
pub struct Stats {
    /// Latencies in microseconds, by name, since the last report.
    lat: Mutex<BTreeMap<String, Vec<u32>>>,
    /// Counts by name, since the last report.
    count: Mutex<BTreeMap<String, u64>>,
    /// Latencies of the whole step, by name, for its summary.
    step_lat: Mutex<BTreeMap<String, Vec<u32>>>,
    step_count: Mutex<BTreeMap<String, u64>>,
    pub daemons: AtomicI64,
    pub browsers: AtomicI64,
    pub channels: AtomicI64,
    /// Pane messages and bytes browsers received.
    pub rx_msgs: AtomicU64,
    pub rx_bytes: AtomicU64,
    /// Pane messages daemons wrote.
    pub tx_msgs: AtomicU64,
}

/// p50, p95, p99 and max of `v` (sorted here), in ms.
fn percentiles(v: &mut [u32]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    v.sort_unstable();
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize] as f64 / 1000.0;
    json!({ "n": v.len(), "p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": at(1.0) })
}

impl Stats {
    pub fn time(&self, name: &str, since: Instant) {
        self.sample(name, since.elapsed().as_micros().min(u32::MAX as u128) as u32);
    }

    pub fn sample(&self, name: &str, micros: u32) {
        for m in [&self.lat, &self.step_lat] {
            m.lock().unwrap().entry(name.to_owned()).or_default().push(micros);
        }
    }

    pub fn add(&self, name: &str, n: u64) {
        for m in [&self.count, &self.step_count] {
            *m.lock().unwrap().entry(name.to_owned()).or_default() += n;
        }
    }

    pub fn inc(&self, name: &str) {
        self.add(name, 1);
    }

    /// The window since the last report, as one JSON object, and a fresh
    /// window.
    pub fn report(&self, secs: f64) -> Value {
        let lat = std::mem::take(&mut *self.lat.lock().unwrap());
        let count = std::mem::take(&mut *self.count.lock().unwrap());
        let rx = self.rx_msgs.swap(0, Ordering::Relaxed);
        let rxb = self.rx_bytes.swap(0, Ordering::Relaxed);
        let tx = self.tx_msgs.swap(0, Ordering::Relaxed);
        json!({
            "open": self.open(),
            "rate": {
                "pane_msgs_rx_per_s": (rx as f64 / secs).round(),
                "pane_kbytes_rx_per_s": (rxb as f64 / secs / 1000.0).round(),
                "pane_msgs_tx_per_s": (tx as f64 / secs).round(),
            },
            "latency_ms": lat.into_iter().map(|(k, mut v)| (k, percentiles(&mut v))).collect::<BTreeMap<_, _>>(),
            "count": count,
            "generator": generator(),
        })
    }

    /// The whole step's latencies and counts, and a fresh step.
    pub fn step_summary(&self) -> Value {
        let lat = std::mem::take(&mut *self.step_lat.lock().unwrap());
        let count = std::mem::take(&mut *self.step_count.lock().unwrap());
        json!({
            "open": self.open(),
            "latency_ms": lat.into_iter().map(|(k, mut v)| (k, percentiles(&mut v))).collect::<BTreeMap<_, _>>(),
            "count": count,
        })
    }

    fn open(&self) -> Value {
        json!({
            "daemons": self.daemons.load(Ordering::Relaxed),
            "browsers": self.browsers.load(Ordering::Relaxed),
            "channels": self.channels.load(Ordering::Relaxed),
        })
    }
}

/// The generator's host: its load average, so a report says when the
/// generator (not control) was the limit.
fn generator() -> Value {
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut f = load.split_whitespace();
    json!({ "load1": f.next().and_then(|v| v.parse::<f64>().ok()) })
}
