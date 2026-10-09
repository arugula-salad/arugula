//! Sandboxes from the home daemon's provider (M4b): listing them, and
//! making a daemon resident in one.
//!
//! **Open shell** needs nothing in the sandbox: it's a pane here whose
//! terminal is a plain exec on the sandbox (`RunRequest::sandbox`, a
//! borrowed machine in `mux.rs`). The output is logged here, but while the
//! pane isn't attached (a daemon restart) the sandbox keeps only what its
//! provider replays, about 6.5 KB on Fly: such shells are disposable.
//!
//! **Promote to resident** copies the static daemon (`just static`) into
//! the sandbox and registers it as a provider *service*, so it starts on
//! every boot and restarts when it exits: after a cold wake (on wisp, a
//! real reboot) it restores its panes from its own disk (M2). It listens on
//! loopback only. The home daemon reaches it through the provider's proxy
//! (`provider_tunnel.rs`), never a public URL, with a token minted here: the daemon
//! there is given only the token's SHA-256 (in its arguments), and refuses
//! anything on loopback without it. Then it joins the host list as a
//! provider host.

use std::{path::PathBuf, sync::Arc, time::Duration};

use arugula_proto::hosts::{Demoted, Host, PromoteRequest, ProviderInfo, ProviderRef, SandboxInfo, SandboxList};
use axum::{Router, http::StatusCode};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

use crate::{provider::ServiceDef, server::App};

/// The daemon's port inside a sandbox, unless asked otherwise.
pub const DEFAULT_PORT: u16 = 7681;
/// The provider service that keeps it running. Frozen (#504): daemons of
/// other versions look for it in each other's sandboxes.
pub const SERVICE: &str = "illogicald";
/// How long a new resident daemon gets to answer.
const START_PATIENCE: Duration = Duration::from_secs(60);
/// Our own machines' sandboxes (VM panes and tabs, any daemon's): not for
/// opening shells on or making resident. Frozen (#504), with
/// `sprite_prefix`: daemons of other versions share an account.
const EPHEMERAL: &str = "illogical-eph-";

/// Where the static binaries to copy in are.
#[derive(Debug, Clone)]
pub struct Binaries {
    pub dir: PathBuf,
}

/// The sandboxes routes, as operations (`ops/sandboxes.rs`).
pub fn routes() -> Router<Arc<App>> {
    use crate::ops::OpRoutes;
    use arugula_proto::op::ops::{SandboxDemote, SandboxPromote, SandboxesList};
    Router::new().op::<SandboxesList>().op::<SandboxPromote>().op::<SandboxDemote>()
}

pub async fn list(app: &App) -> Result<SandboxList, Failed> {
    let Some(p) = app.mux.provider.clone() else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "no sandbox provider is set up here".into()));
    };
    let all = match p.list("").await {
        Ok(l) => l,
        Err(e) => return Err((StatusCode::BAD_GATEWAY, format!("{}: {e}", p.name()))),
    };
    let hosts = app.hosts.list().hosts;
    let resident = |name: &str| {
        hosts
            .iter()
            .find(|h| h.provider.as_ref().is_some_and(|at| at.provider == p.name() && at.sandbox == name))
            .map(|h| h.name.clone())
    };
    let sandboxes = all
        .into_iter()
        .filter(|s| !s.name.starts_with(EPHEMERAL))
        .map(|s| SandboxInfo { host: resident(&s.name), name: s.name, status: s.status })
        .collect();
    let caps = p.caps();
    let provider =
        ProviderInfo { name: p.name().to_owned(), exec_replay: caps.exec_replay, resident: caps.fs && caps.services };
    Ok(SandboxList { provider, sandboxes })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn mint_token() -> String {
    let b = crate::push::random::<32>();
    format!("ilp_{}", hex(&b))
}

/// Stop the resident daemon in a sandbox (its provider service) and take
/// its host off the list. Its panes end with it; its state stays on the
/// sandbox's disk.
pub async fn demote(app: &App, sandbox: &str) -> Result<Demoted, Failed> {
    let Some(p) = app.mux.provider.clone() else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "no sandbox provider is set up here".into()));
    };
    if let Err(e) = p.delete_service(sandbox, SERVICE).await {
        return Err((StatusCode::BAD_GATEWAY, format!("{e:#}")));
    }
    let hosts: Vec<String> = app
        .hosts
        .list()
        .hosts
        .into_iter()
        .filter(|h| h.provider.as_ref().is_some_and(|at| at.provider == p.name() && at.sandbox == sandbox))
        .map(|h| h.name)
        .collect();
    for h in &hosts {
        app.hosts.remove(h);
    }
    info!(sandbox, ?hosts, "resident daemon stopped");
    Ok(Demoted { removed: hosts })
}

pub type Failed = (StatusCode, String);

