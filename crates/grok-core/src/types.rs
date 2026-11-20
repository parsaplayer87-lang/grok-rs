use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Sender {
    #[default]
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    #[default]
    File,
    Image,
    Audio,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Attachment {
    pub id: String,
    pub kind: AttachmentKind,
    pub name: String,
    pub mime: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub ok: bool,
    pub content: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    #[default]
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AgentStep {
    pub index: u32,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub status: StepStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

impl Usage {
    pub fn accumulate(&mut self, other: &Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.total_tokens += other.total_tokens;
    }
}

/// A node in the response tree. `parent_id` is what makes editing or regenerating
/// produce a sibling branch rather than mutating history.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub parent_id: Option<String>,
    pub sender: Sender,
    pub text: String,
    pub reasoning: String,
    pub created_at: i64,
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub attachments: Vec<Attachment>,
    pub search_results: Vec<SearchResult>,
    pub cited_results: Vec<SearchResult>,
    pub generated_images: Vec<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub steps: Vec<AgentStep>,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub starred: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub modified_at: i64,
    pub starred: bool,
    pub temporary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leaf_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub created_at: i64,
    pub source_conversation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    Start {
        conversation_id: String,
        response_id: String,
        model: String,
    },
    Reasoning {
        text: String,
    },
    Delta {
        text: String,
    },
    ToolCall {
        call: ToolCall,
    },
    ToolResult {
        result: ToolResult,
    },
    ApprovalRequired {
        approval_id: String,
        call: ToolCall,
        risk: String,
    },
    Search {
        results: Vec<SearchResult>,
    },
    Images {
        urls: Vec<String>,
    },
    Step {
        step: AgentStep,
    },
    Usage {
        usage: Usage,
    },
    Done {
        response_id: String,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub temporary: bool,
    #[serde(default)]
    pub agent: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub search: bool,
    #[serde(default)]
    pub image_generation: bool,
    #[serde(default)]
    pub regenerate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Walk from a leaf to the root and return the branch in chronological order.
pub fn branch_to(messages: &[Message], leaf: Option<&str>) -> Vec<Message> {
    let Some(leaf) = leaf else {
        return messages.to_vec();
    };
    let mut chain = Vec::new();
    let mut cursor = Some(leaf.to_string());
    let mut guard = 0;
    while let Some(id) = cursor {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        match messages.iter().find(|m| m.id == id) {
            Some(m) => {
                cursor = m.parent_id.clone();
                chain.push(m.clone());
            }
            None => break,
        }
    }
    chain.reverse();
    chain
}

pub fn derive_title(text: &str) -> String {
    let clean: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return "New chat".to_string();
    }
    let cut: String = trimmed.chars().take(60).collect();
    if trimmed.chars().count() > 60 {
        format!("{cut}…")
    } else {
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, parent: Option<&str>) -> Message {
        Message {
            id: id.into(),
            parent_id: parent.map(|s| s.into()),
            ..Default::default()
        }
    }

    #[test]
    fn branch_walks_leaf_to_root() {
        let all = vec![
            msg("a", None),
            msg("b", Some("a")),
            msg("c", Some("b")),
            msg("b2", Some("a")),
            msg("c2", Some("b2")),
        ];
        let branch = branch_to(&all, Some("c2"));
        let ids: Vec<_> = branch.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b2", "c2"]);
    }

    #[test]
    fn branch_tolerates_cycle() {
        let all = vec![msg("a", Some("b")), msg("b", Some("a"))];
        let branch = branch_to(&all, Some("a"));
        assert!(branch.len() <= 100_001);
    }

    #[test]
    fn title_derivation_handles_multibyte() {
        let t = derive_title("سلام دنیا این یک پیام آزمایشی طولانی است که باید بریده شود");
        assert!(t.chars().count() <= 61);
        assert!(t.starts_with("سلام"));
        assert_eq!(derive_title("   "), "New chat");
    }
}
