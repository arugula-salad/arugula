//! `arugula-control-load`: the load test of control (#530). It signs up
//! people, enrols their machines and opens their pages against a control
//! (a staging copy: docs/control-ops.md, "Load testing"), through the same
//! routes the page and `arugulad` use, then holds them there and reports.
//!
//! - `run --steps 100,1000,5000`: for each step, as many machines in all
//!   (`--per-account` to a person), a page open for `--pages` of the people,
//!   with `--busy` of the channels carrying a busy pane's output; held for
//!   `--hold` seconds, a report every `--report`.
//! - `signup --accounts N`: a burst of sign-ups alone, `--concurrency` at
//!   once, each with a new page's first calls (the platform's among them).
//!
//! Every report is a JSON line in `--out`. A staging control needs the
//! generator's address in `ARUGULA_LOADTEST_EXEMPT_IPS`, or the
//! per-address limits stop it at 10 sign-ups and 30 joins an hour.

mod account;
mod browser;
mod daemon;
mod keys;
mod stats;

use std::{
    io::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_tungstenite::Connector;

use crate::{account::Account, daemon::Daemon, stats::Stats};

#[derive(Parser)]
#[command(about = "Load-test an Arugula control (#530): fake daemons and browser pages through the real routes")]
struct Args {
    /// The control to load, without a trailing slash. Never production.
    #[arg(long, env = "ARUGULA_LOAD_URL")]
    url: String,
    /// Where the JSON reports go (appended).
    #[arg(long, default_value = "load.jsonl")]
    out: PathBuf,
    /// Sign-ups and joins at once.
    #[arg(long, default_value_t = 32)]
    concurrency: usize,
    #[command(subcommand)]
    what: What,
}

#[derive(Subcommand)]
enum What {
    /// Ramp through steps of machines, holding each.
    Run(RunArgs),
    /// A burst of sign-ups alone.
    Signup {
        #[arg(long)]
        accounts: usize,
    },
}

#[derive(clap::Args, Clone)]
struct RunArgs {
    /// Machines in all at each step.
    #[arg(long, value_delimiter = ',', default_value = "100,1000,5000")]
    steps: Vec<usize>,
    /// Machines to a person (control allows 50).
    #[arg(long, default_value_t = 3)]
    per_account: usize,
    /// The share of people with a page open, on all their machines.
    #[arg(long, default_value_t = 0.3)]
    pages: f64,
    /// The share of a page's channels whose machine has a busy pane.
    #[arg(long, default_value_t = 0.1)]
    busy: f64,
    /// A busy pane's messages a second (#443: about 4,000).
    #[arg(long, default_value_t = 4000.0)]
    busy_rate: f64,
    /// An idle pane's messages a second.
    #[arg(long, default_value_t = 0.5)]
    idle_rate: f64,
    /// Each output message's size in bytes (a Noise message of a few cells).
    #[arg(long, default_value_t = 120)]
    msg_bytes: usize,
    /// How often a page times an echo on each channel, in ms.
    #[arg(long, default_value_t = 1000)]
    echo_every_ms: u64,
    /// Seconds to hold each step.
    #[arg(long, default_value_t = 300)]
    hold: u64,
    /// Seconds between reports.
    #[arg(long, default_value_t = 10)]
    report: u64,
    /// People and machines from an earlier run, reused, and kept for the
    /// next.
    #[arg(long)]
    state: Option<PathBuf>,
}

/// What every task shares.
pub struct Ctx {
    pub http: reqwest::Client,
    pub base: url::Url,
    pub origin: String,
    pub tls: Option<Connector>,
    pub stats: Stats,
    pub cfg: Cfg,
}

#[derive(Default)]
pub struct Cfg {
    pub busy: f64,
    pub busy_rate: f64,
    pub idle_rate: f64,
    pub msg_bytes: usize,
    pub echo_every_ms: u64,
}

impl Ctx {
    pub fn at(&self, path: &str) -> String {
        format!("{}{path}", self.base.as_str().trim_end_matches('/'))
    }

    pub fn ws_at(&self, path: &str) -> String {
        let http = self.at(path);
        http.replacen("http", "ws", 1)
    }
}

/// A uniform number in [0, 1).
pub fn rand_f64() -> f64 {
    (u64::from_be_bytes(arugula_e2e::random::<8>()) >> 11) as f64 / (1u64 << 53) as f64
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    url: String,
    people: Vec<Person>,
}

#[derive(Serialize, Deserialize)]
struct Person {
    #[serde(flatten)]
    account: Account,
    /// Its machines' keys (`keys::to_text`).
    machines: Vec<String>,
}

fn tls() -> anyhow::Result<Connector> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    for c in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(c);
    }
    let cfg = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(cfg)))
}

struct Out(Mutex<std::fs::File>);

