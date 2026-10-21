//! [`Github`]: GitHub's REST API (github.com's, or a GitHub Enterprise
//! host's at `https://HOST/api/v3`) through the caller's HTTP client, with
//! a token from a [`TokenSource`] for each request, asked again after a 401.
//!
//! - **REST with ETags, not GraphQL.** Every GET is conditional
//!   (`If-None-Match` with the ETag it last answered, per URL); a 304
//!   reuses the body kept with it ([`EtagCache`]) and costs no rate-limit
//!   points. A poll is the item, its head's check runs and its combined
//!   status: three 304s when nothing moved. Reviews and the timeline are
//!   read only when the caller asks ([`Github::rest`]), and those are
//!   conditional too.
//! - **Rerun:** a failed check run from Actions names its workflow run;
//!   [`Github::rerun_failed`] posts `actions/runs/ID/rerun-failed-jobs` for
//!   each.
//! - **Rate limits:** with fewer than [`LOW`] requests left, a poll reads
//!   at most once every [`LOW_EVERY`] (the last answer stands between). A
//!   403/429 that says the limit is spent (or gives `Retry-After`) stops
//!   reads until then. [`Github::rate`] says so (`backoff`).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_proto::forge::{RateLimit, ReviewEvent};
use serde_json::{Value, json};

use crate::{
    Error, Polled, Sent,
    model::{Check, Event, Item, Linked, Me, Review, Reviewer},
    parse::{checks, events, fingerprint, issue_item, item, linked, me, reviews},
    redact,
    token::{Purpose, Token, TokenSource},
    util::{EVENTS, short},
};

/// Fewer requests left than this: back off.
pub const LOW: u64 = 100;
/// How often it polls while the limit is low.
pub const LOW_EVERY: Duration = Duration::from_secs(60);

/// How a review event is named in GitHub's API.
fn review_event(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Approve => "APPROVE",
        ReviewEvent::RequestChanges => "REQUEST_CHANGES",
        ReviewEvent::Comment => "COMMENT",
    }
}

/// A merge style as GitHub's `merge_method`.
fn merge_method(style: Option<&str>) -> Result<&'static str, String> {
    match style.unwrap_or("merge") {
        "merge" => Ok("merge"),
        "squash" => Ok("squash"),
        "rebase" => Ok("rebase"),
        s => Err(format!("GitHub merges by merge, squash or rebase, not {s}")),
    }
}

/// A `Link` header's URL for `rel` (`last`, `next`).
fn link_rel(link: &str, rel: &str) -> Option<String> {
    let want = format!("rel=\"{rel}\"");
    link.split(',').find_map(|part| {
        let (url, r) = part.split_once(';')?;
        r.contains(&want).then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_owned())
    })
}

/// A `Link` header's `rel="last"` URL.
fn link_last(link: &str) -> Option<String> {
    link_rel(link, "last")
}

fn with_page(url: &str, page: u64) -> String {
    let Ok(mut u) = url::Url::parse(url) else { return url.to_owned() };
    let pairs: Vec<(String, String)> =
        u.query_pairs().filter(|(k, _)| k != "page").map(|(k, v)| (k.into(), v.into())).collect();
    u.query_pairs_mut().clear().extend_pairs(pairs).append_pair("page", &page.to_string());
    u.to_string()
}

fn page_of(url: &str) -> Option<u64> {
    url::Url::parse(url).ok()?.query_pairs().find(|(k, _)| k == "page").and_then(|(_, v)| v.parse().ok())
}

/// What a URL last answered: its ETag, body and `Link`.
struct Cached {
    etag: String,
    body: Value,
    link: Option<String>,
}

/// Every URL's last answer (memory only), for conditional GETs. Clones
/// share it. Share one only among clients that read as the same person:
/// the daemon shares one among its blocks, so a second block on the same
/// host asks `user` and `user/teams` for free, and so does a block after
/// `refresh`.
#[derive(Clone, Default)]
pub struct EtagCache(Arc<Mutex<HashMap<String, Cached>>>);

