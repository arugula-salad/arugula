//! Durable, bounded reconciliation steps. Browser connections never own work.
use crate::{App, db::cloud::Job};
use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn start(app: Arc<App>) {
    for _ in 0..4 {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = tick(&app).await {
                    tracing::warn!(%error, "cloud reconciliation failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }
}

pub async fn tick(app: &App) -> Result<()> {
    tick_at(app, arugula_e2e::now_ms()).await
}

async fn tick_at(app: &App, now: u64) -> Result<()> {
    let Some(_) = &app.cloud else {
        return Ok(());
    };
    let Some(mut job) = app.db.claim_cloud_job(now)? else {
        return Ok(());
    };
    if let Err(error) = step(app, &mut job, now).await {
        let rejected = error
            .downcast_ref::<super::provider::HttpFailure>()
            .is_some_and(|e| e.1 == Method::POST && (400..500).contains(&e.0) && e.0 != 408);
        if rejected
            && matches!(
                job.execution.stage.as_str(),
                "environment_creating" | "agent_creating" | "conversation_creating"
            )
        {
            job.execution.outcome = Some("failed".into());
            job.execution.stage = "cleanup_agent".into();
            return save(
                app,
                &job,
                "cleaning_up",
                Some(&json!({"message":"Cloud setup was rejected by the execution service."})),
                now,
            );
        }
        // Keep the durable intent on failure. In particular, never repeat a
        // creation when its response may have been lost.
        app.db.checkpoint_cloud_job(&job, "recovering", None, Some(now + 10_000))?;
        tracing::warn!(run = %job.run.id, %error, "cloud run will reconcile");
    }
    Ok(())
}

fn save(app: &App, job: &Job, state: &str, result: Option<&Value>, now: u64) -> Result<()> {
    app.db.checkpoint_cloud_job(job, state, result, Some(now + 2_000))
}

async fn step(app: &App, job: &mut Job, now: u64) -> Result<()> {
    let cloud = app.cloud.as_ref().context("cloud is disabled")?;
    let p = &cloud.provider;
    let stopped = job.cancelled || now >= job.run.deadline;
    if stopped && job.execution.outcome.is_none() {
        job.execution.outcome = Some(if job.cancelled { "cancelled" } else { "expired" }.into());
    }
    // Never discard an in-flight creation: reconcile it before cleaning up.
    if stopped && matches!(job.execution.stage.as_str(), "" | "environment_ready" | "agent_ready") {
        job.execution.stage = "cleanup_agent".into();
        return save(app, job, "cleaning_up", Some(&json!({"message":"Run stopped before execution"})), now);
    }
    let spec = &job.run.spec;
    match job.execution.stage.as_str() {
        "" => {
            job.execution.stage = "environment_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let root = if spec.runtime == "gemini" { "/tmp/gemini-workspace/repo" } else { "/home/sprite/repo" };
            let mut repository = json!({"url":spec.repository,"mount_path":root});
            if !spec.git_ref.is_empty() {
                repository["ref"] = json!(spec.git_ref);
            }
            if let Some(name) = &spec.repository_secret {
                repository["secret_key"] = json!(name);
            }
            let environment = p
                .create(
                    "environments",
                    json!({
                        "name":job.run.id,"metadata":{"arugula_run":job.run.id},
                        "repositories":[repository],
                        "setup_script":format!("git -C {root} update-ref refs/arugula/base HEAD"),
                    }),
                )
                .await?;
            job.execution.environment = Some(environment);
            job.execution.stage = "environment_ready".into();
            save(app, job, "provisioning", None, now)
        }
        "environment_creating" => {
            job.execution.environment = p.find("environments", &job.run.id).await?;
            if job.execution.environment.is_some() {
                job.execution.stage = "environment_ready".into();
            }
            save(app, job, "recovering", None, now)
        }
        "environment_ready" => {
            let secrets = app.db.cloud_run_secrets(&job.account, &job.run.id)?;
            if let Some((name, sealed)) =
                secrets.iter().find(|(name, _)| !job.execution.secrets_uploaded.contains(name))
            {
                let cipher = cloud.secret_cipher.as_ref().context("cloud secrets key is unavailable")?;
                let value = cipher.open(&job.account, name, sealed)?;
                let environment = job.execution.environment.as_deref().context("missing environment")?;
                p.query(
                    Method::POST,
                    &["api", "environments", environment, "secrets"],
                    &[],
                    Some(json!({"key":name,"value":value})),
                )
                .await?;
                job.execution.secrets_uploaded.push(name.clone());
                return save(app, job, "provisioning", None, now);
            }
            job.execution.stage = "agent_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let id = p.create("agents", json!({
                "name":job.run.id,"metadata":{"arugula_run":job.run.id},
                "environment_id":job.execution.environment,
                "allowed_environment_ids":[],"allowed_vault_ids":[],
                "runtime":spec.runtime,"model":spec.model,
                "system":format!("{}\nWork in the repository at repo/ under your working directory.",spec.agent.instructions),
                "mcp_servers":spec.agent.mcp_servers,"skills":spec.agent.skills,
                "permission_policy":{"default":"ask"},
            })).await?;
            job.execution.agent = Some(id);
            job.execution.stage = "agent_ready".into();
            save(app, job, "provisioning", None, now)
        }
        "agent_creating" => {
            job.execution.agent = p.find("agents", &job.run.id).await?;
            if job.execution.agent.is_some() {
                job.execution.stage = "agent_ready".into();
            }
            save(app, job, "recovering", None, now)
        }
        "agent_ready" => {
            job.execution.stage = "conversation_creating".into();
            app.db.checkpoint_cloud_job(job, "provisioning", None, None)?;
            let value = p
                .launch(
                    job.execution.agent.as_deref().context("missing agent")?,
                    job.execution.environment.as_deref().context("missing environment")?,
                    &job.run.id,
                    &spec.prompt,
                )
                .await?;
            job.execution.conversation = Some(value["id"].as_str().context("missing conversation")?.into());
            job.execution.stage = "running".into();
            save(app, job, "running", None, now)
        }
        "conversation_creating" => {
            job.execution.conversation = p.find("conversations", &job.run.id).await?;
            if job.execution.conversation.is_some() {
                job.execution.stage = "running".into();
            }
            save(app, job, "recovering", None, now)
        }
        "running" => {
            let id = job.execution.conversation.as_deref().context("missing conversation")?;
            let c = p.conversation(id).await?;
            let state = c["status"].as_str().context("missing conversation status")?;
            // Interrupting an already idle turn may return a conflict. Read
            // status first; a racing completion still proceeds to preservation.
            if stopped
                && matches!(state, "running" | "pending")
                && let Err(error) = p.interrupt(id).await
                && !error.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 409)
            {
                return Err(error);
            }
            let events = p.events(id, job.execution.cursor).await?;
            app.db.append_cloud_events(job, &events)?;
            // A failed turn can leave its reusable provider conversation idle.
            // Persist failure before draining subsequent event pages.
            if job.execution.outcome.is_none()
                && events.iter().any(|event| event["stage"] == "turn" && event["state"] == "failed")
            {
                job.execution.outcome = Some("failed".into());
            }
            if let Some(cursor) = events.iter().filter_map(|e| e["id"].as_u64()).max() {
                job.execution.cursor = cursor;
            }
            job.execution.sandbox = c["sandbox_id"].as_str().map(str::to_owned);
            if matches!(state, "idle" | "failed" | "terminated") && events.len() < 1000 {
                if job.execution.outcome.is_none() {
                    job.execution.outcome = Some(if state == "idle" { "completed" } else { "failed" }.into());
                }
                job.execution.stage = "collecting".into();
                save(app, job, "saving_results", Some(&json!({"usage":c["usage_total"]})), now)
            } else {
                let waiting = !app.db.cloud_requests(&job.account, &job.run.id)?.is_empty();
                save(app, job, if waiting { "needs_input" } else { "running" }, None, now)
            }
        }
        "collecting" => {
            let Some(sandbox) = job.execution.sandbox.as_deref() else {
                job.execution.stage = "terminating".into();
                return save(
                    app,
                    job,
                    "cleaning_up",
                    Some(
                        &json!({"message":"No sandbox was provisioned","usage":job.run.result.as_ref().map(|r| &r["usage"])}),
                    ),
                    now,
                );
            };
            let sandbox_state = p.query(Method::GET, &["api", "sandboxes", sandbox], &[], None).await?;
            match sandbox_state["status"].as_str() {
                Some("failed" | "terminated") => {
                    if job.execution.outcome.as_deref() == Some("completed") {
                        job.execution.outcome = Some("failed".into());
                    }
                    job.execution.stage = "terminating".into();
                    let mut result = job.run.result.clone().unwrap_or_else(|| json!({}));
                    result["message"] = json!(
                        "The execution environment became unavailable before files could be saved. The captured transcript is retained."
                    );
                    result["files_preserved"] = json!(false);
                    return save(app, job, "cleaning_up", Some(&result), now);
                }
                Some("suspended") => {
                    let conversation = job.execution.conversation.as_deref().context("missing conversation")?;
                    p.query(Method::POST, &["api", "conversations", conversation, "wake"], &[], None).await?;
                    return save(app, job, "saving_results", None, now);
                }
                Some("ready") if sandbox_state["transition"].is_null() => {}
                _ => return save(app, job, "saving_results", None, now),
            }
            let diff = p
                .query(
                    Method::GET,
                    &["api", "sandboxes", sandbox, "diff"],
                    &[("path", "repo"), ("ref", "refs/arugula/base"), ("max_bytes", "4194304")],
                    None,
                )
                .await?;
            let status = p
                .query(
                    Method::GET,
                    &["api", "sandboxes", sandbox, "git-status"],
                    &[("path", "repo"), ("untracked", "all")],
                    None,
                )
                .await?;
            // Retain the environment if preservation would lose data.
            if diff["truncated"] == true || status["truncated"] == true {
                bail!("cloud result exceeds preservation limit");
            }
            // The provider's textual diff omits binary bodies. Keep the
            // sandbox until an operator can recover those files.
            if diff["diff"].as_str().is_some_and(|text| {
                text.lines().any(|line| line.starts_with("Binary files ") && line.ends_with(" differ"))
            }) {
                bail!("binary changes require manual preservation");
            }
            let result = json!({"usage":job.run.result.as_ref().map(|r| &r["usage"]),"diff":diff,"git_status":status,"files":{}});
            job.execution.stage = "collecting_files".into();
            save(app, job, "saving_results", Some(&result), now)
        }
        "collecting_files" => {
            let sandbox = job.execution.sandbox.as_deref().context("missing sandbox")?;
            let mut result = job.run.result.clone().context("missing saved diff")?;
            let entries = result["git_status"]["entries"].as_array().context("missing git status")?;
            let next = entries
                .iter()
                .filter(|e| e["index"] == "untracked" || e["worktree"] == "untracked")
                .filter_map(|e| e["path"].as_str())
                .find(|path| result["files"].get(*path).is_none())
                .map(str::to_owned);
            if let Some(path) = next {
                let full = format!("repo/{path}");
                let file = p
                    .query(
                        Method::GET,
                        &["api", "sandboxes", sandbox, "file"],
                        &[("path", &full), ("max_bytes", "4194304")],
                        None,
                    )
                    .await?;
                if file["truncated"] == true {
                    bail!("cloud file exceeds preservation limit");
                }
                result["files"][&path] = file;
                save(app, job, "saving_results", Some(&result), now)
            } else {
                job.execution.stage = "terminating".into();
                save(app, job, "cleaning_up", Some(&result), now)
            }
        }
        "terminating" => {
            if let Some(id) = &job.execution.conversation {
                let c = match p.conversation(id).await {
                    Ok(c) => c,
                    Err(e) if e.downcast_ref::<super::provider::HttpFailure>().is_some_and(|e| e.0 == 404) => {
                        json!({"status":"terminated"})
                    }
                    Err(e) => return Err(e),
                };
                if c["status"] != "terminated" {
                    p.terminate(id).await?;
                    return save(app, job, "cleaning_up", None, now);
                }
            }
            job.execution.stage = "cleanup_agent".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_agent" => {
            if let Some(id) = &job.execution.agent {
                p.remove("agents", id).await?;
            }
            job.execution.agent = None;
            job.execution.stage = "cleanup_environment".into();
            save(app, job, "cleaning_up", None, now)
        }
        "cleanup_environment" => {
            if let Some(id) = &job.execution.environment {
                p.remove("environments", id).await?;
            }
            job.execution.environment = None;
            job.execution.stage = "done".into();
            let outcome = job.execution.outcome.as_deref().unwrap_or("failed");
            save(app, job, outcome, None, now)?;
            app.analytics.capture(
                &job.account,
                "cloud run finished",
                json!({
                    "outcome": outcome, "agent_curated": spec.agent_id.starts_with("curated_"),
                    "runtime": spec.runtime, "model": spec.model,
                    "minutes": now.saturating_sub(job.run.created) / 60_000,
                }),
            );
            Ok(())
        }
        _ => bail!("unknown cloud execution stage"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode, Uri},
        response::IntoResponse,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Vec<(String, String, Value)>,
        environment: Option<Value>,
        terminated: bool,
        lose_create_response: bool,
        deny_lookup_once: bool,
        failed_sandbox: bool,
        truncate_diff: bool,
        binary_diff: bool,
        failed_turn: bool,
    }

    async fn handle(
        State(fake): State<Arc<Mutex<Fake>>>,
        method: axum::http::Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> axum::response::Response {
        assert_eq!(headers["authorization"], "Bearer mock-key");
        let mut fake = fake.lock().unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        fake.calls.push((method.to_string(), uri.to_string(), value.clone()));
        let path = uri.path();
        let data = match (method.as_str(), path) {
            ("POST", "/api/environments") => {
                let mut environment = value.clone();
                environment["id"] = json!("env1");
                fake.environment = Some(environment);
                if fake.lose_create_response {
                    fake.lose_create_response = false;
                    return StatusCode::BAD_GATEWAY.into_response();
                }
                json!({"id":"env1"})
            }
            ("GET", "/api/environments") => {
                if fake.deny_lookup_once {
                    fake.deny_lookup_once = false;
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                json!([fake.environment])
            }
            ("POST", "/api/environments/env1/secrets") => json!({"key":value["key"]}),
            ("POST", "/api/agents") => json!({"id":"agent1"}),
            ("POST", "/api/conversations") => json!({"id":"conv1"}),
            ("GET", "/api/conversations/conv1/events") => {
                if uri.query().unwrap_or_default().contains("after=0") {
                    json!([{"id":1,"stage":"turn","state":if fake.failed_turn {"failed"} else {"done"},"blocks":[{"kind":"text","body":"Finished the change"}]}])
                } else {
                    json!([])
                }
            }
            ("GET", "/api/conversations/conv1") => {
                json!({"id":"conv1","status":if fake.terminated {"terminated"} else {"idle"},"sandbox_id":"sandbox1","usage_total":{"cost_usd":0.01}})
            }
            ("GET", "/api/sandboxes/sandbox1") => {
                json!({"status":if fake.failed_sandbox {"failed"} else {"ready"},"transition":null})
            }
            ("GET", "/api/sandboxes/sandbox1/diff") => {
                json!({"diff":if fake.binary_diff { "Binary files a/image.png and b/image.png differ" } else { "diff --git a/a b/a\n+fixed" },"truncated":fake.truncate_diff})
            }
            ("GET", "/api/sandboxes/sandbox1/git-status") => {
                json!({"entries":[{"path":"new.txt","index":"untracked","worktree":"untracked"}],"truncated":false})
            }
            ("GET", "/api/sandboxes/sandbox1/file") => {
                json!({"content":"new content","encoding":"utf-8","truncated":false})
            }
            ("POST", "/api/conversations/conv1/terminate") => {
                fake.terminated = true;
                json!({})
            }
            ("POST", "/api/conversations/conv1/interrupt") => return StatusCode::CONFLICT.into_response(),
            ("DELETE", _) => return StatusCode::NO_CONTENT.into_response(),
            _ => panic!("unexpected mock request: {method} {uri}"),
        };
        axum::Json(json!({"data":data})).into_response()
    }

    async fn exercise(scenario: Fake, cancel_at: Option<u64>, duration_ms: u64, expected: &str) {
        let failed_sandbox = scenario.failed_sandbox;
        let truncated = scenario.truncate_diff || scenario.binary_diff;
        let fake = Arc::new(Mutex::new(scenario));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(handle).with_state(fake.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut app = crate::cloud::tests::test_app();
        Arc::get_mut(&mut app).unwrap().cloud.as_mut().unwrap().provider =
            crate::cloud::provider::Provider::new(&base, "mock-key".into()).unwrap();
        let spec = crate::db::cloud::RunSpec {
            agent_id: "curated_coding".into(),
            agent_version: 1,
            agent: crate::cloud::curated_agents().remove(0).definition,
            model: "anthropic/test".into(),
            runtime: "claude".into(),
            repository: "https://github.com/example/repo".into(),
            git_ref: "main".into(),
            repository_secret: None,
            prompt: "Fix it".into(),
        };
        let limits = crate::db::cloud::RunLimits { daily_runs: 10, concurrent_runs: 2, duration_ms };
        let mut spec = spec;
        spec.repository_secret = Some("GITHUB_TOKEN".into());
        let cipher = app.cloud.as_ref().unwrap().secret_cipher.as_ref().unwrap();
        app.db
            .put_cloud_secret("a", "GITHUB_TOKEN", &cipher.seal("a", "GITHUB_TOKEN", "repo-secret").unwrap())
            .unwrap();
        let run = app.db.reserve_cloud_run("a", "request", &spec, &limits, 0).unwrap();
        for i in 0..30 {
            if cancel_at == Some(i) {
                app.db.cancel_cloud_run("a", &run.id).unwrap();
            }
            tick_at(&app, i * 11_000).await.unwrap();
        }
        let saved = app.db.cloud_run("a", &run.id).unwrap().unwrap();
        assert_eq!(saved.state, expected);
        if failed_sandbox {
            assert_eq!(saved.result.as_ref().unwrap()["files_preserved"], false);
            assert!(!fake.lock().unwrap().calls.iter().any(|(_, p, _)| p.contains("/diff?")));
        }
        if truncated {
            assert!(!fake.lock().unwrap().calls.iter().any(|(_, p, _)| p.ends_with("/terminate")));
        }
        if expected == "completed" {
            let result = saved.result.unwrap();
            assert_eq!(result["files"]["new.txt"]["content"], "new content");
            assert_eq!(app.db.cloud_events("a", &run.id, 0).unwrap().len(), 1);
            assert!(app.db.cloud_events("b", &run.id, 0).unwrap().is_empty());
            let fake = fake.lock().unwrap();
            let secret_at = fake.calls.iter().position(|(_, p, _)| p.ends_with("/secrets")).unwrap();
            let launch_at = fake.calls.iter().position(|(m, p, _)| m == "POST" && p == "/api/conversations").unwrap();
            assert!(secret_at < launch_at);
            assert_eq!(fake.calls[secret_at].2["value"], "repo-secret");
            let environment = fake.calls.iter().find(|(m, p, _)| m == "POST" && p == "/api/environments").unwrap();
            assert_eq!(environment.2["repositories"][0]["secret_key"], "GITHUB_TOKEN");
            let file_at = fake.calls.iter().position(|(_, p, _)| p.contains("/file?")).unwrap();
            let terminate_at = fake.calls.iter().position(|(_, p, _)| p.ends_with("/terminate")).unwrap();
            assert!(file_at < terminate_at);
            let launch = fake.calls.iter().find(|(m, p, _)| m == "POST" && p == "/api/conversations").unwrap();
            assert_eq!(launch.2["sandbox_api_access"], "none");
        }
        assert_eq!(
            fake.lock().unwrap().calls.iter().filter(|(m, p, _)| m == "POST" && p == "/api/environments").count(),
            1
        );
        server.abort();
    }

    #[tokio::test]
    async fn failed_turn_in_idle_conversation_is_not_success() {
        exercise(Fake { failed_turn: true, ..Default::default() }, None, 500_000, "failed").await;
    }

    /// Opt-in smoke test. Uses a real, dedicated provider account and durable
    /// SQLite state so an interrupted test can resume cleanup on the next run.
    #[tokio::test]
    #[ignore = "requires a dedicated funded provider account; creates a real sandbox"]
    async fn live_subscription_run() {
        let key = std::env::var("ARUGULA_MANAGOAT_API_KEY").expect("provider key required");
        let model = std::env::var("ARUGULA_CLOUD_MODEL").expect("model required");
        let runtime = std::env::var("ARUGULA_CLOUD_RUNTIME").expect("runtime required");
        let path = std::env::var("ARUGULA_LIVE_TEST_DB").expect("durable test database required");
        let mut app = crate::cloud::tests::test_app();
        let inner = Arc::get_mut(&mut app).unwrap();
        inner.db = crate::db::Db::open(std::path::Path::new(&path)).unwrap();
        inner.db.account_for("test", "live-smoke", "live-smoke", "live-smoke", 0).unwrap();
        inner.cloud.as_mut().unwrap().provider = crate::cloud::provider::Provider::new(
            &std::env::var("ARUGULA_MANAGOAT_URL").unwrap_or_else(|_| "https://managoat.com".into()),
            key,
        )
        .unwrap();
        let spec = crate::db::cloud::RunSpec {
            agent_id: "curated_coding".into(), agent_version: 1,
            agent: crate::cloud::curated_agents().remove(0).definition,
            model, runtime, repository: "https://github.com/octocat/Hello-World".into(),
            git_ref: "master".into(), repository_secret: None,
            prompt: "This is an isolated integration smoke test. In repo/, create arugula-smoke.txt containing exactly ARUGULA_LIVE_OK followed by a newline. Do not change any other file or contact any external service. Then reply ARUGULA_LIVE_OK.".into(),
        };
        let run = app
            .db
            .reserve_cloud_run(
                "live-smoke",
                "subscription-smoke-v1",
                &spec,
                &crate::db::cloud::RunLimits { daily_runs: 1, concurrent_runs: 1, duration_ms: 600_000 },
                arugula_e2e::now_ms(),
            )
            .unwrap();
        eprintln!("Live run {} uses durable database {}", run.id, path);
        let mut previous = String::new();
        for _ in 0..450 {
            tick(&app).await.unwrap();
            let saved = app.db.cloud_run("live-smoke", &run.id).unwrap().unwrap();
            if saved.state != previous {
                eprintln!("Live run state: {}", saved.state);
                // Never blindly approve arbitrary provider commands, even in
                // a disposable checkout. Inspect the request before answering.
                if saved.state == "needs_input" {
                    eprintln!("Approval pending; inspect the durable database before answering.");
                }
                previous = saved.state.clone();
            }
            if matches!(saved.state.as_str(), "completed" | "failed" | "cancelled" | "expired") {
                assert_eq!(saved.state, "completed", "live execution did not complete");
                let result = saved.result.unwrap();
                assert_eq!(result["files"]["arugula-smoke.txt"]["content"], "ARUGULA_LIVE_OK\n");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        panic!("live test observation timed out; resume with the same database to reconcile resources");
    }

    #[tokio::test]
    async fn binary_changes_keep_the_sandbox_for_recovery() {
        exercise(Fake { binary_diff: true, ..Default::default() }, None, 500_000, "recovering").await;
    }

    #[tokio::test]
    async fn saves_results_before_cleanup_without_a_connected_browser() {
        exercise(Fake::default(), None, 500_000, "completed").await;
    }
    #[tokio::test]
    async fn reconciles_lost_create_response_without_duplicate_environment() {
        exercise(Fake { lose_create_response: true, ..Default::default() }, None, 500_000, "completed").await;
    }
    #[tokio::test]
    async fn cancellation_is_durable_and_cleans_up() {
        exercise(Fake::default(), Some(3), 500_000, "cancelled").await;
    }
    #[tokio::test]
    async fn recovery_lookup_failure_does_not_discard_an_uncertain_creation() {
        exercise(
            Fake { lose_create_response: true, deny_lookup_once: true, ..Default::default() },
            None,
            500_000,
            "completed",
        )
        .await;
    }
    #[tokio::test]
    async fn cancelling_an_idle_turn_does_not_loop_on_interrupt_conflict() {
        exercise(Fake::default(), Some(4), 500_000, "cancelled").await;
    }
    #[tokio::test]
    async fn unavailable_sandbox_preserves_transcript_and_releases_capacity() {
        exercise(Fake { failed_sandbox: true, ..Default::default() }, None, 500_000, "failed").await;
    }
    #[tokio::test]
    async fn incomplete_diff_never_triggers_destructive_cleanup() {
        exercise(Fake { truncate_diff: true, ..Default::default() }, None, 500_000, "recovering").await;
    }
    #[tokio::test]
    async fn deadline_stops_a_run_without_marking_it_successful() {
        exercise(Fake::default(), None, 40_000, "expired").await;
    }
}
