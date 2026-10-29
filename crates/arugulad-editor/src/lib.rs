//! arugulad-editor: the daemon's editor server (`editor/`: VS Code as
//! code-server in an editor block, the editors that join the swarm,
//! Arugula's extension) and Claude Code's IDE (`ide/`: the daemon as the IDE
//! Claude Code connects to). The binary calls [`register`] when its `editor`
//! feature is on; the daemon (`arugulad-base`) reaches it only through
//! `arugulad_base::editors`, so an edit here rebuilds this crate and the
//! binary, not the daemon (#232).

mod editor;
mod ide;

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_proto::{
    BlockType, DiffInfo, Driver, PaneId,
    api::{IdeInfo, IdeOther},
};
use axum::Router;
use futures_util::future::BoxFuture;
use serde_json::Value;

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::{block, classify, mux, pane, paths, perm, procinfo, provider, store, sys};
use arugulad_base::{browser, editors, ports, push, roots, server, sites, task};

use crate::{
    mux::{Api, MuxHandle, PaneCards},
    pane::Launcher,
    server::App,
};

/// Puts the editor in this daemon. Call it before `arugulad_base::main`.
pub fn register() {
    editors::register(Box::new(Editors));
}

struct Editors;

/// Whose the IDE's and arugula.nvim's operations are (`impl Handle<Editor>
/// for IdeGet`): this crate's, so it may handle `arugula-proto`'s operation
/// types (#232).
pub struct Editor;

impl editors::Editors for Editors {
    fn kinds(&self, kinds: &mut block::BlockKinds) {
        kinds.add(BlockType::Editor, block::Make(editor::Editor::create));
    }

    fn routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        use arugula_proto::op::ops::{
            IdeGet, IdeMention, IdeSet, NvimBlockOpen, NvimBlockTheme, NvimPluginGet, NvimPluginInstall,
            NvimPluginRemove, PaneNvimHelp,
        };
        use arugulad_base::ops::OpRoutes;
        r.route("/api/editors/vsix", axum::routing::get(editor::vsix::download))
            .op_of::<IdeGet, Editor>()
            .op_of::<IdeSet, Editor>()
            .op_of::<IdeMention, Editor>()
            .op_of::<NvimPluginInstall, Editor>()
            .op_of::<NvimPluginGet, Editor>()
            .op_of::<NvimPluginRemove, Editor>()
            .op_of::<PaneNvimHelp, Editor>()
            .op_of::<NvimBlockOpen, Editor>()
            .op_of::<NvimBlockTheme, Editor>()
    }

    fn local_routes(&self, r: Router<Arc<App>>) -> Router<Arc<App>> {
        r.route("/api/editors/connect", axum::routing::get(editor::link::connect))
    }

    fn install(&self, s: editors::Settings) {
        editor::server::install(s);
    }

    fn watch(&self, mux: MuxHandle) {
        editor::nvim::watch(mux.clone());
        editor::nvim_plugin::refresh_at_start();
        editor::nvim_block::watch(mux);
    }

    fn open_in_pane(
        &self,
        mux: &MuxHandle,
        pane: PaneId,
        file: &Path,
        line: Option<u32>,
    ) -> BoxFuture<'static, Result<(), String>> {
        let (mux, file) = (mux.clone(), file.to_owned());
        Box::pin(async move { editor::nvim::open_in_pane(&mux, pane, &file, line).await })
    }

    fn nvim_block(&self, mux: &MuxHandle, at: editors::NvimBlockAt) -> BoxFuture<'static, Result<PaneId, String>> {
        let mux = mux.clone();
        let at = editor::nvim_block::At { from: at.from, dir: at.dir, file: at.file, line: at.line, local: at.local };
        Box::pin(async move { editor::nvim_block::open(&mux, at).await })
    }

    fn start_ide(
        &self,
        state_dir: &Path,
        lock_dir: Option<PathBuf>,
        home: &Path,
        launch: Launcher,
    ) -> BoxFuture<'static, Option<Arc<dyn editors::Ide>>> {
        let dir = state_dir.join("ide");
        let lock_dir = lock_dir.unwrap_or_else(|| ide::default_lock_dir(home));
        Box::pin(async move {
            match ide::Ide::start(dir, lock_dir, launch).await {
                Ok(i) => Some(Arc::new(Ide(i)) as Arc<dyn editors::Ide>),
                Err(e) => {
                    tracing::warn!(error = %e, "can't be Claude Code's IDE");
                    None
                }
            }
        })
    }

    fn subcommand(&self, argv: &[String]) -> Option<anyhow::Result<()>> {
        match argv.get(1).map(String::as_str) {
            // `just vsix`: Arugula's VS Code extension, for the marketplaces.
            Some("_vsix") if argv.len() >= 3 => Some((|| {
                let out = Path::new(&argv[2]).join(editor::vsix::file_name());
                std::fs::write(&out, editor::vsix::build())?;
                println!("{}", out.display());
                Ok(())
            })()),
            // Claude Code's IDE connections, kept across daemon restarts (M28).
            #[cfg(unix)]
            Some("_ide_relay") if argv.len() >= 4 => {
                let args = ide::relay::Args { dir: argv[2].clone().into(), lock_dir: argv[3].clone().into() };
                Some((|| Ok(tokio::runtime::Runtime::new()?.block_on(ide::relay::run(args))?))())
            }
            _ => None,
        }
    }
}

