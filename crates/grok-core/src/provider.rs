use std::pin::Pin;
use std::sync::Once;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{ApiKind, Provider};
use crate::types::Usage;

static TLS_INIT: Once = Once::new();

/// Install the pure-Rust crypto provider. No ring / aws-lc, so the whole tree
/// cross-compiles to Android without a C toolchain for crypto.
pub fn init_tls() {
    TLS_INIT.call_once(|| {
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });
}

pub fn http_client(timeout_secs: u64) -> Result<reqwest::Client> {
    init_tls();
    // Bundled Mozilla roots rather than the platform verifier: reqwest's default
    // rustls backend reaches for a Java certificate-verifier class on Android that
    // this app does not ship, and for schannel/Security.framework elsewhere. One
    // static root store behaves identically on phone, server and desktop.
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| std::sync::Arc::new(rustls_rustcrypto::provider()));
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("selecting TLS protocol versions")?
        .with_root_certificates(roots)
        .with_no_client_auth();

    // No total-request timeout: it would abort a long-lived SSE stream. Bound the
    // connect and the gap between reads instead.
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(timeout_secs.max(30)))
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent(concat!("grok-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building http client")
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Value::String(text.into()),
            ..Default::default()
        }
    }

    pub fn parts(role: &str, parts: Vec<Value>) -> Self {
        Self {
            role: role.into(),
            content: Value::Array(parts),
            ..Default::default()
        }
    }

    pub fn as_text(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Default)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub top_p: f32,
    pub tools: Vec<ToolSpec>,
    pub reasoning: bool,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    Text(String),
    Reasoning(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        args_delta: String,
    },
    Finish(String),
    Usage(Usage),
    Done,
}

pub type EventStream = Pin<Box<dyn Stream<Item = Result<ProviderEvent>> + Send>>;

fn auth_headers(p: &Provider, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut b = builder;
    match p.kind {
        ApiKind::OpenAi => {
            if !p.api_key.is_empty() {
                b = b.bearer_auth(&p.api_key);
            }
        }
        ApiKind::Anthropic => {
            b = b
                .header("x-api-key", &p.api_key)
                .header("anthropic-version", "2023-06-01");
        }
    }
    for (k, v) in &p.extra_headers {
        if !k.is_empty() {
            b = b.header(k.as_str(), v.as_str());
        }
    }
    b
}

fn openai_body(req: &LlmRequest, p: &Provider) -> Value {
    let mut body = json!({
        "model": req.model,
        "messages": req.messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "temperature": req.temperature,
        "max_tokens": req.max_tokens,
        "top_p": req.top_p,
    });
    if !req.tools.is_empty() && p.supports_tools {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect(),
        );
        body["tool_choice"] = json!("auto");
    }
    if req.reasoning {
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    body
}

fn anthropic_body(req: &LlmRequest) -> Value {
    let mut system: Vec<String> = Vec::new();
    let mut msgs: Vec<Value> = Vec::new();
    for m in &req.messages {
        if m.role == "system" {
            system.push(m.as_text());
            continue;
        }
        if m.role == "tool" {
            msgs.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.as_text(),
                }]
            }));
            continue;
        }
        let mut content: Vec<Value> = Vec::new();
        match &m.content {
            Value::String(s) if !s.is_empty() => content.push(json!({"type":"text","text":s})),
            Value::Array(parts) => {
                for part in parts {
                    if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                        content.push(json!({"type":"text","text":t}));
                    } else if let Some(url) = part
                        .get("image_url")
                        .and_then(|x| x.get("url"))
                        .and_then(|x| x.as_str())
                    {
                        if let Some((mime, data)) = split_data_url(url) {
                            content.push(json!({
                                "type":"image",
                                "source":{"type":"base64","media_type":mime,"data":data}
                            }));
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(calls) = &m.tool_calls {
            for c in calls {
                let f = c.get("function").cloned().unwrap_or_default();
                let args: Value = f
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({}));
                content.push(json!({
                    "type":"tool_use",
                    "id": c.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                    "name": f.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                    "input": args,
                }));
            }
        }
        if content.is_empty() {
            content.push(json!({"type":"text","text":" "}));
        }
        msgs.push(json!({ "role": m.role, "content": content }));
    }

    let mut body = json!({
        "model": req.model,
        "messages": msgs,
        "max_tokens": req.max_tokens.max(1),
        "stream": true,
        "temperature": req.temperature,
        "top_p": req.top_p,
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect(),
        );
    }
    if req.reasoning {
        body["thinking"] = json!({"type":"enabled","budget_tokens": 2048});
    }
    body
}

fn split_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.split(';').next()?.to_string();
    Some((mime, data.to_string()))
}