pub async fn make_resident(app: &App, sandbox: &str, req: PromoteRequest) -> Result<Host, Failed> {
    let bad = |s: String| (StatusCode::BAD_REQUEST, s);
    let gateway = |e: anyhow::Error| (StatusCode::BAD_GATEWAY, format!("{e:#}"));
    let p = app.mux.provider.clone().ok_or((StatusCode::SERVICE_UNAVAILABLE, "no sandbox provider here".into()))?;
    if !(p.caps().fs && p.caps().services) {
        return Err(bad(format!("{} can't keep a daemon running (no files or services)", p.name())));
    }
    if sandbox.starts_with(EPHEMERAL) {
        return Err(bad(format!("{sandbox} is a VM pane's machine")));
    }
    let binaries = app.binaries.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "no static daemon to copy in: build it with `just static` and start arugulad with --static-dir".into(),
    ))?;
    let daemon = tokio::fs::read(binaries.dir.join("arugulad"))
        .await
        .map_err(|e| bad(format!("reading {}/arugulad: {e}", binaries.dir.display())))?;
    // The CLI, for programs in its panes; optional.
    let cli = tokio::fs::read(binaries.dir.join("arugula")).await.ok();
    if p.status(sandbox).await.map_err(gateway)?.is_none() {
        return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox}")));
    }
    let host = req.host.unwrap_or_else(|| sandbox.to_owned());
    let port = req.port.unwrap_or(DEFAULT_PORT);
    info!(sandbox, host, port, "making a daemon resident");
    // Its home directory (this wakes it, which the copy would anyway).
    let (out, code) = p.run(sandbox, &["sh", "-c", "printf %s \"$HOME\""]).await.map_err(gateway)?;
    let home = String::from_utf8_lossy(&out).trim().to_owned();
    if code != Some(0) || !home.starts_with('/') {
        return Err((StatusCode::BAD_GATEWAY, format!("can't find {sandbox}'s home directory")));
    }
    let bin = format!("{home}/.local/bin");
    p.write_file(sandbox, &format!("{bin}/arugulad"), daemon, 0o755).await.map_err(gateway)?;
    if let Some(cli) = cli {
        p.write_file(sandbox, &format!("{bin}/arugula"), cli, 0o755).await.map_err(gateway)?;
    }
    let token = mint_token();
    let digest = hex(&Sha256::digest(token.as_bytes()));
    let def = ServiceDef {
        cmd: format!("{bin}/arugulad"),
        args: vec![
            "--listen".into(),
            format!("127.0.0.1:{port}"),
            "--name".into(),
            host.clone(),
            "--provider-token-sha256".into(),
            digest,
            "--no-manager-env".into(),
        ],
        env: vec![("PATH".into(), format!("{bin}:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin"))],
        dir: Some(home.clone()),
    };
    p.put_service(sandbox, SERVICE, &def).await.map_err(gateway)?;
    // It answers (through the provider, with the token) before it's listed.
    let deadline = tokio::time::Instant::now() + START_PATIENCE;
    loop {
        match ask_host(&*p, sandbox, port, &token).await {
            Ok(name) if name == host => break,
            Ok(other) => return Err((StatusCode::BAD_GATEWAY, format!("{sandbox} answered as {other}, not {host}"))),
            Err(e) if tokio::time::Instant::now() >= deadline => {
                return Err((StatusCode::BAD_GATEWAY, format!("the daemon in {sandbox} didn't answer: {e}")));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let at = ProviderRef { provider: p.name().to_owned(), sandbox: sandbox.to_owned(), port };
    let h = app.hosts.add_provider(host, at, token).map_err(bad)?;
    app.hosts.note_status(&h.name, "running");
    info!(sandbox, host = h.name, "resident daemon added");
    Ok(app.hosts.list().hosts.into_iter().find(|x| x.name == h.name).unwrap_or(h))
}

/// `GET /api/host` through the provider, as the tunnel would: its name.
async fn ask_host(p: &dyn crate::provider::Provider, sandbox: &str, port: u16, token: &str) -> anyhow::Result<String> {
    let mut c = p.dial(sandbox, port).await?;
    let req = format!(
        "GET /api/host HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    c.write_all(req.as_bytes()).await?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), c.read_to_end(&mut buf)).await??;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    if !head.starts_with("HTTP/1.1 200") {
        anyhow::bail!("{}", head.lines().next().unwrap_or("no answer"));
    }
    // The body may be chunked: find the JSON object in it.
    let json = body.find('{').and_then(|a| body.rfind('}').map(|b| &body[a..=b])).unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(json)?;
    Ok(v["name"].as_str().unwrap_or_default().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_unique() {
        let (a, b) = (mint_token(), mint_token());
        assert_ne!(a, b);
        assert!(a.starts_with("ilp_") && a.len() == 4 + 64);
    }
}
