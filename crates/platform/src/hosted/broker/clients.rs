//! Registered clients of the inference broker: another product (studio's
//! lobby) that calls it for the accounts it names ("studio is a full
//! client").
//!
//! Two parts, kept apart so the hub's issuer (M228) can take over the
//! first without touching the second:
//!
//! - **Who the client is** ([`ClientVerifier`]): from the credential it
//!   presents (`Authorization: Bearer <credential>`). Until M228 lands,
//!   [`Static`] checks it against the SHA-256 the operator registered in
//!   `ARUGULA_INFERENCE_CLIENTS`; M228 plugs in a verifier of its own
//!   (its registered clients' secrets, or a signed client assertion
//!   checked against the hub's JWKS) that answers the same client id.
//! - **What the client may do** ([`Policy`]): the platform's, by client id,
//!   from the same setting. Whether the platform's key pays for the
//!   accounts it names when nobody's own key does (the lobby's "house"
//!   key, studio-018) and up to what each month, and whether it may name
//!   who pays besides the account (a sponsor or a visitor, which only the
//!   lobby knows about today).
//!
//! A client is trusted to name its accounts: a full client signs its
//! people in. The account it names must be a member of the project it
//! calls for; a sponsor or a visitor it names needn't be, and the platform
//! can't check the lobby's adoption links or grants (M236 moves them, or
//! M228's consent binds them).

use std::{collections::BTreeMap, sync::Arc};

use futures_util::future::BoxFuture;
use sha2::{Digest as _, Sha256};

/// Who a client is, from the credential it presented: its id, or nothing
/// when it's no client's. Never says why: a wrong credential and an
/// unknown client look the same.
pub trait ClientVerifier: Send + Sync {
    fn verify<'a>(&'a self, credential: &'a str) -> BoxFuture<'a, anyhow::Result<Option<String>>>;
}

/// What a registered client may do with the broker.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// What the platform's key may spend for each account the client names
    /// in a UTC month, when no key of theirs pays (studio's lobby:
    /// `STUDIO_SPEND_CAP_USD`, 20). Zero: the platform's key never pays
    /// for this client's accounts beyond what the allowlist gives them.
    pub house_usd: f64,
    /// Whether it may name who pays besides the account: a sponsor (the
    /// lobby's adoption) and a visitor (the lobby's grants).
    pub names_payers: bool,
}

/// One client as `ARUGULA_INFERENCE_CLIENTS` registers it.
#[derive(Clone, PartialEq)]
pub struct Registered {
    pub id: String,
    /// The SHA-256 of its credential.
    pub digest: [u8; 32],
    pub policy: Policy,
}

impl std::fmt::Debug for Registered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registered").field("id", &self.id).field("policy", &self.policy).finish_non_exhaustive()
    }
}

/// `ARUGULA_INFERENCE_CLIENTS`: entries `id:sha256[:house_usd[:payers]]`,
/// comma-separated. `sha256` is the hex SHA-256 of the client's
/// credential (`printf %s "$CREDENTIAL" | sha256sum`), so the platform
/// never holds the credential itself. `house_usd` is [`Policy::house_usd`]
/// (0 when left out); `payers` turns on [`Policy::names_payers`].
pub fn parse(entries: &[String]) -> anyhow::Result<Vec<Registered>> {
    let mut out: Vec<Registered> = Vec::new();
    for e in entries.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
        let parts: Vec<&str> = e.split(':').collect();
        let bad = || anyhow::anyhow!("ARUGULA_INFERENCE_CLIENTS: {:?} isn't id:sha256[:house_usd[:payers]]", parts[0]);
        let (id, digest) = match parts.as_slice() {
            [id, digest, ..] if parts.len() <= 4 => (*id, *digest),
            _ => return Err(bad()),
        };
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(bad());
        }
        let digest: [u8; 32] = hex::decode(digest).ok().and_then(|d| d.try_into().ok()).ok_or_else(bad)?;
        let house_usd = match parts.get(2) {
            None | Some(&"") => 0.0,
            Some(n) => n.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).ok_or_else(bad)?,
        };
        let names_payers = match parts.get(3) {
            None | Some(&"") => false,
            Some(&"payers") => true,
            Some(_) => return Err(bad()),
        };
        if out.iter().any(|r| r.id == id) {
            anyhow::bail!("ARUGULA_INFERENCE_CLIENTS: {id} is registered twice");
        }
        out.push(Registered { id: id.to_owned(), digest, policy: Policy { house_usd, names_payers } });
    }
    Ok(out)
}

