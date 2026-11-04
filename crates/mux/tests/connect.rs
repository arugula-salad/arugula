//! Who a new client is, as the mux hears of it. A client's handshake and its
//! `Cmd::Connect` are apart, and access can go between them: then its
//! `AclChanged` has been and gone with no client to close, so `Connect` looks
//! again. The real actor, with the commands in that order.

#![cfg(unix)]

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use arugula_mux::{
    acl::{Acl, Principal},
    block::BlockKinds,
    mux::{self, AgentSessions, Cmd, Config, Followups, MuxHandle, Notify, PaneGone, People, Tag, Turn, Usage},
    pane::{Launcher, Subscriber, ToClient, client_queue},
    provider::Provider,
    store::StateDir,
};
use arugula_proto::{PaneId, ServerMsg};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug)]
struct Quiet;

impl Notify for Quiet {
    fn notify(&self, _: PaneId, _: &str, _: &str, _: Option<Value>, _: Option<Value>, _: &dyn Fn(&Principal) -> bool) {}
}

impl People for Quiet {
    fn is_team(&self) -> bool {
        false
    }
    fn owns_here(&self, _: &str) -> bool {
        false
    }
    fn co_owners(&self) -> Vec<Principal> {
        Vec::new()
    }
    fn team_people(&self) -> Vec<Principal> {
        Vec::new()
    }
}

impl AgentSessions for Quiet {
    fn transcript(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn live(&self, _: HashMap<u32, PaneId>) -> HashMap<PaneId, (String, Option<String>)> {
        HashMap::new()
    }
}

impl PaneGone for Quiet {
    fn forget(&self, _: PaneId) {}
    fn forget_on(&self, _: Arc<dyn Provider>, _: String, _: PaneId) {}
}

impl Usage for Quiet {
    fn pane_opened(&self, _: bool, _: usize) {}
    fn block_opened(&self, _: arugula_proto::BlockType, _: &Value) {}
}

impl Followups for Quiet {
    fn turn(&self, _: PaneId, _: &Tag, _: Turn) {}
}

/// A mux with an ACL we can change, and no program that lives: its first
/// pane's shim is `true`.
fn start(name: &str) -> (MuxHandle, Arc<Acl>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("arugula-connect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let acl = Arc::new(Acl::open(&dir));
    let quiet = Arc::new(Quiet);
    let config = Config {
        at_exit: None,
        acl: acl.clone(),
        notify: quiet.clone(),
        people: quiet.clone(),
        owner_name: "owner".into(),
        owner_pic: None,
        guest_machines: 0,
        shell: "/bin/true".into(),
        shell_args: Vec::new(),
        home: dir.clone(),
        manager_env: false,
        launch: Launcher {
            exe: "/bin/true".into(),
            scopes: false,
            no_expand: false,
            service: None,
            fd_store: false,
            hold: false,
        },
        integration: None,
        socket: dir.join("sock"),
        provider: None,
        daemon_id: "test".into(),
        private: Vec::new(),
        kinds: Arc::new(BlockKinds::default()),
        pane_env: Vec::new(),
        shell_env_skip: Vec::new(),
        sessions: quiet.clone(),
        pane_gone: quiet.clone(),
        usage: quiet.clone(),
        followups: quiet,
    };
    let store = StateDir::open(dir.join("state")).unwrap();
    (mux::start(config, store, HashMap::new(), None), acl, dir)
}

fn connect(m: &MuxHandle, client: u64, principal: Principal) -> mpsc::UnboundedReceiver<ToClient> {
    let (data, data_rx) = client_queue();
    // The data side stays open for as long as the test runs.
    std::mem::forget(data_rx);
    let (ctrl, ctrl_rx) = mpsc::unbounded_channel();
    m.send(Cmd::Connect { sub: Subscriber { client, data, ctrl, principal, name: None } });
    ctrl_rx
}

async fn next(rx: &mut mpsc::UnboundedReceiver<ToClient>) -> ToClient {
    tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("the mux answers").expect("still open")
}

fn link(id: &str) -> Principal {
    Principal::User { id: id.to_owned(), name: "someone with a link".into(), pic: None }
}

/// Access goes after a client's handshake and before the mux hears of it:
/// the change's `AclChanged` has already run, with nobody to close. The
/// client hears why and is hung up on, with no state first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_whose_access_went_before_it_connected_is_turned_away() {
    let (m, acl, dir) = start("gone");
    let id = acl.add_link(1, &"a".repeat(64), arugula_mux::store::now_ms() + 60_000, None).unwrap();
    assert!(acl.revoke_link(&id, "owner").unwrap());
    m.send(Cmd::AclChanged);

    let mut ctrl = connect(&m, 901, link(&id));
    match next(&mut ctrl).await {
        ToClient::Msg(ServerMsg::Error { id: None, message }) => assert_eq!(message, "your access was removed"),
        ToClient::Json(j) => panic!("sent state to a client with no access: {}", &j[..j.len().min(120)]),
        other => panic!("not told why: {other:?}"),
    }
    assert!(matches!(next(&mut ctrl).await, ToClient::Close), "and hung up on");
    m.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// Whoever still has access connects as ever, and a change later closes them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_with_access_connects_and_a_later_change_closes_it() {
    let (m, acl, dir) = start("live");
    let id = acl.add_link(1, &"b".repeat(64), arugula_mux::store::now_ms() + 60_000, None).unwrap();

    let mut owner = connect(&m, 902, Principal::Owner);
    assert!(matches!(next(&mut owner).await, ToClient::Json(_)), "the owner gets state");
    let mut viewer = connect(&m, 903, link(&id));
    assert!(matches!(next(&mut viewer).await, ToClient::Json(_)), "a live link gets state");

    assert!(acl.revoke_link(&id, "owner").unwrap());
    m.send(Cmd::AclChanged);
    loop {
        match next(&mut viewer).await {
            ToClient::Close => break,
            ToClient::Msg(ServerMsg::Error { .. }) | ToClient::Json(_) | ToClient::Msg(_) | ToClient::Frame(_) => {}
        }
    }
    m.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