/// Byte-buffered SSE splitter. Decoding per-chunk would corrupt multi-byte
/// characters (Persian, emoji) that straddle a network chunk boundary.
fn sse_data(resp: reqwest::Response) -> impl Stream<Item = Result<String>> {
    async_stream::stream! {
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => { yield Err(anyhow!("stream read: {e}")); return; }
            };
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim_end_matches(['\r', '\n']);
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data.is_empty() { continue; }
                yield Ok(data.to_string());
            }
        }
        if !buf.is_empty() {
            let line = String::from_utf8_lossy(&buf);
            if let Some(data) = line.trim().strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() { yield Ok(data.to_string()); }
            }
        }
    }
}

fn parse_openai(v: &Value, out: &mut Vec<ProviderEvent>) {
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("provider error");
        out.push(ProviderEvent::Finish(format!("error: {msg}")));
        return;
    }
    if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
        out.push(ProviderEvent::Usage(Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            reasoning_tokens: u
                .get("completion_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        }));
    }
    let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else {
        return;
    };
    for choice in choices {
        if let Some(delta) = choice.get("delta") {
            if let Some(t) = delta.get("content").and_then(|x| x.as_str()) {
                if !t.is_empty() {
                    out.push(ProviderEvent::Text(t.to_string()));
                }
            }
            for key in ["reasoning_content", "reasoning", "thinking"] {
                if let Some(t) = delta.get(key).and_then(|x| x.as_str()) {
                    if !t.is_empty() {
                        out.push(ProviderEvent::Reasoning(t.to_string()));
                    }
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(|x| x.as_array()) {
                for c in calls {
                    let index = c.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    let f = c.get("function").cloned().unwrap_or_default();
                    out.push(ProviderEvent::ToolCallDelta {
                        index,
                        id: c.get("id").and_then(|x| x.as_str()).map(String::from),
                        name: f.get("name").and_then(|x| x.as_str()).map(String::from),
                        args_delta: f
                            .get("arguments")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").and_then(|x| x.as_str()) {
            out.push(ProviderEvent::Finish(finish.to_string()));
        }
    }
}

fn parse_anthropic(v: &Value, out: &mut Vec<ProviderEvent>) {
    let Some(kind) = v.get("type").and_then(|t| t.as_str()) else {
        return;
    };
    match kind {
        "content_block_start" => {
            if let Some(block) = v.get("content_block") {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    out.push(ProviderEvent::ToolCallDelta {
                        index: v.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                        id: block.get("id").and_then(|x| x.as_str()).map(String::from),
                        name: block.get("name").and_then(|x| x.as_str()).map(String::from),
                        args_delta: String::new(),
                    });
                }
            }
        }
        "content_block_delta" => {
            let Some(delta) = v.get("delta") else { return };
            match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text_delta" => {
                    if let Some(t) = delta.get("text").and_then(|x| x.as_str()) {
                        out.push(ProviderEvent::Text(t.to_string()));
                    }
                }
                "thinking_delta" => {
                    if let Some(t) = delta.get("thinking").and_then(|x| x.as_str()) {
                        out.push(ProviderEvent::Reasoning(t.to_string()));
                    }
                }
                "input_json_delta" => {
                    if let Some(t) = delta.get("partial_json").and_then(|x| x.as_str()) {
                        out.push(ProviderEvent::ToolCallDelta {
                            index: v.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                            id: None,
                            name: None,
                            args_delta: t.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(u) = v.get("usage") {
                out.push(ProviderEvent::Usage(Usage {
                    prompt_tokens: u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                    completion_tokens: u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                    reasoning_tokens: 0,
                    total_tokens: u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0)
                        + u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                }));
            }
            if let Some(stop) = v
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|x| x.as_str())
            {
                out.push(ProviderEvent::Finish(stop.to_string()));
            }
        }
        "message_stop" => out.push(ProviderEvent::Done),
        "error" => {
            let msg = v
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("anthropic stream error");
            out.push(ProviderEvent::Finish(format!("error: {msg}")));
        }
        _ => {}
    }
}

pub fn stream_chat(client: &reqwest::Client, p: &Provider, req: &LlmRequest) -> EventStream {
    let client = client.clone();
    let p = p.clone();
    let req = req.clone();
    Box::pin(async_stream::stream! {
        let endpoint = p.chat_endpoint();
        if endpoint.is_empty() || p.base_url.trim().is_empty() {
            yield Err(anyhow!("no base URL configured for provider '{}'", p.name));
            return;
        }
        let body = match p.kind {
            ApiKind::OpenAi => openai_body(&req, &p),
            ApiKind::Anthropic => anthropic_body(&req),
        };
        let builder = client.post(&endpoint).header("content-type", "application/json")
            .header("accept", "text/event-stream");
        let builder = auth_headers(&p, builder);
        let resp = match builder.json(&body).send().await {
            Ok(r) => r,
            Err(e) => { yield Err(anyhow!("request to {endpoint} failed: {e}")); return; }
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let snippet: String = text.chars().take(600).collect();
            if status.as_u16() == 429 {
                yield Err(anyhow!("rate limited (429) by {}: {snippet}", p.name));
            } else {
                yield Err(anyhow!("{} returned {status}: {snippet}", p.name));
            }
            return;
        }
        let mut events = Box::pin(sse_data(resp));
        let mut finished = false;
        while let Some(item) = events.next().await {
            let data = match item { Ok(d) => d, Err(e) => { yield Err(e); return; } };
            if data == "[DONE]" { finished = true; break; }
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            let mut parsed = Vec::new();
            match p.kind {
                ApiKind::OpenAi => parse_openai(&v, &mut parsed),
                ApiKind::Anthropic => parse_anthropic(&v, &mut parsed),
            }
            for ev in parsed {
                if matches!(ev, ProviderEvent::Done) { finished = true; }
                yield Ok(ev);
            }
        }
        if !finished {
            // The official client treats a missing terminal marker as a recoverable
            // stream error rather than silently succeeding. Surface it the same way.
            yield Ok(ProviderEvent::Finish("stream_closed_without_marker".into()));
        }
        yield Ok(ProviderEvent::Done);
    })
}

pub async fn list_models(client: &reqwest::Client, p: &Provider) -> Result<Vec<String>> {
    let endpoint = p.models_endpoint();
    let builder = client.get(&endpoint);
    let builder = auth_headers(p, builder);
    let resp = builder.send().await.context("listing models")?;
    if !resp.status().is_success() {
        return Err(anyhow!("models endpoint returned {}", resp.status()));
    }
    let v: Value = resp.json().await.context("decoding models")?;
    let mut out: Vec<String> = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for m in arr {
            if let Some(id) = m.get("id").and_then(|x| x.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(arr) = v.as_array() {
        for m in arr {
            if let Some(id) = m.as_str() {
                out.push(id.to_string());
            } else if let Some(id) = m.get("name").and_then(|x| x.as_str()) {
                out.push(id.trim_start_matches("models/").to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub async fn generate_image(
    client: &reqwest::Client,
    p: &Provider,
    prompt: &str,
    count: u32,
) -> Result<Vec<String>> {
    let body = json!({
        "model": "grok-2-image",
        "prompt": prompt,
        "n": count.clamp(1, 4),
        "response_format": "url",
    });
    let builder = client.post(p.images_endpoint()).json(&body);
    let builder = auth_headers(p, builder);
    let resp = builder.send().await.context("image generation request")?;
    if !resp.status().is_success() {
        return Err(anyhow!("image endpoint returned {}", resp.status()));
    }
    let v: Value = resp.json().await?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(u) = item.get("url").and_then(|x| x.as_str()) {
                out.push(u.to_string());
            } else if let Some(b) = item.get("b64_json").and_then(|x| x.as_str()) {
                out.push(format!("data:image/png;base64,{b}"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_delta_and_reasoning_are_separate_channels() {
        let v = json!({"choices":[{"delta":{"content":"hi","reasoning_content":"think"}}]});
        let mut out = Vec::new();
        parse_openai(&v, &mut out);
        assert!(matches!(out[0], ProviderEvent::Text(ref t) if t == "hi"));
        assert!(matches!(out[1], ProviderEvent::Reasoning(ref t) if t == "think"));
    }

    #[test]
    fn openai_tool_call_deltas_carry_index() {
        let v = json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_1","function":{"name":"web_search","arguments":"{\"q\":"}},
            {"index":0,"function":{"arguments":"\"rust\"}"}}
        ]}}]});
        let mut out = Vec::new();
        parse_openai(&v, &mut out);
        assert_eq!(out.len(), 2);
        match &out[0] {
            ProviderEvent::ToolCallDelta {
                index,
                name,
                args_delta,
                ..
            } => {
                assert_eq!(*index, 0);
                assert_eq!(name.as_deref(), Some("web_search"));
                assert_eq!(args_delta, "{\"q\":");
            }
            _ => panic!("expected tool delta"),
        }
    }

    #[test]
    fn anthropic_thinking_and_text_are_distinct() {
        let mut out = Vec::new();
        parse_anthropic(
            &json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hmm"}}),
            &mut out,
        );
        parse_anthropic(
            &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"answer"}}),
            &mut out,
        );
        assert!(matches!(out[0], ProviderEvent::Reasoning(_)));
        assert!(matches!(out[1], ProviderEvent::Text(_)));
    }

    #[test]
    fn anthropic_tool_use_start_and_json_delta() {
        let mut out = Vec::new();
        parse_anthropic(
            &json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"calculator"}}),
            &mut out,
        );
        parse_anthropic(
            &json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"expr\":"}}),
            &mut out,
        );
        assert_eq!(out.len(), 2);
        assert!(matches!(
            &out[0],
            ProviderEvent::ToolCallDelta { index: 1, .. }
        ));
    }

    #[test]
    fn usage_is_extracted_from_openai_tail() {
        let v = json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,
            "completion_tokens_details":{"reasoning_tokens":3}}});
        let mut out = Vec::new();
        parse_openai(&v, &mut out);
        match &out[0] {
            ProviderEvent::Usage(u) => {
                assert_eq!(u.prompt_tokens, 10);
                assert_eq!(u.reasoning_tokens, 3);
                assert_eq!(u.total_tokens, 15);
            }
            _ => panic!("expected usage"),
        }
    }

    #[test]
    fn data_url_split() {
        let (mime, data) = split_data_url("data:image/png;base64,AAAA").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(data, "AAAA");
        assert!(split_data_url("https://x/y.png").is_none());
    }

    #[test]
    fn anthropic_body_lifts_system_and_converts_tool_results() {
        let req = LlmRequest {
            messages: vec![
                ChatMessage::text("system", "be brief"),
                ChatMessage::text("user", "hi"),
                ChatMessage {
                    role: "tool".into(),
                    content: json!("42"),
                    tool_call_id: Some("tu_1".into()),
                    ..Default::default()
                },
            ],
            model: "claude".into(),
            max_tokens: 100,
            ..Default::default()
        };
        let body = anthropic_body(&req);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "tu_1");
    }

    #[test]
    fn openai_body_omits_tools_when_unsupported() {
        let mut p = Provider::new("local", ApiKind::OpenAi, "http://127.0.0.1:11434/v1", "m");
        p.supports_tools = false;
        let req = LlmRequest {
            tools: vec![ToolSpec {
                name: "t".into(),
                description: "d".into(),
                parameters: json!({}),
            }],
            ..Default::default()
        };
        let body = openai_body(&req, &p);
        assert!(body.get("tools").is_none());
    }
}
