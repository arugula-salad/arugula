//! M180 over a real relay socket: what a hosted machine says about being
//! busy reaches the platform's `HubPort::activity`, and goes when its
//! socket does.

use std::time::Duration;

use arugula_platform_api::hub::HubPort;
use futures_util::SinkExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    App,
    forge_wire::{daemon, dial},
};

#[tokio::test]
async fn a_machines_activity_reaches_the_platform_while_its_socket_is_up() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = App::for_tests(&base).start();
    let svc = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    let hub = app.hub_port.clone();
    let (busy, quiet) = (daemon(&app, "a1", "busy"), daemon(&app, "a1", "quiet"));
    let ids = [busy.id(), quiet.id()];
    let mut s = dial(&base, &busy).await;
    let _q = dial(&base, &quiet).await;
    s.send(Message::Text(json!({ "t": "activity", "busy": true, "since_ms": 42 }).to_string().into())).await.unwrap();
    let mut got = vec![];
    for _ in 0..100 {
        got = hub.activity(&ids).await.unwrap();
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The quiet one never said: left out.
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].machine.as_str(), got[0].busy, got[0].since_ms), (busy.id().as_str(), true, 42));
    s.close(None).await.unwrap();
    for _ in 0..100 {
        if hub.activity(&ids).await.unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("a machine that left still has activity");
}
