use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

use grok_core::{ChatRequest, Engine, StreamEvent};

mod ui;

pub struct ServerHandle {
    pub local_addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl ServerHandle {
    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-grok-token")
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        })
}

/// Loopback callers are the on-device app itself and are trusted. Anything arriving
/// over the LAN must present the host token.
async fn auth(
    State(engine): State<Engine>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") || path == "/api/health" {
        return next.run(req).await;
    }
    if addr.ip().is_loopback() {
        return next.run(req).await;
    }
    let expected = engine.settings().host.token;
    match bearer(&headers) {
        Some(got)
            if !expected.is_empty() && constant_time_eq(got.as_bytes(), expected.as_bytes()) =>
        {
            next.run(req).await
        }
        _ => err(StatusCode::UNAUTHORIZED, "missing or invalid host token"),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn router(engine: Engine) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(ui::index))
        .route("/api/health", get(health))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/presets", get(presets))
        .route("/api/models", get(models))
        .route("/api/tools", get(tool_list))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        // POST is an alias for PATCH: Android's HttpURLConnection rejects the PATCH
        // method outright, and the phone shell uses that transport.
        .route(
            "/api/conversations/{id}",
            get(get_conversation)
                .patch(patch_conversation)
                .post(patch_conversation)
                .delete(delete_conversation),
        )
        .route("/api/conversations/{id}/leaf", post(set_leaf))
        .route("/api/conversations/{id}/export", get(export_conversation))
        .route("/api/chat", post(chat))
        .route("/api/stop", post(stop))
        .route("/api/approvals", get(approvals))
        .route("/api/approvals/{id}", post(resolve_approval))
        .route("/api/memories", get(list_memories).post(add_memory))
        .route("/api/memories/{id}", axum::routing::delete(delete_memory))
        .route("/api/images", post(images))
        .route("/api/search", get(search))
        .route("/api/host", get(host_info).post(host_update))
        .layer(middleware::from_fn_with_state(engine.clone(), auth))
        .layer(cors)
        .with_state(engine)
}

async fn health(State(engine): State<Engine>) -> Json<Value> {
    let s = engine.settings();
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "allow_lan": s.host.allow_lan,
        "provider_count": s.providers.len(),
        "active_model": s.active().map(|p| p.model.clone()).unwrap_or_default(),
    }))
}

async fn get_settings(State(engine): State<Engine>) -> Json<Value> {
    Json(json!(engine.settings().redacted()))
}

#[derive(Deserialize)]
struct SettingsPatch {
    #[serde(flatten)]
    value: Value,
}