/// More URLs than this: start again.
const CACHED: usize = 2000;

#[derive(Default)]
struct Limits {
    limit: Option<u64>,
    remaining: Option<u64>,
    used: Option<u64>,
    /// When the window resets (epoch seconds).
    reset: Option<u64>,
    /// No reads until then (a spent limit, or `Retry-After`).
    until: Option<Instant>,
    why: Option<String>,
    last_poll: Option<Instant>,
    requests: u64,
    not_modified: u64,
}

fn epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn header_u64(h: &reqwest::header::HeaderMap, k: &str) -> Option<u64> {
    h.get(k)?.to_str().ok()?.trim().parse().ok()
}

fn mins(secs: u64) -> String {
    if secs < 90 { format!("{secs}s") } else { format!("{} min", secs.div_ceil(60)) }
}

/// GitHub (or a GitHub Enterprise host), with tokens from `T`. One per
/// thing read: it keeps the rate limit as last heard, and the last poll's
/// answer.
pub struct Github<T> {
    api: String,
    http: reqwest::Client,
    tokens: T,
    cache: EtagCache,
    limits: Mutex<Limits>,
    /// The last poll's answer, reused while backing off, and the head the
    /// reviews are read against.
    last: Mutex<Option<(Item, Vec<Check>, String)>>,
}

impl<T: TokenSource> Github<T> {
    /// `api`: `https://api.github.com`, or `https://HOST/api/v3`. It has a
    /// cache of its own until [`with_cache`](Self::with_cache).
    pub fn new(api: &str, http: reqwest::Client, tokens: T) -> Self {
        Self {
            api: api.trim_end_matches('/').to_owned(),
            http,
            tokens,
            cache: EtagCache::default(),
            limits: Mutex::default(),
            last: Mutex::default(),
        }
    }

    /// ...that shares `cache` with other clients.
    pub fn with_cache(mut self, cache: EtagCache) -> Self {
        self.cache = cache;
        self
    }

