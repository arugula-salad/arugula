//! Durable, at-least-once Web Push for cloud approvals. Repeated deliveries
//! use the same notification tag. Cloud payloads are distinct from opaque
//! terminal notifications and contain no prompt or secret values.
use crate::App;
use aes_gcm::{Aes128Gcm, KeyInit, aead::Aead};
use anyhow::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hkdf::Hkdf;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;
use std::sync::Arc;

fn public_bytes(key: &SecretKey) -> Vec<u8> {
    key.public_key().to_sec1_bytes().to_vec()
}

pub fn start(app: Arc<App>) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&app).await {
                tracing::warn!(%error,"cloud notifications will retry");
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

async fn tick(app: &App) -> Result<()> {
    let now = arugula_e2e::now_ms();
    for n in app.db.cloud_notifications(now)? {
        let target = match &n.cloud_workspace {
            Some(cloud_workspace) => Target::Tab { cloud_workspace, tab: &n.conversation },
            None => Target::Run(&n.conversation),
        };
        let delivered = send(app, &n.account, target).await.is_ok();
        app.db.cloud_notification_result(&n.conversation, &n.request, delivered, now + 30_000)?;
    }
    Ok(())
}

/// The conversation a push opens: a run, or a tab in a Cloud workspace.
#[derive(Clone, Copy)]
enum Target<'a> {
    Run(&'a str),
    Tab { cloud_workspace: &'a str, tab: &'a str },
}

/// What the push says: no prompt, no request, only where to go.
fn payload(target: Target<'_>) -> serde_json::Value {
    match target {
        Target::Run(run) => serde_json::json!({
            "title":"Arugula cloud agent", "body":"An agent needs your input.",
            "tag":format!("cloud-{run}"), "cloud_run":run,
        }),
        Target::Tab { cloud_workspace, tab } => serde_json::json!({
            "title":"Arugula cloud agent", "body":"An agent needs your input.",
            "tag":format!("cloud-{tab}"), "cloud_workspace":cloud_workspace, "cloud_tab":tab,
        }),
    }
}

async fn send(app: &App, account: &str, target: Target<'_>) -> Result<()> {
    let payload = serde_json::to_vec(&payload(target))?;
    for value in app.db.push_subs(account)? {
        let sub: arugula_e2e::push::PushSub = serde_json::from_value(value)?;
        let Some((cert, true)) = app.db.device(account, &sub.device)? else {
            continue;
        };
        if !sub.signed_by(&cert)
            || !crate::push::allowed_endpoint(app, &sub.endpoint)
            || app.db.revocations(account)?.iter().any(|r| r.device == sub.device)
        {
            continue;
        }
        let key = loop {
            if let Ok(key) = SecretKey::from_slice(&arugula_e2e::random::<32>()) {
                break key;
            }
        };
        let encrypted =
            encrypt(&payload, &B64.decode(&sub.p256dh)?, &B64.decode(&sub.auth)?, &key, &arugula_e2e::random::<16>())?;
        let url = url::Url::parse(&sub.endpoint)?;
        let audience = url.origin().ascii_serialization();
        let response = app
            .http
            .post(url)
            .header("TTL", "300")
            .header("Urgency", "high")
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header(
                "Authorization",
                format!("vapid t={}, k={}", app.vapid.jwt(&audience, &app.cfg.public_url), app.vapid.public()),
            )
            .body(encrypted)
            .send()
            .await?;
        let status = response.status().as_u16();
        if matches!(status, 404 | 410) {
            app.db.drop_push_sub(&sub.endpoint, Some(account))?;
        } else if !response.status().is_success() {
            anyhow::bail!("push service returned HTTP {status}");
        }
    }
    Ok(())
}

fn encrypt(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    local: &SecretKey,
    salt: &[u8; 16],
) -> anyhow::Result<Vec<u8>> {
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| anyhow::anyhow!("bad p256dh key"))?;
    let as_public = public_bytes(local);
    let shared = p256::ecdh::diffie_hellman(local.to_nonzero_scalar(), ua.as_affine());

    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(&as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes().as_ref())
        .expand(&key_info, &mut ikm)
        .map_err(|_| anyhow::anyhow!("hkdf"))?;

    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| anyhow::anyhow!("hkdf"))?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| anyhow::anyhow!("hkdf"))?;

    let mut padded = plaintext.to_vec();
    padded.push(0x02); // the last (and only) record
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| anyhow::anyhow!("aes key"))?;
    let sealed = cipher.encrypt(&nonce.into(), padded.as_slice()).map_err(|_| anyhow::anyhow!("aes-gcm"))?;

    let mut out = Vec::with_capacity(16 + 4 + 1 + as_public.len() + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend_from_slice(&as_public);
    out.extend_from_slice(&sealed);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn sends_encrypted_cloud_push_and_removes_expired_subscription() {
        use arugula_e2e::{Cert, DeviceKeys, Kind, push::PushSub};
        use axum::{
            Router,
            body::Bytes,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = received.clone();
        let router = Router::new().route(
            "/push",
            post(move |headers: HeaderMap, body: Bytes| {
                let count = count.clone();
                async move {
                    assert_eq!(headers["content-encoding"], "aes128gcm");
                    assert!(headers["authorization"].to_str().unwrap().starts_with("vapid t="));
                    assert!(body.len() > 100);
                    assert!(!String::from_utf8_lossy(&body).contains("needs your input"));
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    StatusCode::GONE
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut app = crate::cloud::tests::test_app();
        Arc::get_mut(&mut app).unwrap().cfg.push_hosts.push(address.to_string());
        let device = DeviceKeys::generate();
        let mut cert = Cert::new(&device, "a", Kind::Browser, "test browser");
        cert.sign_with(&device);
        app.db.put_device(&cert, true, 0).unwrap();
        let browser_key = SecretKey::from_slice(&[1u8; 32]).unwrap();
        let mut sub = PushSub {
            v: 1,
            account: "a".into(),
            device: String::new(),
            endpoint: format!("http://{address}/push"),
            p256dh: B64.encode(public_bytes(&browser_key)),
            auth: B64.encode([2u8; 16]),
            at: 0,
            sig: String::new(),
        };
        sub.sign_with(&device);
        app.db.put_push_sub(&sub.endpoint, "a", &serde_json::to_string(&sub).unwrap()).unwrap();
        send(&app, "b", Target::Run("run_test")).await.unwrap();
        assert_eq!(received.load(std::sync::atomic::Ordering::SeqCst), 0);
        send(&app, "a", Target::Run("run_test")).await.unwrap();
        assert_eq!(received.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            payload(Target::Tab { cloud_workspace: "cw_1", tab: "cwc_1" }),
            serde_json::json!({"title":"Arugula cloud agent","body":"An agent needs your input.",
                "tag":"cloud-cwc_1","cloud_workspace":"cw_1","cloud_tab":"cwc_1"})
        );
        assert!(app.db.push_subs("a").unwrap().is_empty());
        server.abort();
    }

    #[test]
    fn rfc8291_example() {
        let d = |s: &str| B64.decode(s).unwrap();
        let local = SecretKey::from_slice(&d("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        assert_eq!(
            B64.encode(public_bytes(&local)),
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
        );
        let ua_public = d("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let body = encrypt(b"When I grow up, I want to be a watermelon", &ua_public, &auth, &local, &salt).unwrap();
        assert_eq!(
            B64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }
}
