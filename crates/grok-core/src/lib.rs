pub mod agent;
pub mod config;
pub mod provider;
pub mod store;
pub mod tools;
pub mod types;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use futures_util::{Stream, StreamExt};
use serde_json::Value;

pub use agent::ApprovalRegistry;
pub use config::{ApiKind, ApprovalMode, Provider, Settings};
pub use store::Store;
pub use types::{
    Attachment, ChatRequest, Conversation, Memory, Message, SearchResult, Sender, StreamEvent,
    ToolCall, ToolResult, Usage,
};

use agent::{build_history, system_prompt, ToolCallAssembler};
use provider::{ChatMessage, LlmRequest, ProviderEvent};
use tools::{ToolCtx, ToolOutcome};

type CancelMap = Arc<tokio::sync::Mutex<HashMap<String, Arc<AtomicBool>>>>;

#[derive(Clone)]
pub struct Engine {
    pub store: Store,
    settings: Arc<RwLock<Settings>>,
    http: reqwest::Client,
    pub approvals: ApprovalRegistry,
    workspace: PathBuf,
    cancels: CancelMap,
}

impl Engine {
    pub fn open(data_dir: &Path) -> Result<Self> {
        provider::init_tls();
        let store = Store::open(data_dir)?;
        let mut settings = store.settings()?;
        settings.ensure_token();
        store.save_settings(&settings)?;
        let workspace = if settings.host.workspace_dir.is_empty() {
            data_dir.join("workspace")
        } else {
            PathBuf::from(&settings.host.workspace_dir)
        };
        std::fs::create_dir_all(&workspace)?;
        let http = provider::http_client(
            settings
                .providers
                .iter()
                .map(|p| p.timeout_secs)
                .max()
                .unwrap_or(300),
        )?;
        Ok(Self {
            store,
            settings: Arc::new(RwLock::new(settings)),
            http,
            approvals: ApprovalRegistry::new(),
            workspace,
            cancels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn update_settings(&self, mut next: Settings) -> Result<()> {
        next.ensure_token();
        self.store.save_settings(&next)?;
        if let Ok(mut guard) = self.settings.write() {
            *guard = next;
        }
        Ok(())
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn data_dir(&self) -> PathBuf {
        self.workspace
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    }

    pub async fn list_models(&self, provider_id: Option<&str>) -> Result<Vec<String>> {
        let settings = self.settings();
        let p = settings
            .provider(provider_id)
            .ok_or_else(|| anyhow!("no provider configured"))?;
        provider::list_models(&self.http, p).await
    }

    pub fn conversations(&self) -> Result<Vec<Conversation>> {
        self.store.conversations()
    }

    pub fn conversation(&self, id: &str) -> Result<Option<Conversation>> {
        self.store.conversation(id)
    }

    pub fn create_conversation(
        &self,
        title: Option<&str>,
        temporary: bool,
    ) -> Result<Conversation> {
        let settings = self.settings();
        let c = Conversation {
            id: types::new_id(),
            title: title.unwrap_or("New chat").to_string(),
            created_at: types::now_ms(),
            modified_at: types::now_ms(),
            temporary,
            provider_id: Some(settings.active_provider.clone()),
            model: settings.active().map(|p| p.model.clone()),
            ..Default::default()
        };
        if !temporary {
            self.store.put_conversation(&c)?;
        }
        Ok(c)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        self.store.delete_conversation(id)
    }

    pub fn patch_conversation(
        &self,
        id: &str,
        title: Option<String>,
        starred: Option<bool>,
    ) -> Result<Conversation> {
        let mut c = self
            .store
            .conversation(id)?
            .ok_or_else(|| anyhow!("conversation not found"))?;
        if let Some(t) = title {
            c.title = t;
        }
        if let Some(s) = starred {
            c.starred = s;
        }
        c.modified_at = types::now_ms();
        self.store.put_conversation(&c)?;
        Ok(c)
    }

    /// The visible branch: leaf-to-root, chronological. Sibling branches stay stored.
    pub fn messages(&self, conv: &str) -> Result<Vec<Message>> {
        let all = self.store.messages(conv)?;
        let leaf = self.store.conversation(conv)?.and_then(|c| c.leaf_id);
        Ok(types::branch_to(&all, leaf.as_deref()))
    }

    pub fn all_messages(&self, conv: &str) -> Result<Vec<Message>> {
        self.store.messages(conv)
    }

    pub fn set_leaf(&self, conv: &str, leaf: &str) -> Result<()> {
        if let Some(mut c) = self.store.conversation(conv)? {
            c.leaf_id = Some(leaf.to_string());
            self.store.put_conversation(&c)?;
        }
        Ok(())
    }

    pub fn memories(&self) -> Result<Vec<Memory>> {
        self.store.memories()
    }

    pub fn add_memory(&self, text: &str) -> Result<Memory> {
        let m = Memory {
            id: types::new_id(),
            text: text.to_string(),
            created_at: types::now_ms(),
            source_conversation: None,
        };
        self.store.put_memory(&m)?;
        Ok(m)
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.store.delete_memory(id)
    }

    pub fn export(&self, conv: &str) -> Result<Value> {
        self.store.export_conversation(conv)
    }

    pub async fn stop(&self, response_id: &str) -> bool {
        let guard = self.cancels.lock().await;
        match guard.get(response_id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub async fn generate_image(&self, prompt: &str, count: u32) -> Result<Vec<String>> {
        let settings = self.settings();
        let p = settings
            .active()
            .ok_or_else(|| anyhow!("no provider configured"))?;
        provider::generate_image(&self.http, p, prompt, count).await
    }

    pub fn chat(&self, req: ChatRequest) -> impl Stream<Item = StreamEvent> + Send {
        let engine = self.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        Box::pin(async_stream::stream! {
            let settings = engine.settings();
            let provider = match settings.provider(req.provider_id.as_deref()) {
                Some(p) => p.clone(),
                None => {
                    yield StreamEvent::Error { message: "No provider configured. Add a base URL, API key and model in Settings.".into() };
                    return;
                }
            };
            let model = req.model.clone().unwrap_or_else(|| provider.model.clone());
            if model.trim().is_empty() {
                yield StreamEvent::Error { message: "No model selected.".into() };
                return;
            }

            // Resolve or create the conversation.
            let mut conv = match &req.conversation_id {
                Some(id) => match engine.store.conversation(id) {
                    Ok(Some(c)) => c,
                    Ok(None) => engine.create_conversation(None, req.temporary).unwrap_or_default(),
                    Err(e) => { yield StreamEvent::Error { message: e.to_string() }; return; }
                },
                None => match engine.create_conversation(None, req.temporary) {
                    Ok(c) => c,
                    Err(e) => { yield StreamEvent::Error { message: e.to_string() }; return; }
                },
            };
            if req.temporary { conv.temporary = true; }
            let persist = !conv.temporary && settings.save_history;
            let conv_id = conv.id.clone();

            let response_id = types::new_id();
            engine.cancels.lock().await.insert(response_id.clone(), cancel.clone());

            // Parent selection: explicit branch point, else the current leaf.
            let parent_id = req.parent_id.clone().or_else(|| conv.leaf_id.clone());

            let user_msg_id = if req.regenerate {
                // Regenerating reuses the existing user turn and creates a sibling answer.
                parent_id.clone().unwrap_or_default()
            } else {
                let id = types::new_id();
                let user = Message {
                    id: id.clone(),
                    conversation_id: conv_id.clone(),
                    parent_id: parent_id.clone(),
                    sender: Sender::User,
                    text: req.message.clone(),
                    created_at: types::now_ms(),
                    attachments: req.attachments.clone(),
                    ..Default::default()
                };
                if persist { let _ = engine.store.put_message(&user); }
                id
            };

            let mut assistant = Message {
                id: response_id.clone(),
                conversation_id: conv_id.clone(),
                parent_id: if user_msg_id.is_empty() { None } else { Some(user_msg_id.clone()) },
                sender: Sender::Assistant,
                created_at: types::now_ms(),
                partial: true,
                model: Some(model.clone()),
                ..Default::default()
            };
            // Persist before the first token so a dropped stream is resumable by id.
            if persist { let _ = engine.store.put_message(&assistant); }

            yield StreamEvent::Start {
                conversation_id: conv_id.clone(),
                response_id: response_id.clone(),
                model: model.clone(),
            };

            let persona_prompt = req.persona.as_deref()
                .and_then(|name| settings.personas.iter().find(|p| p.id == name || p.name == name))
                .map(|p| p.prompt.clone());
            let memories: Vec<String> = engine.store.memories().unwrap_or_default()
                .into_iter().take(30).map(|m| m.text).collect();
            let system = system_prompt(&settings, persona_prompt.as_deref(), &memories);

            let mut enabled_tools = settings.enabled_tools.clone();
            if req.search && !enabled_tools.iter().any(|t| t == "web_search") {
                enabled_tools.push("web_search".into());
            }
            let agent_on = req.agent || settings.agent_enabled;
            let tool_specs = if agent_on {
                tools::specs(&enabled_tools, settings.host.allow_shell)
            } else { Vec::new() };

            let prior = if persist || !conv.temporary {
                engine.store.messages(&conv_id).unwrap_or_default()
            } else { Vec::new() };
            let branch = types::branch_to(&prior, if user_msg_id.is_empty() { None } else { Some(user_msg_id.as_str()) });
            let mut history: Vec<ChatMessage> = build_history(&system, &branch, provider.supports_vision);
            if req.search {
                history.push(ChatMessage::text("system", "Web search is available. Use the web_search tool for current information and cite the URLs you rely on."));
            }

            if req.image_generation {
                yield StreamEvent::Step { step: types::AgentStep {
                    index: 0, kind: "image".into(), title: "Generating images".into(),
                    detail: req.message.chars().take(120).collect(), status: types::StepStatus::Running,
                }};
                match provider::generate_image(&engine.http, &provider, &req.message, 2).await {
                    Ok(urls) if !urls.is_empty() => {
                        assistant.generated_images = urls.clone();
                        yield StreamEvent::Images { urls };
                    }
                    Ok(_) => yield StreamEvent::Error { message: "provider returned no images".into() },
                    Err(e) => yield StreamEvent::Error { message: format!("image generation unavailable on this provider: {e}") },
                }
            }

            let ctx = ToolCtx {
                http: engine.http.clone(),
                store: engine.store.clone(),
                settings: settings.clone(),
                workspace: engine.workspace.clone(),
            };

            let max_steps = if agent_on { settings.agent_max_steps.clamp(1, 25) } else { 1 };
            let mut usage_total = Usage::default();
            let mut step_index = 0u32;
            let mut last_save = Instant::now();
            let mut failed: Option<String> = None;

            'steps: for _ in 0..max_steps {
                if cancel.load(Ordering::SeqCst) { break 'steps; }

                let llm = LlmRequest {
                    messages: history.clone(),
                    model: model.clone(),
                    temperature: req.temperature.unwrap_or(settings.temperature),
                    max_tokens: req.max_tokens.unwrap_or(settings.max_tokens),
                    top_p: settings.top_p,
                    tools: tool_specs.clone(),
                    reasoning: req.reasoning || provider.supports_reasoning,
                    reasoning_effort: provider.reasoning_effort.clone(),
                };

                let mut stream = provider::stream_chat(&engine.http, &provider, &llm);
                let mut assembler = ToolCallAssembler::new();
                let mut turn_text = String::new();
                let mut turn_reasoning = String::new();

                while let Some(item) = stream.next().await {
                    if cancel.load(Ordering::SeqCst) { break; }
                    match item {
                        Ok(ProviderEvent::Text(t)) => {
                            turn_text.push_str(&t);
                            assistant.text.push_str(&t);
                            yield StreamEvent::Delta { text: t };
                        }
                        Ok(ProviderEvent::Reasoning(t)) => {
                            turn_reasoning.push_str(&t);
                            assistant.reasoning.push_str(&t);
                            yield StreamEvent::Reasoning { text: t };
                        }
                        Ok(ev @ ProviderEvent::ToolCallDelta { .. }) => assembler.push(&ev),
                        Ok(ProviderEvent::Usage(u)) => {
                            usage_total.accumulate(&u);
                            assistant.usage = usage_total.clone();
                            yield StreamEvent::Usage { usage: u };
                        }
                        Ok(ProviderEvent::Finish(reason)) => {
                            if let Some(err) = reason.strip_prefix("error: ") {
                                failed = Some(err.to_string());
                            } else if reason == "stream_closed_without_marker" {
                                tracing::warn!("provider closed the stream without a terminal marker");
                            }
                        }
                        Ok(ProviderEvent::Done) => {}
                        Err(e) => { failed = Some(e.to_string()); break; }
                    }
                    if persist && last_save.elapsed() > Duration::from_millis(400) {
                        let _ = engine.store.put_message(&assistant);
                        last_save = Instant::now();
                    }
                }

                if failed.is_some() || cancel.load(Ordering::SeqCst) { break 'steps; }

                let calls = assembler.finish();
                if calls.is_empty() || !agent_on { break 'steps; }

                // Record the assistant turn that requested tools, then run them.
                assistant.tool_calls.extend(calls.clone());
                history.push(ChatMessage {
                    role: "assistant".into(),
                    content: Value::String(turn_text.clone()),
                    tool_calls: Some(calls.iter().map(|c| serde_json::json!({
                        "id": c.id, "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments }
                    })).collect()),
                    ..Default::default()
                });

                for call in calls {
                    if cancel.load(Ordering::SeqCst) { break 'steps; }
                    step_index += 1;
                    let risk = tools::risk_of(&call.name);
                    yield StreamEvent::ToolCall { call: call.clone() };
                    yield StreamEvent::Step { step: types::AgentStep {
                        index: step_index, kind: "tool".into(), title: call.name.clone(),
                        detail: call.arguments.chars().take(300).collect(), status: types::StepStatus::Running,
                    }};

                    let approved = match settings.approval_mode {
                        ApprovalMode::Auto => true,
                        ApprovalMode::Deny => false,
                        ApprovalMode::Ask => {
                            if risk == tools::Risk::Low {
                                true
                            } else {
                                let (approval_id, decision) = engine.approvals
                                    .request(&call, risk, Duration::from_secs(120)).await;
                                yield StreamEvent::ApprovalRequired {
                                    approval_id, call: call.clone(), risk: risk.as_str().to_string(),
                                };
                                decision
                            }
                        }
                    };

                    let started = Instant::now();
                    let outcome: ToolOutcome = if !approved {
                        ToolOutcome { ok: false, content: "User denied this tool call.".into(), results: Vec::new(), images: Vec::new() }
                    } else {
                        let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
                        tools::execute(&call.name, &args, &ctx).await
                    };

                    let result = ToolResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        ok: outcome.ok,
                        content: outcome.content.clone(),
                        duration_ms: started.elapsed().as_millis() as u64,
                    };
                    assistant.tool_results.push(result.clone());
                    if !outcome.results.is_empty() {
                        assistant.search_results.extend(outcome.results.clone());
                        yield StreamEvent::Search { results: outcome.results.clone() };
                    }
                    if !outcome.images.is_empty() {
                        assistant.generated_images.extend(outcome.images.clone());
                        yield StreamEvent::Images { urls: outcome.images.clone() };
                    }
                    assistant.steps.push(types::AgentStep {
                        index: step_index, kind: "tool".into(), title: call.name.clone(),
                        detail: outcome.content.chars().take(300).collect(),
                        status: if outcome.ok { types::StepStatus::Done } else { types::StepStatus::Failed },
                    });
                    yield StreamEvent::ToolResult { result: result.clone() };
                    yield StreamEvent::Step { step: assistant.steps.last().cloned().unwrap_or_default() };

                    history.push(ChatMessage {
                        role: "tool".into(),
                        content: Value::String(outcome.content),
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.name.clone()),
                        ..Default::default()
                    });
                    if persist { let _ = engine.store.put_message(&assistant); }
                }
                // Loop again so the model can consume the tool results.
            }

            assistant.partial = false;
            assistant.usage = usage_total;
            if let Some(err) = failed {
                assistant.error = Some(err.clone());
                yield StreamEvent::Error { message: err };
            }
            if cancel.load(Ordering::SeqCst) {
                assistant.error = Some("stopped".into());
            }
            if persist {
                let _ = engine.store.put_message(&assistant);
                conv.leaf_id = Some(response_id.clone());
                conv.modified_at = types::now_ms();
                if conv.title == "New chat" && !req.message.trim().is_empty() {
                    conv.title = types::derive_title(&req.message);
                }
                conv.model = Some(model.clone());
                let _ = engine.store.put_conversation(&conv);
            }
            engine.cancels.lock().await.remove(&response_id);
            yield StreamEvent::Done { response_id };
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_engine() -> Engine {
        let dir = std::env::temp_dir().join(format!("grok-rs-test-{}", types::new_id()));
        Engine::open(&dir).expect("engine")
    }

    #[test]
    fn engine_creates_and_lists_conversations() {
        let e = temp_engine();
        let c = e.create_conversation(Some("hello"), false).unwrap();
        assert_eq!(e.conversations().unwrap().len(), 1);
        assert_eq!(e.conversation(&c.id).unwrap().unwrap().title, "hello");
        e.delete_conversation(&c.id).unwrap();
        assert_eq!(e.conversations().unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(e.data_dir());
    }

    #[test]
    fn temporary_conversations_are_not_persisted() {
        let e = temp_engine();
        let c = e.create_conversation(None, true).unwrap();
        assert!(e.conversations().unwrap().is_empty());
        assert!(e.conversation(&c.id).unwrap().is_none());
        let _ = std::fs::remove_dir_all(e.data_dir());
    }

    #[test]
    fn settings_roundtrip_and_token_generation() {
        let e = temp_engine();
        let s = e.settings();
        assert!(!s.host.token.is_empty());
        let mut next = s.clone();
        next.language = "fa".into();
        e.update_settings(next).unwrap();
        assert_eq!(e.settings().language, "fa");
        let _ = std::fs::remove_dir_all(e.data_dir());
    }

    #[tokio::test]
    async fn chat_without_provider_key_reports_actionable_error() {
        let e = temp_engine();
        let mut s = e.settings();
        s.providers[0].base_url = String::new();
        e.update_settings(s).unwrap();
        let events: Vec<StreamEvent> = e
            .chat(ChatRequest {
                message: "hi".into(),
                ..Default::default()
            })
            .collect()
            .await;
        assert!(events.iter().any(
            |ev| matches!(ev, StreamEvent::Error { message } if message.contains("base URL"))
        ));
        let _ = std::fs::remove_dir_all(e.data_dir());
    }

    #[tokio::test]
    async fn stop_unknown_response_is_false() {
        let e = temp_engine();
        assert!(!e.stop("nope").await);
        let _ = std::fs::remove_dir_all(e.data_dir());
    }

    #[test]
    fn memories_crud() {
        let e = temp_engine();
        let m = e.add_memory("user prefers Persian").unwrap();
        assert_eq!(e.memories().unwrap().len(), 1);
        e.delete_memory(&m.id).unwrap();
        assert_eq!(e.memories().unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(e.data_dir());
    }
}