/// Claude Code's IDE, as the daemon holds it.
#[derive(Debug)]
struct Ide(Arc<ide::Ide>);

/// A call on the IDE's cards in the mux task, answered with what `f` makes
/// of them (`None` while there are none, or once the daemon is stopping).
async fn on_cards<T: Send + 'static>(
    mux: &MuxHandle,
    f: impl FnOnce(&mut ide::Cards, &mut dyn mux::CardsCx) -> T + Send + 'static,
) -> Option<T> {
    mux.api(|r| {
        Api::Cards(Box::new(move |cards, cx| {
            if let Some(c) = ide::Cards::of(cards) {
                let _ = r.send(f(c, cx));
            }
        }))
    })
    .await
}

impl editors::Ide for Ide {
    fn port(&self) -> u16 {
        self.0.port
    }

    fn info(&self) -> IdeInfo {
        let others = self
            .0
            .others()
            .into_iter()
            .map(|o| IdeOther { name: o.name, port: o.port, pid: o.pid, folders: o.folders, alive: o.alive })
            .collect();
        IdeInfo {
            on: true,
            name: Some(ide::NAME.into()),
            port: Some(self.0.port),
            lock_dir: Some(self.0.lock_dir.clone()),
            diffs: Some(self.diffs_to()),
            others: Some(others),
        }
    }

    fn diffs_to(&self) -> String {
        self.0.diffs_to().unwrap_or_else(|| ide::NAME.into())
    }

    fn set_diffs_to(&self, name: Option<String>) -> io::Result<()> {
        self.0.set_diffs_to(name)
    }

    fn notify(&self, conn: Option<u64>, method: &str, params: Value) {
        self.0.notify(conn, method, params);
    }

    fn run(self: Arc<Self>, mux: MuxHandle) {
        self.0.run(mux);
    }

    fn cards(self: Arc<Self>) -> Box<dyn PaneCards> {
        Box::new(ide::Cards::new(self.0.clone()))
    }

    fn conns_of(&self, mux: &MuxHandle, pane: PaneId) -> BoxFuture<'static, Vec<u64>> {
        let mux = mux.clone();
        Box::pin(async move { on_cards(&mux, move |c, _| c.conns_of(pane)).await.unwrap_or_default() })
    }

    fn diff_of(&self, mux: &MuxHandle, pane: PaneId) -> BoxFuture<'static, Option<(DiffInfo, String, String)>> {
        let mux = mux.clone();
        Box::pin(async move { on_cards(&mux, move |c, _| c.diff_of(pane)).await.flatten() })
    }

    fn answer_diff(
        &self,
        mux: &MuxHandle,
        pane: PaneId,
        id: Option<String>,
        accept: bool,
        text: Option<String>,
        by: Driver,
    ) -> BoxFuture<'static, Result<(), String>> {
        let mux = mux.clone();
        Box::pin(async move {
            on_cards(&mux, move |c, cx| c.answer(cx, pane, id, accept, text, by))
                .await
                .unwrap_or_else(|| Err("the daemon is stopping".into()))
        })
    }
}
