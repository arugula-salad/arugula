//! The run-log bucket (M213): an S3-compatible store (Cloudflare R2 for
//! the hosted control), reached only through URLs this signs with AWS's
//! Signature Version 4 in the query string ("presigned"), path-style
//! (`<endpoint>/<bucket>/<key>`). The machine puts with them, members get
//! with them, and the platform itself heads, gets and deletes with them:
//! no S3 client, and the key never leaves the platform.
//!
//! A `PUT`'s URL signs its `Content-Length` too, so it takes exactly the
//! bytes it was asked for and no more. The payload isn't signed
//! (`UNSIGNED-PAYLOAD`): the platform checks the object's size before it
//! records it, and keeps the machine's SHA-256 beside it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, bail};
use bytes::Bytes;
use hmac::{KeyInit as _, Mac as _};
use sha2::{Digest as _, Sha256};

type HmacSha256 = hmac::Hmac<Sha256>;

/// The most a project's runs send in a day unless `ARUGULA_RUN_LOGS_DAILY_GIB`
/// says.
pub const DAILY_BYTES: u64 = 5 << 30;

/// The bucket's flags: `ARUGULA_RUN_LOGS_*`. With no bucket, run logs are
/// off, and a run's log stays in its pane.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(id = "run_logs")]
pub struct Args {
    /// The bucket runs' logs and artifacts go to; unset, none do.
    #[arg(id = "run-logs-bucket", long = "run-logs-bucket", env = "ARUGULA_RUN_LOGS_BUCKET")]
    pub bucket: Option<String>,
    /// Its S3 endpoint (R2: `https://<account id>.r2.cloudflarestorage.com`).
    #[arg(id = "run-logs-endpoint", long = "run-logs-endpoint", env = "ARUGULA_RUN_LOGS_ENDPOINT")]
    pub endpoint: Option<String>,
    #[arg(id = "run-logs-region", long = "run-logs-region", env = "ARUGULA_RUN_LOGS_REGION", default_value = "auto")]
    pub region: String,
    /// The token's access key id (R2: an *Object Read & Write* token for
    /// this bucket only).
    #[arg(
        id = "run-logs-access-key-id",
        long = "run-logs-access-key-id",
        env = "ARUGULA_RUN_LOGS_ACCESS_KEY_ID",
        hide_env_values = true
    )]
    pub key_id: Option<String>,
    #[arg(
        id = "run-logs-secret-access-key",
        long = "run-logs-secret-access-key",
        env = "ARUGULA_RUN_LOGS_SECRET_ACCESS_KEY",
        hide_env_values = true
    )]
    pub secret: Option<String>,
    /// How many days the platform keeps its rows for a run's log and
    /// artifacts: the bucket's own rule should say the same.
    #[arg(id = "run-logs-days", long = "run-logs-days", env = "ARUGULA_RUN_LOGS_DAYS", default_value_t = 30)]
    pub days: u64,
    /// The most a project's runs send to the bucket in a day, in GiB:
    /// past it, the rest of a run's log stays in its pane.
    #[arg(
        id = "run-logs-daily-gib",
        long = "run-logs-daily-gib",
        env = "ARUGULA_RUN_LOGS_DAILY_GIB",
        default_value_t = 5
    )]
    pub daily_gib: u64,
}

impl Args {
    /// The bucket, if one is set: then its endpoint and token must be too.
    pub fn bucket(&self) -> Result<Option<Bucket>> {
        let Some(bucket) = self.bucket.clone().filter(|b| !b.is_empty()) else {
            // A bucket's token with no bucket is a mistake, not "off".
            for (v, name) in [
                (&self.endpoint, "ARUGULA_RUN_LOGS_ENDPOINT"),
                (&self.key_id, "ARUGULA_RUN_LOGS_ACCESS_KEY_ID"),
                (&self.secret, "ARUGULA_RUN_LOGS_SECRET_ACCESS_KEY"),
            ] {
                anyhow::ensure!(
                    v.as_deref().is_none_or(str::is_empty),
                    "{name} is set but ARUGULA_RUN_LOGS_BUCKET isn't"
                );
            }
            tracing::info!("no ARUGULA_RUN_LOGS_BUCKET: runs' logs stay in their panes");
            return Ok(None);
        };
        let need = |v: &Option<String>, name: &str| {
            v.clone()
                .filter(|v| !v.is_empty())
                .with_context(|| format!("ARUGULA_RUN_LOGS_BUCKET is set but {name} isn't"))
        };
        let endpoint = need(&self.endpoint, "ARUGULA_RUN_LOGS_ENDPOINT")?;
        let key_id = need(&self.key_id, "ARUGULA_RUN_LOGS_ACCESS_KEY_ID")?;
        let secret = need(&self.secret, "ARUGULA_RUN_LOGS_SECRET_ACCESS_KEY")?;
        anyhow::ensure!(self.days > 0, "ARUGULA_RUN_LOGS_DAYS is at least 1");
        anyhow::ensure!(self.daily_gib > 0, "ARUGULA_RUN_LOGS_DAILY_GIB is at least 1");
        let mut b = Bucket::new(&endpoint, &bucket, &self.region, &key_id, &secret, self.days * 86_400_000)?;
        b.daily_bytes = self.daily_gib << 30;
        tracing::info!(bucket, endpoint, days = self.days, "runs' logs go to a bucket");
        Ok(Some(b))
    }
}