impl Out {
    fn line(&self, v: &Value) {
        let mut f = self.0.lock().unwrap();
        let _ = writeln!(f, "{v}");
        let _ = f.flush();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let base = url::Url::parse(&args.url).context("--url")?;
    let host = base.host_str().unwrap_or("");
    anyhow::ensure!(
        !(host == "arugula.io" || host.ends_with(".arugula.io") || host.starts_with("illogical-control")),
        "{host} is production: load a staging copy (docs/control-ops.md, \"Load testing\")"
    );
    let origin = base.origin().ascii_serialization();
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).pool_max_idle_per_host(256).build()?;
    let out = Arc::new(Out(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&args.out)?)));
    let mut cfg = Cfg::default();
    if let What::Run(r) = &args.what {
        cfg = Cfg {
            busy: r.busy,
            busy_rate: r.busy_rate,
            idle_rate: r.idle_rate,
            msg_bytes: r.msg_bytes,
            echo_every_ms: r.echo_every_ms,
        };
    }
    let tls = if base.scheme() == "https" { Some(tls()?) } else { None };
    let ctx = Arc::new(Ctx { http, base, origin, tls, stats: Stats::default(), cfg });
    match args.what {
        What::Signup { accounts } => signup(&ctx, &out, accounts, args.concurrency).await,
        What::Run(r) => run(&ctx, &out, &args.url, r, args.concurrency).await,
    }
}

