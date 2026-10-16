//! Server-only Managoat adapter. No provider credential or raw error body is
//! returned to a client. Mutating requests are never automatically retried:
//! the provider's client_request_id is correlation, not idempotency.
use anyhow::{Context, Result, bail};
use reqwest::{Client, Method, Url};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug)]
pub struct HttpFailure(pub u16, pub Method);
impl std::fmt::Display for HttpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cloud provider returned HTTP {}", self.0)
    }
}
impl std::error::Error for HttpFailure {}

pub struct Provider {
    client: Client,
    base: Url,
    key: String,
}

impl Provider {
    pub fn new(base: &str, key: String) -> Result<Self> {
        let base = Url::parse(base)?;
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            bail!("invalid cloud provider URL");
        }
        let local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if base.scheme() != "https" && !(base.scheme() == "http" && local) {
            bail!("cloud provider requires HTTPS");
        }
        if key.trim().is_empty() {
            bail!("cloud provider credential is empty");
        }
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("arugula/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base,
            key,
        })
    }

    async fn request(&self, method: Method, segments: &[&str], body: Option<Value>) -> Result<Value> {
        self.query(method, segments, &[], body).await
    }

    pub async fn query(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<Value> {
        let mut url = self.base.clone();
        url.path_segments_mut().map_err(|_| anyhow::anyhow!("invalid cloud provider URL"))?.clear().extend(segments);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let deleting = method == Method::DELETE;
        let mut request = self.client.request(method.clone(), url).bearer_auth(&self.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("cloud provider unavailable")?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND && deleting {
            return Ok(Value::Null);
        }
        if !status.is_success() {
            // Provider error bodies can echo prompts, secret values or IDs.
            return Err(HttpFailure(status.as_u16(), method).into());
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let value: Value = response.json().await.context("invalid cloud provider response")?;
        Ok(value.get("data").cloned().unwrap_or(value))
    }

    /// Each run gets its own conversation/sandbox. Explicitly prohibit the
    /// provider from injecting its account-wide API credential into the sandbox.
    pub async fn launch(&self, agent: &str, environment: &str, run: &str, prompt: &str) -> Result<Value> {
        self.request(
            Method::POST,
            &["api", "conversations"],
            Some(json!({
                "agent_id": agent,
                "environment_id": environment,
                "prompt": prompt,
                "client_request_id": run,
                "sandbox_api_access": "none",
                "labels": {"arugula_run": run},
            })),
        )
        .await
    }

    pub async fn create(&self, collection: &str, body: Value) -> Result<String> {
        let value = self.request(Method::POST, &["api", collection], Some(body)).await?;
        value["id"].as_str().map(str::to_owned).context("cloud resource has no id")
    }

    pub async fn find(&self, collection: &str, run: &str) -> Result<Option<String>> {
        let label = format!("arugula_run:{run}");
        let query = if collection == "conversations" { vec![("label", label.as_str())] } else { vec![("search", run)] };
        let values = self.query(Method::GET, &["api", collection], &query, None).await?;
        let values = values.as_array().context("invalid cloud resource list")?;
        let matches: Vec<_> = values
            .iter()
            .filter(|v| v["metadata"]["arugula_run"] == run || v["labels"]["arugula_run"] == run)
            .collect();
        if matches.len() > 1 {
            bail!("multiple cloud resources for one run");
        }
        matches.first().map(|v| v["id"].as_str().map(str::to_owned).context("cloud resource has no id")).transpose()
    }

    pub async fn remove(&self, collection: &str, id: &str) -> Result<Value> {
        self.request(Method::DELETE, &["api", collection, id], None).await
    }

    pub async fn events(&self, id: &str, cursor: u64) -> Result<Vec<Value>> {
        let cursor = cursor.to_string();
        let value = self
            .query(
                Method::GET,
                &["api", "conversations", id, "events"],
                &[("after", &cursor), ("limit", "1000"), ("blocks", "true"), ("prompts", "true")],
                None,
            )
            .await?;
        value.as_array().cloned().context("invalid cloud event list")
    }

    /// One-time migration read using the user's old credential. The temporary
    /// client is discarded; it is never used to launch a managed run.
    pub async fn legacy_agent(&self, key: String, id: &str) -> Result<Value> {
        Self::new(self.base.as_str(), key)?.request(Method::GET, &["api", "agents", id], None).await
    }

    pub async fn conversation(&self, id: &str) -> Result<Value> {
        self.request(Method::GET, &["api", "conversations", id], None).await
    }

    pub async fn answer(&self, id: &str, request: &str, answer: Value) -> Result<Value> {
        self.request(Method::POST, &["api", "conversations", id, "requests", request], Some(answer)).await
    }

    pub async fn interrupt(&self, id: &str) -> Result<Value> {
        self.request(Method::POST, &["api", "conversations", id, "interrupt"], None).await
    }

    /// Only after result preservation has succeeded.
    pub async fn terminate(&self, id: &str) -> Result<Value> {
        self.request(Method::POST, &["api", "conversations", id, "terminate"], None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn launch_uses_server_auth_and_isolated_sandbox() {
        let received = Arc::new(Mutex::new(None));
        async fn capture(
            State(received): State<Arc<Mutex<Option<Value>>>>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(headers["authorization"], "Bearer server-secret");
            *received.lock().unwrap() = Some(body);
            Json(json!({"data": {"id": "provider-conversation"}}))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().route("/api/conversations", post(capture)).with_state(received.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let provider = Provider::new(&base, "server-secret".into()).unwrap();
        let result = provider.launch("agent", "environment", "run_123", "hello").await.unwrap();
        assert_eq!(result["id"], "provider-conversation");
        let body = received.lock().unwrap().clone().unwrap();
        assert_eq!(body["sandbox_api_access"], "none");
        assert_eq!(body["client_request_id"], "run_123");
        assert_eq!(body["environment_id"], "environment");
        assert!(body.get("channel_id").is_none());
        assert!(body.get("sandbox_id").is_none());
        assert!(!body.to_string().contains("server-secret"));
        server.abort();
    }

    #[test]
    fn rejects_insecure_or_credential_bearing_urls() {
        for base in
            ["http://provider.example", "https://user:secret@provider.example", "https://provider.example?token=secret"]
        {
            assert!(Provider::new(base, "secret".into()).is_err());
        }
    }
}