/// The bucket, and the token that signs for it.
#[derive(Clone)]
pub struct Bucket {
    scheme: String,
    /// `host` or `host:port`, as the `Host` header says it.
    host: String,
    /// The endpoint's own path, if it has one, without a trailing slash.
    base: String,
    bucket: String,
    region: String,
    key_id: String,
    secret: String,
    /// How long the platform keeps its rows for a run's uploads.
    pub ttl_ms: u64,
    /// The most bytes a project's runs are given upload URLs for in a day.
    pub daily_bytes: u64,
    /// Chunks of logs read lately, checked: a log followed a page at a
    /// time reads each chunk from the bucket once.
    pub cache: Arc<ChunkCache>,
}

/// The most bytes of checked chunks [`ChunkCache`] keeps.
pub const CACHE_BYTES: usize = 64 << 20;

/// Log chunks read from the bucket and checked against their SHA-256, by
/// key, the least lately read going first past [`CACHE_BYTES`]. A chunk's
/// key is never written again once recorded, so what's here stays true.
#[derive(Default)]
pub struct ChunkCache {
    inner: Mutex<Cached>,
}

#[derive(Default)]
struct Cached {
    /// Each chunk, and when it was last read (by `tick`).
    chunks: HashMap<String, (Bytes, u64)>,
    tick: u64,
    bytes: usize,
}

impl ChunkCache {
    pub fn get(&self, key: &str) -> Option<Bytes> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.tick += 1;
        let tick = g.tick;
        g.chunks.get_mut(key).map(|(b, t)| {
            *t = tick;
            b.clone()
        })
    }

    pub fn put(&self, key: &str, b: Bytes) {
        if b.len() > CACHE_BYTES {
            return;
        }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.tick += 1;
        let tick = g.tick;
        if let Some((old, _)) = g.chunks.insert(key.to_owned(), (b.clone(), tick)) {
            g.bytes -= old.len();
        }
        g.bytes += b.len();
        while g.bytes > CACHE_BYTES {
            let Some(oldest) = g.chunks.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone()) else { break };
            if let Some((b, _)) = g.chunks.remove(&oldest) {
                g.bytes -= b.len();
            }
        }
    }

    /// How many bytes it holds.
    pub fn bytes(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }
}

impl std::fmt::Debug for Bucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bucket")
            .field("endpoint", &format!("{}://{}{}", self.scheme, self.host, self.base))
            .field("bucket", &self.bucket)
            .field("ttl_ms", &self.ttl_ms)
            .finish_non_exhaustive()
    }
}

