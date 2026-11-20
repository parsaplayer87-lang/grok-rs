use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{oneshot, Mutex};

use crate::config::Settings;
use crate::provider::{ChatMessage, ProviderEvent};
use crate::tools::Risk;
use crate::types::{AttachmentKind, Message, Sender, ToolCall};

/// Reassembles streamed tool-call fragments. Providers split `arguments` across
/// many deltas keyed by index, and the id/name may arrive only on the first one.
#[derive(Default)]
pub struct ToolCallAssembler {
    slots: HashMap<usize, ToolCall>,
}

impl ToolCallAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, ev: &ProviderEvent) {
        if let ProviderEvent::ToolCallDelta {
            index,
            id,
            name,
            args_delta,
        } = ev
        {
            let slot = self.slots.entry(*index).or_default();
            if let Some(id) = id {
                if !id.is_empty() {
                    slot.id = id.clone();
                }
            }
            if let Some(name) = name {
                if !name.is_empty() {
                    slot.name = name.clone();
                }
            }
            slot.arguments.push_str(args_delta);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn finish(self) -> Vec<ToolCall> {
        let mut idx: Vec<usize> = self.slots.keys().copied().collect();
        idx.sort_unstable();
        idx.into_iter()
            .filter_map(|i| self.slots.get(&i).cloned())
            .filter(|c| !c.name.is_empty())
            .map(|mut c| {
                if c.id.is_empty() {
                    c.id = format!("call_{}", crate::types::new_id());
                }
                if c.arguments.trim().is_empty() {
                    c.arguments = "{}".into();
                }
                c
            })
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub call: ToolCall,
    pub risk: String,
    pub created_at: i64,
}

struct Entry {
    info: PendingApproval,
    tx: Option<oneshot::Sender<bool>>,
}

#[derive(Clone, Default)]
pub struct ApprovalRegistry {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a call and wait for a decision. Returns the approval id and the
    /// decision; a timeout counts as a denial so a stalled UI cannot hang the agent.
    pub async fn request(&self, call: &ToolCall, risk: Risk, timeout: Duration) -> (String, bool) {
        let id = crate::types::new_id();
        let (tx, rx) = oneshot::channel();
        let info = PendingApproval {
            id: id.clone(),
            call: call.clone(),
            risk: risk.as_str().to_string(),
            created_at: crate::types::now_ms(),
        };
        self.inner
            .lock()
            .await
            .insert(id.clone(), Entry { info, tx: Some(tx) });
        let decided = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(v)) => v,
            _ => false,
        };
        self.inner.lock().await.remove(&id);
        (id, decided)
    }