/// The SHA-256 of a credential, as [`parse`] takes it.
pub fn digest(credential: &str) -> [u8; 32] {
    Sha256::digest(credential.as_bytes()).into()
}

/// Two digests compared in time that doesn't depend on where they differ.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// The verifier until M228's: the registered digests, compared in constant
/// time.
pub struct Static(pub Vec<Registered>);

impl ClientVerifier for Static {
    fn verify<'a>(&'a self, credential: &'a str) -> BoxFuture<'a, anyhow::Result<Option<String>>> {
        let given = digest(credential);
        let mut found = None;
        for r in &self.0 {
            if same(&r.digest, &given) {
                found = Some(r.id.clone());
            }
        }
        Box::pin(async move { Ok(found) })
    }
}

/// The broker's registered clients: who each is ([`ClientVerifier`]) and
/// what it may do ([`Policy`]).
#[derive(Clone)]
pub struct Clients {
    pub verifier: Arc<dyn ClientVerifier>,
    pub policies: BTreeMap<String, Policy>,
}

impl Default for Clients {
    fn default() -> Self {
        Clients::new(vec![])
    }
}

impl std::fmt::Debug for Clients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clients").field("policies", &self.policies).finish_non_exhaustive()
    }
}

impl Clients {
    /// From `ARUGULA_INFERENCE_CLIENTS`, with [`Static`] as the verifier.
    pub fn new(registered: Vec<Registered>) -> Clients {
        let policies = registered.iter().map(|r| (r.id.clone(), r.policy.clone())).collect();
        Clients { verifier: Arc::new(Static(registered)), policies }
    }

    /// The same policies, with another verifier: what M228 calls with its
    /// own, whose client ids are the ones registered here.
    pub fn with_verifier(mut self, verifier: Arc<dyn ClientVerifier>) -> Clients {
        self.verifier = verifier;
        self
    }

    /// The client that presented `credential`, with its policy: nothing
    /// when it's no registered client's, or one with no policy here.
    pub async fn who(&self, credential: &str) -> anyhow::Result<Option<(String, Policy)>> {
        let Some(id) = self.verifier.verify(credential).await? else { return Ok(None) };
        Ok(self.policies.get(&id).map(|p| (id, p.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexd(s: &str) -> String {
        hex::encode(digest(s))
    }

    #[tokio::test]
    async fn registered_clients() {
        let entries =
            vec![format!("studio:{}:20:payers", hexd("s3cret-studio")), format!("hud:{}", hexd("s3cret-hud"))];
        let clients = Clients::new(parse(&entries).unwrap());
        let (id, p) = clients.who("s3cret-studio").await.unwrap().unwrap();
        assert_eq!((id.as_str(), p.house_usd, p.names_payers), ("studio", 20.0, true));
        let (id, p) = clients.who("s3cret-hud").await.unwrap().unwrap();
        assert_eq!((id.as_str(), p.house_usd, p.names_payers), ("hud", 0.0, false));
        assert!(clients.who("s3cret-studiO").await.unwrap().is_none());
        assert!(clients.who("").await.unwrap().is_none());
        assert!(
            !format!("{clients:?} {:?}", parse(&entries).unwrap()).contains(&hexd("s3cret-studio")),
            "no digest in Debug"
        );

        for bad in ["studio", "studio:abc", "Studio:{d}", "studio:{d}:-1", "studio:{d}:20:all", "studio:{d}:1:payers:x"]
        {
            let e = bad.replace("{d}", &hexd("x"));
            assert!(parse(std::slice::from_ref(&e)).is_err(), "{e}");
        }
        assert!(parse(&[format!("a:{}", hexd("1")), format!("a:{}", hexd("2"))]).is_err(), "twice");
        assert!(parse(&[" ".into()]).unwrap().is_empty());

        // M228's verifier answers the same ids; the policies stay.
        struct Always;
        impl ClientVerifier for Always {
            fn verify<'a>(&'a self, _: &'a str) -> BoxFuture<'a, anyhow::Result<Option<String>>> {
                Box::pin(async { Ok(Some("studio".to_owned())) })
            }
        }
        let swapped = clients.with_verifier(Arc::new(Always));
        assert_eq!(swapped.who("anything").await.unwrap().unwrap().1.house_usd, 20.0);
    }
}