impl Bucket {
    pub fn new(endpoint: &str, bucket: &str, region: &str, key_id: &str, secret: &str, ttl_ms: u64) -> Result<Self> {
        let u = url::Url::parse(endpoint).with_context(|| format!("the run-log endpoint {endpoint:?}"))?;
        if !matches!(u.scheme(), "https" | "http") || u.query().is_some() {
            bail!("the run-log endpoint is an http(s) URL with no query: {endpoint:?}");
        }
        let host = u.host_str().context("the run-log endpoint has no host")?.to_ascii_lowercase();
        // Plain http only to this machine (a test's bucket): the token signs
        // every request, but the logs themselves would cross in the clear.
        if u.scheme() == "http" && !matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") {
            bail!("the run-log endpoint is https (http only on localhost): {endpoint:?}");
        }
        let host = match u.port() {
            Some(p) => format!("{host}:{p}"),
            None => host,
        };
        if bucket.is_empty() || !bucket.bytes().all(|b| b.is_ascii_alphanumeric() || b"-.".contains(&b)) {
            bail!("the run-log bucket's name is letters, digits, '-' and '.': {bucket:?}");
        }
        Ok(Self {
            scheme: u.scheme().into(),
            host,
            base: u.path().trim_end_matches('/').into(),
            bucket: bucket.into(),
            region: if region.is_empty() { "auto".into() } else { region.into() },
            key_id: key_id.into(),
            secret: secret.into(),
            ttl_ms,
            daily_bytes: DAILY_BYTES,
            cache: Default::default(),
        })
    }

    /// A URL that lets `method` on `key` until `expires_s` seconds after
    /// `now_ms`. `length`: the `Content-Length` it must be sent with (a
    /// `PUT`'s).
    pub fn presign(&self, method: &str, key: &str, length: Option<u64>, expires_s: u64, now_ms: u64) -> String {
        self.presign_with(method, key, length, &[], expires_s, now_ms)
    }

    /// A signed `GET` of `key` that a browser saves as `filename`, as bytes
    /// (`response-content-type` and `response-content-disposition`, which
    /// the bucket answers with), never shows as a page.
    pub fn download(&self, key: &str, filename: &str, expires_s: u64, now_ms: u64) -> String {
        let name: String = filename.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
        let q = [
            ("response-content-disposition", format!("attachment; filename=\"{name}\"")),
            ("response-content-type", "application/octet-stream".to_owned()),
        ];
        self.presign_with("GET", key, None, &q, expires_s, now_ms)
    }

    fn presign_with(
        &self,
        method: &str,
        key: &str,
        length: Option<u64>,
        query: &[(&str, String)],
        expires_s: u64,
        now_ms: u64,
    ) -> String {
        let path = format!("{}/{}/{}", self.base, encode(&self.bucket, false), encode(key, true));
        let headers: Vec<(&str, String)> = length.map(|n| ("content-length", n.to_string())).into_iter().collect();
        presign_url(&Sign {
            method,
            scheme: &self.scheme,
            host: &self.host,
            path: &path,
            region: &self.region,
            key_id: &self.key_id,
            secret: &self.secret,
            at: &amz_date(now_ms),
            expires_s,
            headers: &headers,
            query,
        })
    }
}

/// `now_ms` as SigV4 writes a time: `20130524T000000Z`.
pub fn amz_date(now_ms: u64) -> String {
    jiff::Timestamp::from_millisecond(now_ms as i64)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string()
}

/// S3's URI encoding: every byte but the unreserved ones (`A-Z a-z 0-9 - .
/// _ ~`) as `%XX`, and `/` kept in a path.
pub fn encode(s: &str, path: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || (path && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What one presigned URL is made of.
pub struct Sign<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub host: &'a str,
    /// The path, encoded already ([`encode`]).
    pub path: &'a str,
    pub region: &'a str,
    pub key_id: &'a str,
    pub secret: &'a str,
    /// [`amz_date`]'s.
    pub at: &'a str,
    pub expires_s: u64,
    /// Headers it signs besides `Host`, by their lower-case names.
    pub headers: &'a [(&'a str, String)],
    /// Query parameters it carries besides SigV4's own (a `GET`'s
    /// `response-content-type`), signed with them.
    pub query: &'a [(&'a str, String)],
}

/// The query a presigned URL carries before its signature, sorted.
fn query(s: &Sign, signed: &str) -> String {
    let date = &s.at[..8];
    let credential = format!("{}/{date}/{}/s3/aws4_request", s.key_id, s.region);
    let mut q = vec![
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential", credential),
        ("X-Amz-Date", s.at.to_owned()),
        ("X-Amz-Expires", s.expires_s.to_string()),
        ("X-Amz-SignedHeaders", signed.to_owned()),
    ];
    q.extend(s.query.iter().cloned());
    q.sort_by(|a, b| a.0.cmp(b.0));
    q.iter().map(|(k, v)| format!("{}={}", encode(k, false), encode(v, false))).collect::<Vec<_>>().join("&")
}

