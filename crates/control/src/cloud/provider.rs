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

    /// A tab: a conversation on a Cloud workspace's machine, in `cwd`. A
    /// workspace's machine is shared, so `sandbox_api_access: "none"` (which
    /// needs a sandbox of the conversation's own) is refused there and not
    /// sent. With no prompt, nothing starts until the first one.
    pub async fn launch_in_cloud_workspace(
        &self,
        agent: &str,
        workspace: &str,
        cwd: &str,
        tab: &str,
        prompt: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "agent_id": agent,
            "workspace_id": workspace,
            "cwd": cwd,
            "client_request_id": tab,
            "labels": {"arugula_tab": tab},
        });
        if let Some(prompt) = prompt {
            body["prompt"] = json!(prompt);
        }
        self.request(Method::POST, &["api", "conversations"], Some(body)).await
    }

    /// A follow-up. Never resent after a lost response: Managoat would open
    /// a second turn.
    pub async fn prompt(&self, id: &str, prompt: &str, client_request_id: &str) -> Result<Value> {
        self.request(
            Method::POST,
            &["api", "conversations", id, "prompts"],
            Some(json!({"prompt":prompt,"client_request_id":client_request_id})),
        )
        .await
    }

    /// The agent or conversation control made for a tab, found by its
    /// `metadata.arugula_tab` or `labels.arugula_tab` after a lost response.
    pub async fn find_cloud_tab_resource(&self, collection: &str, tab: &str) -> Result<Option<String>> {
        let label = format!("arugula_tab:{tab}");
        let query = if collection == "conversations" { vec![("label", label.as_str())] } else { vec![("search", tab)] };
        let values = self.query(Method::GET, &["api", collection], &query, None).await?;
        let values = values.as_array().context("invalid cloud resource list")?;
        let matches: Vec<_> = values
            .iter()
            .filter(|v| v["metadata"]["arugula_tab"] == tab || v["labels"]["arugula_tab"] == tab)
            .collect();
        if matches.len() > 1 {
            bail!("multiple cloud resources for one tab");
        }
        matches.first().map(|v| v["id"].as_str().map(str::to_owned).context("cloud resource has no id")).transpose()
    }

    pub async fn create(&self, collection: &str, body: Value) -> Result<String> {
        let value = self.request(Method::POST, &["api", collection], Some(body)).await?;
        value["id"].as_str().map(str::to_owned).context("cloud resource has no id")
    }

    /// The created resource, whole: an environment's carries its `revision`.
    pub async fn create_value(&self, collection: &str, body: Value) -> Result<Value> {
        let value = self.request(Method::POST, &["api", collection], Some(body)).await?;
        value["id"].as_str().context("cloud resource has no id")?;
        Ok(value)
    }

    /// The environment or vault control made for a Cloud workspace, found by
    /// its `metadata.arugula_workspace` after a lost create response.
    pub async fn find_cloud_workspace_resource(&self, collection: &str, id: &str) -> Result<Option<Value>> {
        let values = self.query(Method::GET, &["api", collection], &[], None).await?;
        let values = values.as_array().context("invalid cloud resource list")?;
        let matches: Vec<_> = values.iter().filter(|v| v["metadata"]["arugula_workspace"] == id).collect();
        if matches.len() > 1 {
            bail!("multiple cloud resources for one cloud workspace");
        }
        matches.first().map(|v| v["id"].as_str().context("cloud resource has no id").map(|_| (*v).clone())).transpose()
    }

    /// A Managoat workspace has no metadata: control names it after the
    /// Cloud workspace's id, on the environment made for it.
    pub async fn create_workspace(&self, name: &str, environment: &str, vault: &str) -> Result<Value> {
        self.create_value("workspaces", json!({"name":name,"environment_id":environment,"vault_id":vault})).await
    }

    pub async fn find_workspace(&self, name: &str, environment: &str) -> Result<Option<Value>> {
        let values = self.query(Method::GET, &["api", "workspaces"], &[], None).await?;
        let values = values.as_array().context("invalid cloud workspace list")?;
        let matches: Vec<_> =
            values.iter().filter(|v| v["name"] == name && v["environment_id"] == environment).collect();
        if matches.len() > 1 {
            bail!("multiple provider workspaces for one cloud workspace");
        }
        Ok(matches.first().map(|v| (*v).clone()))
    }

    pub async fn workspace(&self, id: &str) -> Result<Value> {
        self.request(Method::GET, &["api", "workspaces", id], None).await
    }

    /// Write-only: Managoat never returns the value.
    pub async fn put_vault_secret(&self, vault: &str, key: &str, value: &str) -> Result<Value> {
        self.request(Method::POST, &["api", "vaults", vault, "secrets"], Some(json!({"key":key,"value":value}))).await
    }

    #[allow(dead_code)] // M117: a secret taken off a Cloud workspace's settings.
    pub async fn delete_vault_secret(&self, vault: &str, key: &str) -> Result<Value> {
        self.request(Method::DELETE, &["api", "vaults", vault, "secrets", key], None).await
    }

    /// A partial update: the fields sent replace those fields only.
    #[allow(dead_code)] // M117: saving a Cloud workspace's settings.
    pub async fn update_environment(&self, id: &str, body: Value) -> Result<Value> {
        self.request(Method::PATCH, &["api", "environments", id], Some(body)).await
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
    async fn launch_uses_server_auth_and_the_cloud_workspace() {
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
        let result = provider
            .launch_in_cloud_workspace("agent", "ws", "/home/sprite/app", "cwc_123", Some("hello"))
            .await
            .unwrap();
        assert_eq!(result["id"], "provider-conversation");
        let body = received.lock().unwrap().clone().unwrap();
        assert!(body.get("sandbox_api_access").is_none());
        assert_eq!(body["client_request_id"], "cwc_123");
        assert_eq!((&body["workspace_id"], &body["cwd"]), (&json!("ws"), &json!("/home/sprite/app")));
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