async fn put_settings(State(engine): State<Engine>, Json(body): Json<SettingsPatch>) -> Response {
    let current = engine.settings();
    let mut next: grok_core::Settings = match serde_json::from_value(body.value.clone()) {
        Ok(s) => s,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("invalid settings: {e}")),
    };
    // The GET redacts secrets; preserve stored keys when the client echoes them back.
    for p in next.providers.iter_mut() {
        if p.api_key.starts_with("••••") || p.api_key.is_empty() {
            if let Some(existing) = current.providers.iter().find(|c| c.id == p.id) {
                if p.api_key.starts_with("••••") {
                    p.api_key = existing.api_key.clone();
                }
            }
        }
    }
    if next.search.api_key.starts_with("••••") {
        next.search.api_key = current.search.api_key.clone();
    }
    if next.host.token.is_empty() {
        next.host.token = current.host.token.clone();
    }
    match engine.update_settings(next) {
        Ok(_) => {
            Json(json!({ "ok": true, "settings": engine.settings().redacted() })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn presets() -> Json<Value> {
    let list: Vec<Value> = grok_core::config::presets().into_iter().map(|(name, kind, url, model)| {
        json!({"name":name,"kind":kind,"base_url":url,"model":model})
    }).collect();
    Json(json!({ "presets": list, "personas": grok_core::config::default_personas() }))
}

#[derive(Deserialize)]
struct ModelsQuery {
    provider_id: Option<String>,
}

async fn models(State(engine): State<Engine>, Query(q): Query<ModelsQuery>) -> Response {
    match engine.list_models(q.provider_id.as_deref()).await {
        Ok(m) => Json(json!({ "models": m })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn tool_list(State(engine): State<Engine>) -> Json<Value> {
    let s = engine.settings();
    let tools: Vec<Value> = grok_core::tools::registry()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "risk": t.risk.as_str(),
                "shell_only": t.shell_only,
                "enabled": s.enabled_tools.iter().any(|e| e == t.name),
                "available": !t.shell_only || s.host.allow_shell,
            })
        })
        .collect();
    Json(json!({ "tools": tools, "approval_mode": s.approval_mode }))
}

async fn list_conversations(State(engine): State<Engine>) -> Response {
    match engine.conversations() {
        Ok(c) => Json(json!({ "conversations": c })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct CreateConv {
    title: Option<String>,
    #[serde(default)]
    temporary: bool,
}

async fn create_conversation(
    State(engine): State<Engine>,
    Json(body): Json<CreateConv>,
) -> Response {
    match engine.create_conversation(body.title.as_deref(), body.temporary) {
        Ok(c) => Json(json!(c)).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_conversation(State(engine): State<Engine>, Path(id): Path<String>) -> Response {
    match (engine.conversation(&id), engine.messages(&id)) {
        (Ok(Some(c)), Ok(m)) => Json(json!({ "conversation": c, "messages": m })).into_response(),
        (Ok(None), _) => err(StatusCode::NOT_FOUND, "conversation not found"),
        (Err(e), _) | (_, Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct PatchConv {
    title: Option<String>,
    starred: Option<bool>,
}

async fn patch_conversation(
    State(engine): State<Engine>,
    Path(id): Path<String>,
    Json(body): Json<PatchConv>,
) -> Response {
    match engine.patch_conversation(&id, body.title, body.starred) {
        Ok(c) => Json(json!(c)).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e.to_string()),
    }
}

async fn delete_conversation(State(engine): State<Engine>, Path(id): Path<String>) -> Response {
    match engine.delete_conversation(&id) {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct LeafBody {
    leaf_id: String,
}

async fn set_leaf(
    State(engine): State<Engine>,
    Path(id): Path<String>,
    Json(body): Json<LeafBody>,
) -> Response {
    match engine.set_leaf(&id, &body.leaf_id) {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn export_conversation(State(engine): State<Engine>, Path(id): Path<String>) -> Response {
    match engine.export(&id) {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn chat(State(engine): State<Engine>, Json(req): Json<ChatRequest>) -> Response {
    let stream = engine.chat(req).map(|ev: StreamEvent| {
        Ok::<_, std::convert::Infallible>(
            Event::default()
                .json_data(ev)
                .unwrap_or_else(|_| Event::default().comment("encode-error")),
        )
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Deserialize)]
struct StopBody {
    response_id: String,
}

async fn stop(State(engine): State<Engine>, Json(body): Json<StopBody>) -> Json<Value> {
    Json(json!({ "stopped": engine.stop(&body.response_id).await }))
}

async fn approvals(State(engine): State<Engine>) -> Json<Value> {
    Json(json!({ "pending": engine.approvals.pending().await }))
}

#[derive(Deserialize)]
struct ApprovalBody {
    approved: bool,
}

async fn resolve_approval(
    State(engine): State<Engine>,
    Path(id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> Json<Value> {
    Json(json!({ "resolved": engine.approvals.resolve(&id, body.approved).await }))
}

async fn list_memories(State(engine): State<Engine>) -> Response {
    match engine.memories() {
        Ok(m) => Json(json!({ "memories": m })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct MemoryBody {
    text: String,
}

async fn add_memory(State(engine): State<Engine>, Json(body): Json<MemoryBody>) -> Response {
    match engine.add_memory(&body.text) {
        Ok(m) => Json(json!(m)).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_memory(State(engine): State<Engine>, Path(id): Path<String>) -> Response {
    match engine.delete_memory(&id) {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ImageBody {
    prompt: String,
    #[serde(default = "two")]
    count: u32,
}
fn two() -> u32 {
    2
}

async fn images(State(engine): State<Engine>, Json(body): Json<ImageBody>) -> Response {
    match engine.generate_image(&body.prompt, body.count).await {
        Ok(urls) => Json(json!({ "images": urls })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(State(engine): State<Engine>, Query(q): Query<SearchQuery>) -> Response {
    match engine.store.search_messages(&q.q) {
        Ok(m) => Json(json!({ "messages": m })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn host_info(
    State(engine): State<Engine>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Json<Value> {
    let s = engine.settings();
    Json(json!({
        "allow_lan": s.host.allow_lan,
        "port": s.host.port,
        "token": s.host.token,
        "bound": addr.to_string(),
        "allow_shell": s.host.allow_shell,
        "workspace": engine.workspace().display().to_string(),
    }))
}

#[derive(Deserialize)]
struct HostUpdate {
    allow_lan: Option<bool>,
    port: Option<u16>,
    allow_shell: Option<bool>,
}

async fn host_update(State(engine): State<Engine>, Json(body): Json<HostUpdate>) -> Response {
    let mut s = engine.settings();
    if let Some(v) = body.allow_lan {
        s.host.allow_lan = v;
    }
    if let Some(v) = body.port {
        s.host.port = v;
    }
    if let Some(v) = body.allow_shell {
        s.host.allow_shell = v;
    }
    match engine.update_settings(s) {
        Ok(_) => Json(json!({
            "ok": true,
            "restart_required": body.port.is_some() || body.allow_lan.is_some(),
            "host": engine.settings().host,
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Bind and serve. `port` 0 asks the OS for a free port, which is what the Android
/// shell uses so it never collides with another app.
pub async fn serve(engine: Engine, bind: &str, port: u16) -> Result<ServerHandle> {
    let addr: SocketAddr = format!("{bind}:{port}")
        .parse()
        .with_context(|| format!("parsing bind address {bind}:{port}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    let local_addr = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let app = router(engine).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        let shutdown = async move {
            while rx.changed().await.is_ok() {
                if *rx.borrow() {
                    break;
                }
            }
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
        {
            tracing::error!("server error: {e}");
        }
    });
    Ok(ServerHandle {
        local_addr,
        shutdown: tx,
    })
}

pub fn bind_address(settings: &grok_core::Settings) -> &'static str {
    if settings.host.allow_lan {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    }
}

pub type SharedEngine = Arc<Engine>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare_rejects_length_and_content_mismatch() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn bind_address_follows_lan_setting() {
        let mut s = grok_core::Settings::default();
        assert_eq!(bind_address(&s), "127.0.0.1");
        s.host.allow_lan = true;
        assert_eq!(bind_address(&s), "0.0.0.0");
    }

    #[tokio::test]
    async fn health_and_settings_endpoints_work_over_loopback() {
        let dir = std::env::temp_dir().join(format!("grok-srv-{}", grok_core::types::new_id()));
        let engine = Engine::open(&dir).unwrap();
        let handle = serve(engine.clone(), "127.0.0.1", 0).await.unwrap();
        let base = format!("http://{}", handle.local_addr);
        let client = reqwest::Client::new();

        let health: Value = client
            .get(format!("{base}/api/health"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["ok"], true);

        let settings: Value = client
            .get(format!("{base}/api/settings"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!settings["providers"].as_array().unwrap().is_empty());
        assert!(settings["host"]["token"].as_str().unwrap().len() > 10);

        let conv: Value = client
            .post(format!("{base}/api/conversations"))
            .json(&json!({"title":"t"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = conv["id"].as_str().unwrap().to_string();
        let list: Value = client
            .get(format!("{base}/api/conversations"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(list["conversations"].as_array().unwrap().len(), 1);

        let status = client
            .delete(format!("{base}/api/conversations/{id}"))
            .send()
            .await
            .unwrap()
            .status();
        assert!(status.is_success());

        handle.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn redacted_key_echo_does_not_wipe_the_stored_secret() {
        let dir = std::env::temp_dir().join(format!("grok-srv-{}", grok_core::types::new_id()));
        let engine = Engine::open(&dir).unwrap();
        let mut s = engine.settings();
        s.providers[0].api_key = "super-secret-key-123456".into();
        engine.update_settings(s).unwrap();

        let handle = serve(engine.clone(), "127.0.0.1", 0).await.unwrap();
        let base = format!("http://{}", handle.local_addr);
        let client = reqwest::Client::new();
        let mut redacted: Value = client
            .get(format!("{base}/api/settings"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!redacted.to_string().contains("super-secret"));
        redacted["language"] = json!("fa");
        let _: Value = client
            .put(format!("{base}/api/settings"))
            .json(&redacted)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            engine.settings().providers[0].api_key,
            "super-secret-key-123456"
        );
        assert_eq!(engine.settings().language, "fa");

        handle.stop();
        let _ = std::fs::remove_dir_all(dir);
    }
}