/// The signature over `s`, with `query` its canonical query (sorted,
/// without the signature).
pub fn signature(s: &Sign, query: &str) -> String {
    let mut headers: Vec<(String, String)> =
        s.headers.iter().map(|(k, v)| ((*k).to_owned(), v.trim().into())).collect();
    headers.push(("host".into(), s.host.into()));
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let request = format!("{}\n{}\n{query}\n{canonical_headers}\n{signed}\nUNSIGNED-PAYLOAD", s.method, s.path);
    let date = &s.at[..8];
    let scope = format!("{date}/{}/s3/aws4_request", s.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{}\n{scope}\n{}", s.at, hex::encode(Sha256::digest(request.as_bytes())));
    let mut key = format!("AWS4{}", s.secret).into_bytes();
    for part in [date, s.region, "s3", "aws4_request", to_sign.as_str()] {
        let mut mac = HmacSha256::new_from_slice(&key).expect("hmac takes any key");
        mac.update(part.as_bytes());
        key = mac.finalize().into_bytes().to_vec();
    }
    hex::encode(key)
}

/// The names [`Sign::headers`] and `host`, sorted and joined: SigV4's
/// signed headers.
fn signed_headers(s: &Sign) -> String {
    let mut names: Vec<&str> = s.headers.iter().map(|(k, _)| *k).chain(["host"]).collect();
    names.sort_unstable();
    names.join(";")
}

/// The presigned URL for `s`.
pub fn presign_url(s: &Sign) -> String {
    let q = query(s, &signed_headers(s));
    let sig = signature(s, &q);
    format!("{}://{}{}?{q}&X-Amz-Signature={sig}", s.scheme, s.host, s.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS's own example of a presigned GET ("Authenticating Requests:
    /// Using Query Parameters", Signature Version 4): the same URL, to
    /// the byte.
    #[test]
    fn aws_example() {
        let s = Sign {
            method: "GET",
            scheme: "https",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            region: "us-east-1",
            key_id: "AKIAIOSFODNN7EXAMPLE",                     // gitleaks:allow
            secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", // gitleaks:allow
            at: "20130524T000000Z",
            expires_s: 86400,
            headers: &[],
            query: &[],
        };
        assert_eq!(
            presign_url(&s),
            "https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256\
             &X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request\
             &X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host\
             &X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404" // gitleaks:allow
        );
    }

    #[test]
    fn the_cache_keeps_the_latest_read_within_its_bytes() {
        let c = ChunkCache::default();
        let mb = |n: u8| Bytes::from(vec![n; 8 << 20]);
        for i in 0..8 {
            c.put(&format!("k{i}"), mb(i));
        }
        assert_eq!(c.bytes(), 64 << 20);
        assert!(c.get("k0").is_some(), "read lately");
        c.put("k8", mb(8));
        assert!(c.get("k1").is_none(), "the least lately read went");
        assert!(c.get("k0").is_some() && c.get("k8").is_some());
        assert!(c.bytes() <= CACHE_BYTES);
    }

    #[test]
    fn path_style_with_a_length() {
        let b =
            Bucket::new("https://acct.r2.cloudflarestorage.com/", "arugula-run-logs", "", "id", "secret", 1).unwrap();
        let u = b.presign("PUT", "prj_1/run_2/log/0", Some(42), 900, 1_369_353_600_000);
        assert!(u.starts_with("https://acct.r2.cloudflarestorage.com/arugula-run-logs/prj_1/run_2/log/0?"), "{u}");
        assert!(u.contains("X-Amz-SignedHeaders=content-length%3Bhost"), "{u}");
        assert!(u.contains("X-Amz-Credential=id%2F20130524%2Fauto%2Fs3%2Faws4_request"), "{u}");
        assert_eq!(amz_date(1_369_353_600_000), "20130524T000000Z");
        assert_eq!(encode("a b/c~", true), "a%20b/c~");
        assert!(format!("{b:?}").contains("arugula-run-logs") && !format!("{b:?}").contains("secret"));
        assert!(Bucket::new("ftp://x", "b", "", "i", "s", 1).is_err());
        assert!(Bucket::new("http://acct.r2.cloudflarestorage.com", "b", "", "i", "s", 1).is_err(), "http");
        assert!(Bucket::new("http://127.0.0.1:9000", "b", "", "i", "s", 1).is_ok(), "a test's");
        let d = b.download("prj_1/run_2/artifacts/a.txt", "a.txt", 60, 1_369_353_600_000);
        assert!(d.contains("response-content-disposition=attachment%3B%20filename%3D%22a.txt%22"), "{d}");
        assert!(d.contains("response-content-type=application%2Foctet-stream"), "{d}");
        // A token with no bucket is refused, not taken as "off".
        let a = Args { endpoint: Some("https://x".into()), days: 30, daily_gib: 5, ..Default::default() };
        assert!(a.bucket().unwrap_err().to_string().contains("ARUGULA_RUN_LOGS_BUCKET isn't"));
        assert!(Args { days: 30, daily_gib: 5, ..Default::default() }.bucket().unwrap().is_none());
        let a = Args {
            bucket: Some("b".into()),
            endpoint: Some("https://x".into()),
            key_id: Some("i".into()),
            secret: Some("s".into()),
            days: 30,
            daily_gib: 2,
            ..Default::default()
        };
        assert_eq!(a.bucket().unwrap().unwrap().daily_bytes, 2 << 30);
        assert!(Bucket::new("https://x", "b/c", "", "i", "s", 1).is_err());
    }
}