    pub async fn resolve(&self, id: &str, approved: bool) -> bool {
        let mut guard = self.inner.lock().await;
        match guard.get_mut(id) {
            Some(entry) => match entry.tx.take() {
                Some(tx) => {
                    let _ = tx.send(approved);
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    pub async fn pending(&self) -> Vec<PendingApproval> {
        self.inner
            .lock()
            .await
            .values()
            .map(|e| e.info.clone())
            .collect()
    }

    pub async fn deny_all(&self) {
        let mut guard = self.inner.lock().await;
        for entry in guard.values_mut() {
            if let Some(tx) = entry.tx.take() {
                let _ = tx.send(false);
            }
        }
    }
}

pub fn system_prompt(
    settings: &Settings,
    persona_prompt: Option<&str>,
    memories: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !settings.system_prompt.trim().is_empty() {
        parts.push(settings.system_prompt.trim().to_string());
    }
    if let Some(p) = persona_prompt {
        if !p.trim().is_empty() {
            parts.push(p.trim().to_string());
        }
    }
    if settings.language == "fa" {
        parts.push("پاسخ‌ها را به فارسی روان بنویس. کد و اصطلاحات فنی را به انگلیسی نگه دار.".into());
    }
    if !memories.is_empty() {
        parts.push(format!(
            "Known durable facts about the user (use when relevant, do not recite):\n{}",
            memories
                .iter()
                .map(|m| format!("- {m}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    parts.join("\n\n")
}

/// Convert the persisted branch into provider messages, preserving vision parts
/// and the tool-call/tool-result pairing the APIs require.
pub fn build_history(system: &str, branch: &[Message], supports_vision: bool) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    if !system.trim().is_empty() {
        out.push(ChatMessage::text("system", system));
    }
    for m in branch {
        match m.sender {
            Sender::System => out.push(ChatMessage::text("system", m.text.clone())),
            Sender::User => {
                let mut parts = Vec::new();
                if !m.text.trim().is_empty() {
                    parts.push(json!({"type":"text","text":m.text}));
                }
                for a in &m.attachments {
                    match a.kind {
                        AttachmentKind::Image if supports_vision => {
                            let url = a.url.clone().or_else(|| {
                                a.data_b64
                                    .as_ref()
                                    .map(|b| format!("data:{};base64,{}", a.mime, b))
                            });
                            if let Some(url) = url {
                                parts.push(json!({"type":"image_url","image_url":{"url":url}}));
                            }
                        }
                        _ => {
                            if let Some(text) = &a.text {
                                parts.push(json!({"type":"text","text":format!("[Attached file: {}]\n{}", a.name, text)}));
                            } else {
                                parts.push(json!({"type":"text","text":format!("[Attached file: {} ({}, {} bytes)]", a.name, a.mime, a.size)}));
                            }
                        }
                    }
                }
                if parts.is_empty() {
                    parts.push(json!({"type":"text","text":" "}));
                }
                out.push(ChatMessage::parts("user", parts));
            }
            Sender::Assistant => {
                let calls: Vec<serde_json::Value> = m
                    .tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments }
                        })
                    })
                    .collect();
                let mut msg = ChatMessage::text("assistant", m.text.clone());
                if !calls.is_empty() {
                    msg.tool_calls = Some(calls);
                }
                out.push(msg);
                for r in &m.tool_results {
                    out.push(ChatMessage {
                        role: "tool".into(),
                        content: serde_json::Value::String(r.content.clone()),
                        tool_call_id: Some(r.call_id.clone()),
                        name: Some(r.name.clone()),
                        ..Default::default()
                    });
                }
            }
            Sender::Tool => {
                out.push(ChatMessage {
                    role: "tool".into(),
                    content: serde_json::Value::String(m.text.clone()),
                    tool_call_id: m.tool_results.first().map(|r| r.call_id.clone()),
                    ..Default::default()
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembler_merges_fragments_in_index_order() {
        let mut a = ToolCallAssembler::new();
        a.push(&ProviderEvent::ToolCallDelta {
            index: 1,
            id: Some("b".into()),
            name: Some("second".into()),
            args_delta: "{\"x\":".into(),
        });
        a.push(&ProviderEvent::ToolCallDelta {
            index: 0,
            id: Some("a".into()),
            name: Some("first".into()),
            args_delta: "{}".into(),
        });
        a.push(&ProviderEvent::ToolCallDelta {
            index: 1,
            id: None,
            name: None,
            args_delta: "1}".into(),
        });
        let calls = a.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "first");
        assert_eq!(calls[1].name, "second");
        assert_eq!(calls[1].arguments, "{\"x\":1}");
    }

    #[test]
    fn assembler_defaults_empty_arguments_and_drops_nameless() {
        let mut a = ToolCallAssembler::new();
        a.push(&ProviderEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: Some("datetime".into()),
            args_delta: String::new(),
        });
        a.push(&ProviderEvent::ToolCallDelta {
            index: 1,
            id: Some("x".into()),
            name: None,
            args_delta: "{}".into(),
        });
        let calls = a.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments, "{}");
        assert!(!calls[0].id.is_empty());
    }

    #[tokio::test]
    async fn approval_resolves_and_timeouts_deny() {
        let reg = ApprovalRegistry::new();
        let call = ToolCall {
            id: "c1".into(),
            name: "write_file".into(),
            arguments: "{}".into(),
        };
        let reg2 = reg.clone();
        let call2 = call.clone();
        let handle = tokio::spawn(async move {
            reg2.request(&call2, Risk::High, Duration::from_secs(5))
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let pending = reg.pending().await;
        assert_eq!(pending.len(), 1);
        assert!(reg.resolve(&pending[0].id, true).await);
        let (id, approved) = handle.await.unwrap();
        assert_eq!(id, pending[0].id);
        assert!(approved);

        let (_, denied) = reg
            .request(&call, Risk::High, Duration::from_millis(30))
            .await;
        assert!(!denied);
        assert!(reg.pending().await.is_empty());
    }

    #[tokio::test]
    async fn deny_all_releases_waiters() {
        let reg = ApprovalRegistry::new();
        let call = ToolCall {
            id: "c".into(),
            name: "run_shell".into(),
            arguments: "{}".into(),
        };
        let reg2 = reg.clone();
        let handle = tokio::spawn(async move {
            reg2.request(&call, Risk::High, Duration::from_secs(5))
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        reg.deny_all().await;
        let (_, approved) = handle.await.unwrap();
        assert!(!approved);
    }

    #[test]
    fn history_pairs_tool_calls_with_results() {
        let branch = vec![
            Message {
                sender: Sender::User,
                text: "hi".into(),
                ..Default::default()
            },
            Message {
                sender: Sender::Assistant,
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "calculator".into(),
                    arguments: "{\"expression\":\"2+2\"}".into(),
                }],
                tool_results: vec![crate::types::ToolResult {
                    call_id: "t1".into(),
                    name: "calculator".into(),
                    ok: true,
                    content: "4".into(),
                    duration_ms: 1,
                }],
                ..Default::default()
            },
        ];
        let h = build_history("sys", &branch, true);
        assert_eq!(h[0].role, "system");
        assert_eq!(h[1].role, "user");
        assert_eq!(h[2].role, "assistant");
        assert!(h[2].tool_calls.is_some());
        assert_eq!(h[3].role, "tool");
        assert_eq!(h[3].tool_call_id.as_deref(), Some("t1"));
    }

    #[test]
    fn vision_parts_only_when_supported() {
        let branch = vec![Message {
            sender: Sender::User,
            text: "what is this".into(),
            attachments: vec![crate::types::Attachment {
                kind: AttachmentKind::Image,
                mime: "image/png".into(),
                data_b64: Some("AAAA".into()),
                ..Default::default()
            }],
            ..Default::default()
        }];
        let with = build_history("", &branch, true);
        let without = build_history("", &branch, false);
        assert!(with[0].content.to_string().contains("image_url"));
        assert!(!without[0].content.to_string().contains("image_url"));
    }

    #[test]
    fn persian_language_instruction_is_added() {
        let s = Settings {
            language: "fa".into(),
            ..Default::default()
        };
        let p = system_prompt(&s, None, &[]);
        assert!(p.contains("فارسی"));
    }
}