/// Sign up `n` people, `concurrency` at once.
async fn sign_up_many(ctx: &Arc<Ctx>, n: usize, concurrency: usize, from: usize) -> Vec<Account> {
    let gate = Arc::new(Semaphore::new(concurrency));
    let mut tasks = vec![];
    for i in 0..n {
        let (ctx, gate) = (ctx.clone(), gate.clone());
        tasks.push(tokio::spawn(async move {
            let _p = gate.acquire().await;
            for _ in 0..3 {
                match account::sign_up(&ctx, &format!("Load {}", from + i)).await {
                    Ok(a) => return Some(a),
                    Err(e) => {
                        eprintln!("sign-up: {e:#}");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            None
        }));
    }
    let mut got = vec![];
    for t in tasks {
        if let Ok(Some(a)) = t.await {
            got.push(a);
        }
    }
    got
}

async fn signup(ctx: &Arc<Ctx>, out: &Out, n: usize, concurrency: usize) -> anyhow::Result<()> {
    let t = Instant::now();
    let made = sign_up_many(ctx, n, concurrency, 0).await;
    let secs = t.elapsed().as_secs_f64();
    let v = json!({
        "kind": "signup", "asked": n, "made": made.len(), "concurrency": concurrency,
        "seconds": secs, "per_second": made.len() as f64 / secs, "summary": ctx.stats.step_summary(),
    });
    println!("{v:#}");
    out.line(&v);
    Ok(())
}

async fn run(ctx: &Arc<Ctx>, out: &Arc<Out>, url: &str, r: RunArgs, concurrency: usize) -> anyhow::Result<()> {
    let mut state: State = match &r.state {
        Some(p) if p.exists() => serde_json::from_slice(&std::fs::read(p)?)?,
        _ => State { url: url.to_owned(), people: vec![] },
    };
    anyhow::ensure!(state.url == url, "the state file is for {}, not {url}", state.url);
    for p in &mut state.people {
        p.account.keys = Some(Arc::new(keys::from_text(&p.account.browser)?));
    }
    // What's running: machines and pages started, by person.
    let mut running_machines = vec![0usize; 0];
    let mut paged = vec![false; 0];
    // How late a 100 ms timer fires here: the generator's own scheduling
    // delay (geek is shared, and this runs at the lowest priority), to tell
    // its lag from control's in every latency above.
    {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            loop {
                let t = Instant::now();
                tokio::time::sleep(Duration::from_millis(100)).await;
                let late = t.elapsed().saturating_sub(Duration::from_millis(100));
                ctx.stats.sample("generator.lag", late.as_micros().min(u32::MAX as u128) as u32);
            }
        });
    }
    // Reports, every `report` seconds, for the whole run.
    let reporter = {
        let (ctx, out, every) = (ctx.clone(), out.clone(), r.report);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(every));
            tick.tick().await;
            let mut last = Instant::now();
            loop {
                tick.tick().await;
                let mut v = ctx.stats.report(last.elapsed().as_secs_f64());
                last = Instant::now();
                v["kind"] = json!("report");
                v["at"] = json!(arugula_e2e::now_ms());
                eprintln!(
                    "{} | {} daemons {} pages {} channels | rx {}/s | echo p95 {} ms | generator load {} lag p95 {} ms",
                    chrono_ish(),
                    v["open"]["daemons"],
                    v["open"]["browsers"],
                    v["open"]["channels"],
                    v["rate"]["pane_msgs_rx_per_s"],
                    v["latency_ms"]["page.echo"]["p95"],
                    v["generator"]["load1"],
                    v["latency_ms"]["generator.lag"]["p95"],
                );
                out.line(&v);
            }
        })
    };
    for &target in &r.steps {
        let step_started = Instant::now();
        let people = target.div_ceil(r.per_account.max(1));
        // Sign-ups first, as a burst.
        if state.people.len() < people {
            let t = Instant::now();
            let n = people - state.people.len();
            let made = sign_up_many(ctx, n, concurrency, state.people.len()).await;
            out.line(&json!({
                "kind": "signup", "step": target, "asked": n, "made": made.len(),
                "seconds": t.elapsed().as_secs_f64(), "concurrency": concurrency,
            }));
            state.people.extend(made.into_iter().map(|account| Person { account, machines: vec![] }));
            save(&r.state, &state)?;
        }
        // Their machines, up to the step's count.
        let t = Instant::now();
        let mut wanted = vec![];
        let mut left = target;
        for (i, p) in state.people.iter().enumerate() {
            let n = r.per_account.min(left);
            left -= n;
            if p.machines.len() < n {
                wanted.push((i, n - p.machines.len()));
            }
            if left == 0 {
                break;
            }
        }
        let gate = Arc::new(Semaphore::new(concurrency));
        let mut tasks = vec![];
        for (i, n) in wanted {
            let a = Arc::new(Account {
                account: state.people[i].account.account.clone(),
                session: state.people[i].account.session.clone(),
                browser: state.people[i].account.browser.clone(),
                keys: state.people[i].account.keys.clone(),
            });
            for k in 0..n {
                let (ctx, gate, a) = (ctx.clone(), gate.clone(), a.clone());
                tasks.push(tokio::spawn(async move {
                    let _p = gate.acquire().await;
                    let r =
                        account::enrol(&ctx, &a, &format!("load-{}-{k}", &a.account[..6.min(a.account.len())])).await;
                    if let Err(e) = &r {
                        eprintln!("enrol: {e:#}");
                    }
                    (i, r.ok())
                }));
            }
        }
        let mut enrolled = 0;
        for t in tasks {
            if let Ok((i, Some(k))) = t.await {
                state.people[i].machines.push(keys::to_text(&k));
                enrolled += 1;
                // Kept as it goes: a run stopped halfway keeps what joined.
                if enrolled % 200 == 0 {
                    save(&r.state, &state)?;
                }
            }
        }
        out.line(
            &json!({ "kind": "enrol", "step": target, "enrolled": enrolled, "seconds": t.elapsed().as_secs_f64() }),
        );
        save(&r.state, &state)?;
        // Dial the new machines in, and open the new pages.
        running_machines.resize(state.people.len(), 0);
        paged.resize(state.people.len(), false);
        let mut machines_now = 0;
        for (i, p) in state.people.iter().enumerate() {
            let a = Arc::new(Account {
                account: p.account.account.clone(),
                session: p.account.session.clone(),
                browser: p.account.browser.clone(),
                keys: p.account.keys.clone(),
            });
            let mut ids = vec![];
            for (k, m) in p.machines.iter().enumerate() {
                let keys = keys::from_text(m)?;
                ids.push(keys.id());
                if k >= running_machines[i] && machines_now < target {
                    let d = Arc::new(Daemon { keys, account: a.account.clone() });
                    tokio::spawn(daemon::run(ctx.clone(), d));
                    running_machines[i] = k + 1;
                }
                machines_now += 1;
            }
            let opens = ((i + 1) as f64 * r.pages).floor() > (i as f64 * r.pages).floor();
            if opens && !paged[i] && !ids.is_empty() {
                paged[i] = true;
                tokio::spawn(browser::run(ctx.clone(), a, ids));
            }
        }
        out.line(&json!({
            "kind": "step", "step": target, "people": state.people.len(),
            "pages": paged.iter().filter(|p| **p).count(), "setup_seconds": step_started.elapsed().as_secs_f64(),
        }));
        // Sign-ups' and joins' numbers, and the step's own from here on.
        let mut v = ctx.stats.step_summary();
        v["kind"] = json!("setup_summary");
        v["step"] = json!(target);
        out.line(&v);
        tokio::time::sleep(Duration::from_secs(r.hold)).await;
        let mut v = ctx.stats.step_summary();
        v["kind"] = json!("step_summary");
        v["step"] = json!(target);
        eprintln!("step {target}: {v}");
        out.line(&v);
    }
    reporter.abort();
    Ok(())
}

fn save(path: &Option<PathBuf>, s: &State) -> anyhow::Result<()> {
    if let Some(p) = path {
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(s)?)?;
        std::fs::rename(tmp, p)?;
    }
    Ok(())
}

fn chrono_ish() -> String {
    let s = arugula_e2e::now_ms() / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600 % 24, s / 60 % 60, s % 60)
}
