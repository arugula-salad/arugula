//! The provider VM panes run on, behind the machine's `sandbox-providers`
//! flag (#489). The flag is read on every call, as every Labs gate's is, so
//! it works on the running daemon: with the flag off no adapter is opened
//! (no token read, nothing asked of a provider) and every call gets the
//! flag's refusal; turning it on opens the adapter on the next call;
//! turning it off again drops it.
//!
//! What the daemon holds (`mux.provider`) is `Some` when a provider is set
//! up here, its credential on disk, flag or not: `GET /api/host`'s `vms`
//! and the menus follow the flag and that, as they did.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use arugula_mux::provider::{
    Begin, Caps, Cold, Conn, Exec, ExecEvent, ExecSink, Pipe, PipeBegin, Provider, Sandbox, ServiceDef,
};
use arugula_proto::{flags::SANDBOX_PROVIDERS, fs::FsList};
use futures_util::future::BoxFuture;
use tracing::info;

/// Opens the adapter: `None` when it can't (no token, a bad URL), which it
/// says in the log.
pub type Open = Box<dyn Fn() -> Option<Arc<dyn Provider>> + Send + Sync>;

pub struct Gated {
    state_dir: PathBuf,
    /// The adapter's name before it has been opened (`machines.rs` keeps it
    /// with each machine): `loam`, or `wisp` for the Sprites API.
    kind: &'static str,
    open: Open,
    inner: Mutex<Option<Arc<dyn Provider>>>,
    /// The adapter's capabilities, from its first opening (the same adapter
    /// each time). Before that, [`OFF`].
    caps: OnceLock<Caps>,
}

/// Nothing can be done: what `caps` says before an adapter has been opened.
static OFF: Caps = Caps {
    exec_replay: 0,
    exec_offset: false,
    reattach_owner: false,
    reattach_resize: false,
    kill_signal: false,
    kill_grace_ms: 0,
    cold: Cold::Reboot,
    fs: false,
    fs_browse: false,
    services: false,
};

impl std::fmt::Debug for Gated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gated").field("kind", &self.kind).field("open", &self.inner.lock().unwrap().is_some()).finish()
    }
}

impl Gated {
    /// With the flag on now, the adapter is opened at once, so the log
    /// says at startup whether VM panes are on.
    pub fn new(state_dir: &Path, kind: &'static str, open: Open) -> Self {
        let g = Self { state_dir: state_dir.to_owned(), kind, open, inner: Mutex::new(None), caps: OnceLock::new() };
        if g.flag_on() {
            g.adapter();
        } else {
            info!(provider = kind, "VM panes are set up, with their flag off");
        }
        g
    }

    fn flag_on(&self) -> bool {
        arugula_proto::flags::get(&self.state_dir, SANDBOX_PROVIDERS)
    }

    /// The adapter, opened if the flag is on and it isn't yet; `None` with
    /// the flag off (dropping one that was open) or when opening fails.
    fn adapter(&self) -> Option<Arc<dyn Provider>> {
        let mut inner = self.inner.lock().unwrap();
        if !self.flag_on() {
            if inner.take().is_some() {
                info!(provider = self.kind, "VM panes off: the flag was turned off");
            }
            return None;
        }
        if inner.is_none() {
            let p = (self.open)();
            info!(provider = self.kind, on = p.is_some(), "VM panes");
            if let Some(p) = &p {
                let _ = self.caps.set(p.caps().clone());
            }
            *inner = p;
        }
        inner.clone()
    }

    /// The adapter, or why a call can't have it: the flag's refusal, or
    /// that the adapter couldn't be opened.
    fn want(&self) -> anyhow::Result<Arc<dyn Provider>> {
        if let Some(p) = self.adapter() {
            return Ok(p);
        }
        arugula_mux::labs::flag(&self.state_dir, SANDBOX_PROVIDERS).map_err(anyhow::Error::msg)?;
        Err(anyhow::anyhow!("VM panes aren't set up: the {} provider couldn't be opened", self.kind))
    }
}

impl Provider for Gated {
    fn name(&self) -> &str {
        self.kind
    }

    fn caps(&self) -> &Caps {
        self.adapter();
        self.caps.get().unwrap_or(&OFF)
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Sandbox>>> {
        Box::pin(async move { self.want()?.list(prefix).await })
    }

    fn status<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<Option<Sandbox>>> {
        Box::pin(async move { self.want()?.status(name).await })
    }

    fn create<'a>(&'a self, name: &'a str, image: Option<&'a str>) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.create(name, image).await })
    }

    fn delete<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.delete(name).await })
    }

    fn wake<'a>(&'a self, name: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.wake(name).await })
    }

    fn dial<'a>(&'a self, name: &'a str, port: u16) -> BoxFuture<'a, io::Result<Conn>> {
        Box::pin(async move { self.want().map_err(io::Error::other)?.dial(name, port).await })
    }

    /// With the flag off (or no adapter), the session is lost at once, with
    /// the refusal as its output, as a build without Labs loses it.
    fn exec_tty(
        self: Arc<Self>,
        rt: &tokio::runtime::Handle,
        name: String,
        begin: Begin,
        size: (u16, u16),
        sink: ExecSink,
    ) -> Exec {
        match self.want() {
            Ok(p) => p.exec_tty(rt, name, begin, size, sink),
            Err(e) => {
                sink(ExecEvent::Output(format!("{e}\r\n").into_bytes()));
                sink(ExecEvent::Lost { machine_gone: false });
                Exec { tx: tokio::sync::mpsc::unbounded_channel().0 }
            }
        }
    }

    fn kill<'a>(&'a self, name: &'a str, session: &'a str, signal: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.kill(name, session, signal).await })
    }

    fn run<'a>(&'a self, name: &'a str, argv: &'a [&'a str]) -> BoxFuture<'a, anyhow::Result<(Vec<u8>, Option<i32>)>> {
        Box::pin(async move { self.want()?.run(name, argv).await })
    }

    fn pipe<'a>(&'a self, name: &'a str, begin: PipeBegin) -> BoxFuture<'a, anyhow::Result<Pipe>> {
        Box::pin(async move { self.want()?.pipe(name, begin).await })
    }

    fn write_file<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
        data: Vec<u8>,
        mode: u32,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.write_file(name, path, data, mode).await })
    }

    fn put_service<'a>(
        &'a self,
        name: &'a str,
        service: &'a str,
        def: &'a ServiceDef,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.put_service(name, service, def).await })
    }

    fn delete_service<'a>(&'a self, name: &'a str, service: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move { self.want()?.delete_service(name, service).await })
    }

    fn fs_list<'a>(&'a self, name: &'a str, path: &'a str) -> BoxFuture<'a, anyhow::Result<FsList>> {
        Box::pin(async move { self.want()?.fs_list(name, path).await })
    }

    fn fs_read<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
        offset: u64,
        len: u64,
    ) -> BoxFuture<'a, anyhow::Result<(Vec<u8>, u64)>> {
        Box::pin(async move { self.want()?.fs_read(name, path, offset, len).await })
    }
}