    /// Where its tokens come from.
    pub fn tokens(&self) -> &T {
        &self.tokens
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_owned()
        } else {
            format!("{}/{}", self.api, path.trim_start_matches('/'))
        }
    }

    /// Why reads wait now, if they do.
    fn holding(&self) -> Option<String> {
        let mut l = self.limits.lock().unwrap();
        match l.until {
            Some(u) if u > Instant::now() => Some(format!(
                "{} (reads again in {})",
                l.why.clone().unwrap_or_default(),
                mins((u - Instant::now()).as_secs())
            )),
            Some(_) => {
                l.until = None;
                l.why = None;
                None
            }
            None => None,
        }
    }

    fn note_limits(&self, h: &reqwest::header::HeaderMap) {
        let mut l = self.limits.lock().unwrap();
        // Search and GraphQL have limits of their own; these are core's.
        if h.get("x-ratelimit-resource").and_then(|v| v.to_str().ok()).is_some_and(|r| r != "core") {
            return;
        }
        if let Some(r) = header_u64(h, "x-ratelimit-remaining") {
            l.remaining = Some(r);
            l.limit = header_u64(h, "x-ratelimit-limit").or(l.limit);
            l.used = header_u64(h, "x-ratelimit-used").or(l.used);
            l.reset = header_u64(h, "x-ratelimit-reset").or(l.reset);
        }
    }

    async fn token(&self, repo: &str, purpose: Purpose, fresh: bool) -> Result<Token, Error> {
        match self.tokens.token(repo, purpose, fresh).await {
            Ok(Some(t)) => Ok(t),
            Ok(None) if repo.is_empty() => Err(Error::Login(format!("no GitHub token to {purpose} with"))),
            Ok(None) => Err(Error::Login(format!("no GitHub token to {purpose} {repo}"))),
            Err(e) => Err(Error::Login(e)),
        }
    }

    /// One request about `repo` with its token. A GET reads, and is
    /// conditional on what the URL last answered: a 304 hands that back.
    /// Anything else writes. On a 401 the token is asked for once more.
    async fn send(
        &self,
        method: reqwest::Method,
        repo: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(Value, Option<String>), Error> {
        let url = self.url(path);
        let get = method == reqwest::Method::GET;
        if get && let Some(why) = self.holding() {
            return Err(Error::Http(why));
        }
        let purpose = if get { Purpose::Read } else { Purpose::Write };
        for attempt in 0..2 {
            let token = self.token(repo, purpose, attempt > 0).await?;
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("Authorization", format!("Bearer {}", token.secret))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
            let etag = get.then(|| self.cache.0.lock().unwrap().get(&url).map(|c| c.etag.clone())).flatten();
            if let Some(e) = &etag {
                req = req.header("If-None-Match", e);
            }
            if let Some(b) = body {
                req = req.json(b);
            }
            let res = req.send().await.map_err(|e| Error::Http(format!("{}: {}", redact(&url), short(&e))))?;
            let status = res.status();
            let headers = res.headers().clone();
            self.note_limits(&headers);
            self.limits.lock().unwrap().requests += 1;
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                continue;
            }
            if status == reqwest::StatusCode::NOT_MODIFIED {
                self.limits.lock().unwrap().not_modified += 1;
                let c = self.cache.0.lock().unwrap();
                if let Some(c) = c.get(&url) {
                    return Ok((c.body.clone(), c.link.clone()));
                }
                return Err(Error::Http(format!("{}: 304 for nothing asked", redact(&url))));
            }
            let link = headers.get("link").and_then(|v| v.to_str().ok()).map(str::to_owned);
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                let said = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["message"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| text.chars().take(200).collect());
                let retry = header_u64(&headers, "retry-after");
                let spent = header_u64(&headers, "x-ratelimit-remaining") == Some(0);
                if matches!(status.as_u16(), 403 | 429) && (retry.is_some() || spent) {
                    let wait = retry.unwrap_or_else(|| {
                        self.limits.lock().unwrap().reset.map_or(60, |r| r.saturating_sub(epoch()).max(1))
                    });
                    let why = format!("GitHub's rate limit: {said}");
                    {
                        let mut l = self.limits.lock().unwrap();
                        l.until = Some(Instant::now() + Duration::from_secs(wait));
                        l.why = Some(why.clone());
                    }
                    return Err(Error::Http(format!("{why} (reads again in {})", mins(wait))));
                }
                return Err(match status.as_u16() {
                    401 | 403 => Error::Denied(format!("{} {said}", status.as_u16())),
                    404 => Error::NotFound(said),
                    c => Error::Http(format!("{c} {said}")),
                });
            }
            let v =
                if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::Null) };
            if get && let Some(e) = headers.get("etag").and_then(|v| v.to_str().ok()) {
                let mut c = self.cache.0.lock().unwrap();
                if c.len() >= CACHED {
                    c.clear();
                }
                c.insert(url.clone(), Cached { etag: e.to_owned(), body: v.clone(), link: link.clone() });
            }
            return Ok((v, link));
        }
        unreachable!("the loop returns")
    }

    async fn get(&self, repo: &str, path: &str) -> Result<Value, Error> {
        Ok(self.send(reqwest::Method::GET, repo, path, None).await?.0)
    }

    /// A list that's oldest first: the newest `keep` of it (its last page,
    /// and the one before when that's short).
    async fn newest(&self, repo: &str, path: &str, keep: usize) -> Result<Vec<Value>, Error> {
        let (first, link) = self.send(reqwest::Method::GET, repo, path, None).await?;
        let first = first.as_array().cloned().unwrap_or_default();
        let last = link.as_deref().and_then(link_last).and_then(|u| Some((page_of(&u).filter(|n| *n > 1)?, u)));
        let mut all = match last {
            None => first,
            Some((n, url)) => {
                let mut tail = self.get(repo, &url).await?.as_array().cloned().unwrap_or_default();
                if tail.len() < keep {
                    let mut before = if n == 2 {
                        first
                    } else {
                        self.get(repo, &with_page(&url, n - 1)).await?.as_array().cloned().unwrap_or_default()
                    };
                    before.append(&mut tail);
                    tail = before;
                }
                tail
            }
        };
        let skip = all.len().saturating_sub(keep);
        Ok(all.split_off(skip))
    }

    /// The poll's requests, when not backing off.
    async fn poll_now(&self, repo: &str, number: u64) -> Result<Polled, Error> {
        let raw = self.get(repo, &format!("repos/{repo}/pulls/{number}")).await?;
        let it = item(&raw, repo);
        let (runs, status) = match it.head.sha.as_str() {
            "" => (Value::Null, Value::Null),
            sha => {
                let (rp, sp) = (
                    format!("repos/{repo}/commits/{sha}/check-runs?per_page=100"),
                    format!("repos/{repo}/commits/{sha}/status?per_page=100"),
                );
                let (r, st) = futures_util::join!(self.get(repo, &rp), self.get(repo, &sp));
                (r?, st?)
            }
        };
        let checks = checks(&runs, &status);
        let fp = fingerprint(&raw);
        *self.last.lock().unwrap() = Some((it.clone(), checks.clone(), fp.clone()));
        self.limits.lock().unwrap().last_poll = Some(Instant::now());
        Ok(Polled { item: it, checks, fingerprint: fp })
    }

    fn cached(&self) -> Option<Polled> {
        self.last.lock().unwrap().clone().map(|(item, checks, fingerprint)| Polled { item, checks, fingerprint })
    }

    // ------------------------------------------------------------ reads

    /// Who "you" are: `GET /user` and your teams (`GET /user/teams`, which
    /// needs `read:org`; without it, none). When the token isn't the
    /// person's own ([`Token::login`]), that login, with no teams.
    pub async fn me(&self) -> Result<Me, Error> {
        if let Some(login) = self.token("", Purpose::Read, false).await?.login {
            return Ok(Me { login, teams: vec![] });
        }
        let user = self.get("", "user").await?;
        let teams = self.get("", "user/teams?per_page=100").await.unwrap_or(Value::Null);
        Ok(me(&user, &teams))
    }

    /// A pull request and its head's checks. While the rate limit is low,
    /// or spent, the last answer stands.
    pub async fn poll(&self, repo: &str, number: u64) -> Result<Polled, Error> {
        let low = {
            let l = self.limits.lock().unwrap();
            l.remaining.is_some_and(|r| r < LOW)
                && l.reset.is_some_and(|r| r > epoch())
                && l.last_poll.is_some_and(|p| p.elapsed() < LOW_EVERY)
        };
        if (low || self.holding().is_some())
            && let Some(p) = self.cached()
        {
            return Ok(p);
        }
        self.poll_now(repo, number).await
    }

    /// A pull request's reviews (against the head [`poll`](Self::poll)
    /// last saw), the requests still pending, and its newest [`EVENTS`]
    /// timeline events.
    pub async fn rest(&self, repo: &str, number: u64) -> Result<(Vec<Review>, Vec<Reviewer>, Vec<Event>), Error> {
        let (head, requested) = match self.last.lock().unwrap().as_ref() {
            Some((it, _, _)) => (it.head.sha.clone(), it.requested.clone()),
            None => (String::new(), vec![]),
        };
        let rp = format!("repos/{repo}/pulls/{number}/reviews?per_page=100");
        let tp = format!("repos/{repo}/issues/{number}/timeline?per_page=100");
        let (r, tl) = futures_util::join!(self.newest(repo, &rp, 100), self.newest(repo, &tp, EVENTS));
        Ok((reviews(&Value::Array(r?), &head), requested, events(&Value::Array(tl?), repo)))
    }

    /// An issue (not a pull request), and what changes when its timeline
    /// might have.
    pub async fn issue(&self, repo: &str, number: u64) -> Result<(Item, String), Error> {
        let raw = self.get(repo, &format!("repos/{repo}/issues/{number}")).await?;
        if raw["pull_request"].is_object() {
            return Err(Error::NotFound(format!("{repo}#{number} is a pull request: open it as one")));
        }
        let who: Vec<&str> =
            raw["assignees"].as_array().into_iter().flatten().filter_map(|u| u["login"].as_str()).collect();
        let fp = json!([raw["updated_at"], raw["comments"], raw["state"], who]).to_string();
        Ok((issue_item(&raw), fp))
    }

    /// An issue's newest events, and the pull requests that refer to it.
    pub async fn issue_events(&self, repo: &str, number: u64) -> Result<(Vec<Event>, Vec<Linked>), Error> {
        let path = format!("repos/{repo}/issues/{number}/timeline?per_page=100");
        let tl = Value::Array(self.newest(repo, &path, EVENTS).await?);
        Ok((events(&tl, repo), linked(&tl)))
    }

    /// The newest pull request from `branch` in the repository itself.
    pub async fn pr_by_head(&self, repo: &str, branch: &str) -> Result<Option<Linked>, Error> {
        // GitHub filters by `owner:branch`, so a fork's isn't listed.
        let owner = repo.split('/').next().unwrap_or_default();
        let list = self.get(repo, &format!("repos/{repo}/pulls?state=all&head={owner}:{branch}&per_page=10")).await?;
        Ok(list.as_array().into_iter().flatten().find(|p| p["head"]["ref"] == branch).map(|p| {
            let it = item(p, repo);
            Linked { number: it.number, title: it.title, state: it.state, url: it.url, head: Some(branch.to_owned()) }
        }))
    }

    /// The repository's clone URLs (ssh, https) and its page.
    pub async fn repo_urls(&self, repo: &str) -> Result<Vec<String>, Error> {
        let v = self.get(repo, &format!("repos/{repo}")).await?;
        Ok(["ssh_url", "clone_url", "html_url"].iter().filter_map(|k| v[*k].as_str().map(str::to_owned)).collect())
    }

    /// The branch work starts from.
    pub async fn default_branch(&self, repo: &str) -> Result<String, Error> {
        let v = self.get(repo, &format!("repos/{repo}")).await?;
        v["default_branch"]
            .as_str()
            .filter(|b| !b.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| Error::Http(format!("{repo} has no default branch")))
    }

    /// Any GET about `repo`, as GitHub's JSON: what a caller with a path of
    /// its own reads (the platform's mirror, M164).
    pub async fn read(&self, repo: &str, path: &str) -> Result<Value, Error> {
        self.get(repo, path).await
    }

    /// One page of a list about `repo` (GitHub's JSON: a list, or an
    /// object with one, as `check-runs`), and the next page's URL (`Link:
    /// rel="next"`), to pass back as `path`.
    pub async fn page(&self, repo: &str, path: &str) -> Result<(Value, Option<String>), Error> {
        let (v, link) = self.send(reqwest::Method::GET, repo, path, None).await?;
        let next = link.as_deref().and_then(|l| link_rel(l, "next"));
        Ok((v, next))
    }

    /// The rate limit as last heard, and any backing off.
    pub fn rate(&self) -> RateLimit {
        let why = self.holding();
        let l = self.limits.lock().unwrap();
        let low = l.remaining.is_some_and(|r| r < LOW) && l.reset.is_some_and(|r| r > epoch());
        let backoff = why.or_else(|| {
            low.then(|| {
                format!(
                    "GitHub's rate limit is low ({} of {} left, resets in {}): polling once a minute",
                    l.remaining.unwrap_or(0),
                    l.limit.unwrap_or(0),
                    mins(l.reset.unwrap_or(0).saturating_sub(epoch()))
                )
            })
        });
        RateLimit {
            limit: l.limit,
            remaining: l.remaining,
            used: l.used,
            reset: l.reset,
            requests: l.requests,
            not_modified: l.not_modified,
            backoff,
        }
    }

    // ------------------------------------------------------------ writes

    /// Comment on a pull request or an issue.
    pub async fn comment(&self, repo: &str, number: u64, body: &str) -> Result<Sent, Error> {
        let path = format!("repos/{repo}/issues/{number}/comments");
        let (v, _) = self.send(reqwest::Method::POST, repo, &path, Some(&json!({ "body": body }))).await?;
        Ok(Sent { url: v["html_url"].as_str().map(str::to_owned), said: "commented".into() })
    }

    /// Review a pull request.
    pub async fn review(&self, repo: &str, number: u64, event: ReviewEvent, body: Option<&str>) -> Result<Sent, Error> {
        let mut req = json!({ "event": review_event(event) });
        if let Some(b) = body {
            req["body"] = json!(b);
        }
        let path = format!("repos/{repo}/pulls/{number}/reviews");
        let (v, _) = self.send(reqwest::Method::POST, repo, &path, Some(&req)).await?;
        let said = match event {
            ReviewEvent::Approve => "approved",
            ReviewEvent::RequestChanges => "requested changes",
            ReviewEvent::Comment => "reviewed",
        };
        Ok(Sent { url: v["html_url"].as_str().map(str::to_owned), said: said.into() })
    }

    /// Merge a pull request: `merge` (the default), `squash` or `rebase`.
    pub async fn merge(&self, repo: &str, number: u64, style: Option<&str>) -> Result<Sent, Error> {
        let how = merge_method(style).map_err(Error::Http)?;
        let path = format!("repos/{repo}/pulls/{number}/merge");
        self.send(reqwest::Method::PUT, repo, &path, Some(&json!({ "merge_method": how }))).await?;
        Ok(Sent { url: None, said: format!("merged ({how})") })
    }

    /// Rerun the failed jobs of each workflow run behind a pull request's
    /// failed checks.
    pub async fn rerun_failed(&self, repo: &str, number: u64) -> Result<Sent, Error> {
        // The failed checks now (conditional, so free if unchanged).
        let p = self.poll_now(repo, number).await?;
        let mut runs: Vec<String> = Vec::new();
        for c in p.checks.iter().filter(|c| c.state.red()) {
            if let Some(r) = &c.run
                && !runs.contains(&r.id)
            {
                runs.push(r.id.clone());
            }
        }
        if runs.is_empty() {
            return Err(Error::Http(
                "no failed workflow runs to rerun (another app's checks rerun on its own page)".into(),
            ));
        }
        for id in &runs {
            let path = format!("repos/{repo}/actions/runs/{id}/rerun-failed-jobs");
            self.send(reqwest::Method::POST, repo, &path, None).await?;
        }
        let n = runs.len();
        Ok(Sent {
            url: None,
            said: format!("reran the failed jobs of {n} workflow run{}", if n == 1 { "" } else { "s" }),
        })
    }

    /// Open an issue: its number, and where it is.
    pub async fn new_issue(&self, repo: &str, title: &str, body: &str) -> Result<(u64, Sent), Error> {
        let req = json!({ "title": title, "body": body });
        let (v, _) = self.send(reqwest::Method::POST, repo, &format!("repos/{repo}/issues"), Some(&req)).await?;
        let n = v["number"].as_u64().ok_or_else(|| Error::Http("GitHub didn't say the issue's number".into()))?;
        Ok((n, Sent { url: v["html_url"].as_str().map(str::to_owned), said: format!("opened #{n}") }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_pages() {
        assert_eq!(review_event(ReviewEvent::Approve), "APPROVE");
        assert_eq!(merge_method(None), Ok("merge"));
        assert_eq!(merge_method(Some("squash")), Ok("squash"));
        assert!(merge_method(Some("fast-forward-only")).is_err());
        let link = r#"<https://api.github.com/repositories/1/issues/2/timeline?per_page=100&page=2>; rel="next", <https://api.github.com/repositories/1/issues/2/timeline?per_page=100&page=4>; rel="last""#;
        let last = link_last(link).unwrap();
        assert_eq!(page_of(&last), Some(4));
        assert_eq!(link_rel(link, "next").as_deref().and_then(page_of), Some(2));
        assert_eq!(link_rel(link, "prev"), None);
        assert_eq!(page_of(&with_page(&last, 3)), Some(3));
        assert!(with_page(&last, 3).contains("per_page=100"));
    }
}
