use serde::{Deserialize, Serialize};

use crate::types::new_id;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApiKind {
    #[default]
    OpenAi,
    Anthropic,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ApiKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub known_models: Vec<String>,
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "yes")]
    pub supports_tools: bool,
    #[serde(default = "yes")]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

fn yes() -> bool {
    true
}
fn default_timeout() -> u64 {
    300
}

impl Provider {
    pub fn new(name: &str, kind: ApiKind, base_url: &str, model: &str) -> Self {
        Self {
            id: new_id(),
            name: name.to_string(),
            kind,
            base_url: base_url.to_string(),
            api_key: String::new(),
            model: model.to_string(),
            known_models: Vec::new(),
            extra_headers: Vec::new(),
            timeout_secs: default_timeout(),
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            reasoning_effort: None,
        }
    }

    /// Normalised root. An empty path gets the conventional version prefix so that
    /// `https://api.openai.com` and `https://api.openai.com/v1` both work.
    pub fn root(&self) -> String {
        let trimmed = self.base_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return String::new();
        }
        match url::Url::parse(trimmed) {
            Ok(u) => {
                let path = u.path().trim_end_matches('/');
                if path.is_empty() && self.kind == ApiKind::OpenAi {
                    format!("{trimmed}/v1")
                } else {
                    trimmed.to_string()
                }
            }
            Err(_) => trimmed.to_string(),
        }
    }

    pub fn chat_endpoint(&self) -> String {
        match self.kind {
            ApiKind::OpenAi => format!("{}/chat/completions", self.root()),
            ApiKind::Anthropic => format!("{}/messages", self.root()),
        }
    }

    pub fn models_endpoint(&self) -> String {
        match self.kind {
            ApiKind::OpenAi => format!("{}/models", self.root()),
            ApiKind::Anthropic => format!("{}/models", self.root()),
        }
    }

    pub fn images_endpoint(&self) -> String {
        format!("{}/images/generations", self.root())
    }

    pub fn is_local(&self) -> bool {
        let b = self.base_url.to_ascii_lowercase();
        b.contains("localhost")
            || b.contains("127.0.0.1")
            || b.contains("10.0.2.2")
            || b.contains("192.168.")
    }

    pub fn redacted(&self) -> Provider {
        let mut p = self.clone();
        p.api_key = if p.api_key.is_empty() {
            String::new()
        } else {
            let tail: String = p
                .api_key
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("••••{tail}")
        };
        p
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Auto,
    #[default]
    Ask,
    Deny,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchBackend {
    #[default]
    DuckDuckGo,
    Brave,
    Tavily,
    Searxng,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchConfig {
    pub backend: SearchBackend,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_result_count")]
    pub max_results: u32,
}

fn default_result_count() -> u32 {
    6
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            backend: SearchBackend::default(),
            api_key: String::new(),
            endpoint: String::new(),
            max_results: default_result_count(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostConfig {
    /// Serve on the LAN so other devices can use this phone as the agent host.
    #[serde(default)]
    pub allow_lan: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub allow_shell: bool,
    #[serde(default)]
    pub workspace_dir: String,
}

fn default_port() -> u16 {
    8787
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            allow_lan: false,
            port: default_port(),
            token: String::new(),
            allow_shell: false,
            workspace_dir: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub providers: Vec<Provider>,
    pub active_provider: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub agent_enabled: bool,
    #[serde(default = "default_max_steps")]
    pub agent_max_steps: u32,
    #[serde(default)]
    pub approval_mode: ApprovalMode,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub host: HostConfig,
    #[serde(default = "yes")]
    pub stream_resume: bool,
    #[serde(default = "yes")]
    pub save_history: bool,
    #[serde(default)]
    pub personas: Vec<Persona>,
}

fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> u32 {
    4096
}
fn default_top_p() -> f32 {
    1.0
}
fn default_theme() -> String {
    "dark".into()
}
fn default_max_steps() -> u32 {
    8
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

impl Default for Settings {
    fn default() -> Self {
        let xai = Provider::new(
            "xAI (Grok)",
            ApiKind::OpenAi,
            "https://api.x.ai/v1",
            "grok-4-1",
        );
        let active = xai.id.clone();
        Self {
            providers: vec![xai],
            active_provider: active,
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            top_p: default_top_p(),
            system_prompt: String::new(),
            language: "en".into(),
            theme: default_theme(),
            agent_enabled: false,
            agent_max_steps: default_max_steps(),
            approval_mode: ApprovalMode::default(),
            enabled_tools: default_tools(),
            search: SearchConfig::default(),
            host: HostConfig::default(),
            stream_resume: true,
            save_history: true,
            personas: default_personas(),
        }
    }
}

impl Settings {
    pub fn active(&self) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|p| p.id == self.active_provider)
            .or_else(|| self.providers.first())
    }

    pub fn provider(&self, id: Option<&str>) -> Option<&Provider> {
        match id {
            Some(id) => self.providers.iter().find(|p| p.id == id),
            None => self.active(),
        }
    }

    pub fn ensure_token(&mut self) {
        if self.host.token.is_empty() {
            self.host.token = new_id().replace('-', "");
        }
    }

    pub fn redacted(&self) -> Settings {
        let mut s = self.clone();
        s.providers = s.providers.iter().map(|p| p.redacted()).collect();
        if !s.search.api_key.is_empty() {
            s.search.api_key = "••••".into();
        }
        s
    }
}

pub fn default_tools() -> Vec<String> {
    [
        "web_search",
        "fetch_url",
        "calculator",
        "datetime",
        "memory_store",
        "memory_recall",
        "read_file",
        "list_dir",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn default_personas() -> Vec<Persona> {
    vec![
        Persona {
            id: "default".into(),
            name: "Default".into(),
            prompt: String::new(),
        },
        Persona {
            id: "concise".into(),
            name: "Concise".into(),
            prompt: "Answer in as few words as possible. No preamble, no summary.".into(),
        },
        Persona {
            id: "engineer".into(),
            name: "Engineer".into(),
            prompt: "You are a senior software engineer. Give correct, runnable code, note edge cases, and prefer standard library solutions.".into(),
        },
        Persona {
            id: "persian".into(),
            name: "فارسی".into(),
            prompt: "همیشه به فارسی روان و طبیعی پاسخ بده. اصطلاحات فنی را در صورت نیاز به انگلیسی داخل پرانتز بیاور.".into(),
        },
        Persona {
            id: "tutor".into(),
            name: "Tutor".into(),
            prompt: "Teach step by step. Check understanding with a short question at the end.".into(),
        },
    ]
}

/// Ready-made endpoints so the user only has to paste a key.
pub fn presets() -> Vec<(&'static str, ApiKind, &'static str, &'static str)> {
    vec![
        (
            "xAI (Grok)",
            ApiKind::OpenAi,
            "https://api.x.ai/v1",
            "grok-4-1",
        ),
        (
            "OpenAI",
            ApiKind::OpenAi,
            "https://api.openai.com/v1",
            "gpt-4o",
        ),
        (
            "OpenRouter",
            ApiKind::OpenAi,
            "https://openrouter.ai/api/v1",
            "x-ai/grok-4",
        ),
        (
            "Anthropic",
            ApiKind::Anthropic,
            "https://api.anthropic.com/v1",
            "claude-sonnet-4-5",
        ),
        (
            "Groq",
            ApiKind::OpenAi,
            "https://api.groq.com/openai/v1",
            "llama-3.3-70b-versatile",
        ),
        (
            "Together",
            ApiKind::OpenAi,
            "https://api.together.xyz/v1",
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        ),
        (
            "Mistral",
            ApiKind::OpenAi,
            "https://api.mistral.ai/v1",
            "mistral-large-latest",
        ),
        (
            "DeepSeek",
            ApiKind::OpenAi,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        ),
        (
            "Google Gemini",
            ApiKind::OpenAi,
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-2.5-pro",
        ),
        (
            "Ollama (local)",
            ApiKind::OpenAi,
            "http://127.0.0.1:11434/v1",
            "llama3.2",
        ),
        (
            "LM Studio (local)",
            ApiKind::OpenAi,
            "http://127.0.0.1:1234/v1",
            "local-model",
        ),
        (
            "llama.cpp (local)",
            ApiKind::OpenAi,
            "http://127.0.0.1:8080/v1",
            "local-model",
        ),
        (
            "vLLM (self-host)",
            ApiKind::OpenAi,
            "http://127.0.0.1:8000/v1",
            "your-model",
        ),
        ("Custom", ApiKind::OpenAi, "", ""),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_adds_version_only_when_path_empty() {
        let mut p = Provider::new("t", ApiKind::OpenAi, "https://api.openai.com", "m");
        assert_eq!(p.root(), "https://api.openai.com/v1");
        p.base_url = "https://api.openai.com/v1/".into();
        assert_eq!(p.root(), "https://api.openai.com/v1");
        p.base_url = "https://openrouter.ai/api/v1".into();
        assert_eq!(p.root(), "https://openrouter.ai/api/v1");
        assert_eq!(
            p.chat_endpoint(),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn anthropic_root_is_not_rewritten() {
        let p = Provider::new("a", ApiKind::Anthropic, "https://api.anthropic.com", "m");
        assert_eq!(p.root(), "https://api.anthropic.com");
        assert_eq!(p.chat_endpoint(), "https://api.anthropic.com/messages");
    }

    #[test]
    fn redaction_never_leaks_key_body() {
        let mut p = Provider::new("t", ApiKind::OpenAi, "https://x.ai/v1", "m");
        p.api_key = "xai-SECRET-abcdef123456".into();
        let r = p.redacted();
        assert!(!r.api_key.contains("SECRET"));
        assert!(r.api_key.ends_with("3456"));
    }

    #[test]
    fn default_settings_have_active_provider() {
        let s = Settings::default();
        assert!(s.active().is_some());
        assert_eq!(s.active().unwrap().model, "grok-4-1");
    }
}
