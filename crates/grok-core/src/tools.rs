use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

use crate::config::{SearchBackend, Settings};
use crate::provider::ToolSpec;
use crate::store::Store;
use crate::types::{new_id, now_ms, Memory, SearchResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub risk: Risk,
    pub shell_only: bool,
}

pub struct ToolOutcome {
    pub ok: bool,
    pub content: String,
    pub results: Vec<SearchResult>,
    pub images: Vec<String>,
}

impl ToolOutcome {
    fn ok(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            results: Vec::new(),
            images: Vec::new(),
        }
    }
    fn err(content: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
            results: Vec::new(),
            images: Vec::new(),
        }
    }
}

pub struct ToolCtx {
    pub http: reqwest::Client,
    pub store: Store,
    pub settings: Settings,
    pub workspace: PathBuf,
}

pub fn registry() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search",
            description: "Search the web and return titles, URLs and snippets. Use for current events or facts you are unsure about.",
            parameters: json!({"type":"object","properties":{"query":{"type":"string","description":"Search query"}},"required":["query"]}),
            risk: Risk::Medium, shell_only: false,
        },
        ToolDef {
            name: "fetch_url",
            description: "Fetch a URL and return its readable text content.",
            parameters: json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
            risk: Risk::Medium, shell_only: false,
        },
        ToolDef {
            name: "calculator",
            description: "Evaluate an arithmetic expression. Supports + - * / % ^ parentheses and sqrt, sin, cos, tan, log, ln, abs, pi, e.",
            parameters: json!({"type":"object","properties":{"expression":{"type":"string"}},"required":["expression"]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "datetime",
            description: "Get the current date and time, optionally as a Unix timestamp.",
            parameters: json!({"type":"object","properties":{"utc":{"type":"boolean"}},"required":[]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "memory_store",
            description: "Persist a durable fact about the user or project for future conversations.",
            parameters: json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "memory_recall",
            description: "Recall stored memories, optionally filtered by a substring.",
            parameters: json!({"type":"object","properties":{"query":{"type":"string"}},"required":[]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "read_file",
            description: "Read a UTF-8 text file inside the workspace directory.",
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "list_dir",
            description: "List entries of a directory inside the workspace.",
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":[]}),
            risk: Risk::Low, shell_only: false,
        },
        ToolDef {
            name: "write_file",
            description: "Write a UTF-8 text file inside the workspace directory. Overwrites existing files.",
            parameters: json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            risk: Risk::High, shell_only: false,
        },
        ToolDef {
            name: "http_request",
            description: "Make an arbitrary HTTP request. Use for APIs the other tools cannot reach.",
            parameters: json!({"type":"object","properties":{"url":{"type":"string"},"method":{"type":"string"},"body":{"type":"string"},"headers":{"type":"object"}},"required":["url"]}),
            risk: Risk::High, shell_only: false,
        },
        ToolDef {
            name: "run_shell",
            description: "Run a shell command on the host machine. Only available when the host explicitly enables it.",
            parameters: json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            risk: Risk::High, shell_only: true,
        },
    ]
}

pub fn specs(enabled: &[String], allow_shell: bool) -> Vec<ToolSpec> {
    registry()
        .into_iter()
        .filter(|t| enabled.iter().any(|e| e == t.name))
        .filter(|t| !t.shell_only || allow_shell)
        .map(|t| ToolSpec {
            name: t.name.to_string(),
            description: t.description.to_string(),
            parameters: t.parameters,
        })
        .collect()
}

pub fn risk_of(name: &str) -> Risk {
    registry()
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.risk)
        .unwrap_or(Risk::High)
}

/// Confine a user-supplied relative path to the workspace. Absolute paths and
/// `..` traversal are rejected rather than silently resolved elsewhere.
fn sandbox_path(workspace: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Ok(workspace.to_path_buf());
    }
    let p = Path::new(rel);
    if p.is_absolute() || rel.starts_with('~') {
        return Err("absolute paths are not allowed; use a path relative to the workspace".into());
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("path traversal ('..') is not allowed".into());
    }
    Ok(workspace.join(p))
}

pub async fn execute(name: &str, args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let started = Instant::now();
    let out = match name {
        "web_search" => web_search(args, ctx).await,
        "fetch_url" => fetch_url(args, ctx).await,
        "calculator" => calculator(args),
        "datetime" => datetime(args),
        "memory_store" => memory_store(args, ctx),
        "memory_recall" => memory_recall(args, ctx),
        "read_file" => read_file(args, ctx),
        "list_dir" => list_dir(args, ctx),
        "write_file" => write_file(args, ctx),
        "http_request" => http_request(args, ctx).await,
        "run_shell" => run_shell(args, ctx).await,
        _ => ToolOutcome::err(format!("unknown tool '{name}'")),
    };
    let _ = started.elapsed();
    out
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
}

async fn web_search(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let Some(query) = arg_str(args, "query") else {
        return ToolOutcome::err("missing 'query'");
    };
    let cfg = &ctx.settings.search;
    let limit = cfg.max_results.clamp(1, 10) as usize;
    let results = match cfg.backend {
        SearchBackend::None => return ToolOutcome::err("web search is disabled in settings"),
        SearchBackend::Brave => brave_search(query, &cfg.api_key, limit, ctx).await,
        SearchBackend::Tavily => tavily_search(query, &cfg.api_key, limit, ctx).await,
        SearchBackend::Searxng => searxng_search(query, &cfg.endpoint, limit, ctx).await,
        SearchBackend::DuckDuckGo => ddg_search(query, limit, ctx).await,
    };
    match results {
        Ok(r) if r.is_empty() => ToolOutcome::err(format!("no results for '{query}'")),
        Ok(r) => {
            let text = r
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {}\n{}\n{}", i + 1, s.title, s.url, s.snippet))
                .collect::<Vec<_>>()
                .join("\n\n");
            ToolOutcome {
                ok: true,
                content: text,
                results: r,
                images: Vec::new(),
            }
        }
        Err(e) => ToolOutcome::err(format!("search failed: {e}")),
    }
}

async fn ddg_search(query: &str, limit: usize, ctx: &ToolCtx) -> anyhow::Result<Vec<SearchResult>> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );
    let html = ctx
        .http
        .get(&url)
        .header("user-agent", "Mozilla/5.0 (compatible; grok-rs)")
        .send()
        .await?
        .text()
        .await?;
    let link_re =
        regex::Regex::new(r#"<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)?;
    let snip_re = regex::Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#)?;
    let snippets: Vec<String> = snip_re
        .captures_iter(&html)
        .map(|c| html_to_text(&c[1]))
        .collect();
    let mut out = Vec::new();
    for (i, cap) in link_re.captures_iter(&html).enumerate() {
        if out.len() >= limit {
            break;
        }
        let href = decode_ddg_href(&cap[1]);
        if href.is_empty() {
            continue;
        }
        out.push(SearchResult {
            title: html_to_text(&cap[2]),
            url: href,
            snippet: snippets.get(i).cloned().unwrap_or_default(),
            published: None,
        });
    }
    Ok(out)
}

fn decode_ddg_href(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let enc = &href[pos + 5..];
        let enc = enc.split('&').next().unwrap_or(enc);
        if let Ok(decoded) = urlencoding::decode(enc) {
            return decoded.into_owned();
        }
    }
    if href.starts_with("//") {
        return format!("https:{href}");
    }
    href.to_string()
}

async fn brave_search(
    query: &str,
    key: &str,
    limit: usize,
    ctx: &ToolCtx,
) -> anyhow::Result<Vec<SearchResult>> {
    if key.is_empty() {
        anyhow::bail!("Brave backend needs an API key");
    }
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={limit}",
        urlencoding::encode(query)
    );
    let v: Value = ctx
        .http
        .get(&url)
        .header("x-subscription-token", key)
        .header("accept", "application/json")
        .send()
        .await?
        .json()
        .await?;
    Ok(v["web"]["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit)
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or("").into(),
                    url: r["url"].as_str().unwrap_or("").into(),
                    snippet: r["description"].as_str().unwrap_or("").into(),
                    published: r["age"].as_str().map(String::from),
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn tavily_search(
    query: &str,
    key: &str,
    limit: usize,
    ctx: &ToolCtx,
) -> anyhow::Result<Vec<SearchResult>> {
    if key.is_empty() {
        anyhow::bail!("Tavily backend needs an API key");
    }
    let v: Value = ctx
        .http
        .post("https://api.tavily.com/search")
        .json(&json!({"api_key":key,"query":query,"max_results":limit}))
        .send()
        .await?
        .json()
        .await?;
    Ok(v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit)
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or("").into(),
                    url: r["url"].as_str().unwrap_or("").into(),
                    snippet: r["content"].as_str().unwrap_or("").into(),
                    published: r["published_date"].as_str().map(String::from),
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn searxng_search(
    query: &str,
    endpoint: &str,
    limit: usize,
    ctx: &ToolCtx,
) -> anyhow::Result<Vec<SearchResult>> {
    if endpoint.is_empty() {
        anyhow::bail!("SearXNG backend needs an endpoint URL");
    }
    let base = endpoint.trim_end_matches('/');
    let url = format!("{base}/search?q={}&format=json", urlencoding::encode(query));
    let v: Value = ctx.http.get(&url).send().await?.json().await?;
    Ok(v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit)
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or("").into(),
                    url: r["url"].as_str().unwrap_or("").into(),
                    snippet: r["content"].as_str().unwrap_or("").into(),
                    published: r["publishedDate"].as_str().map(String::from),
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn fetch_url(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let Some(url) = arg_str(args, "url") else {
        return ToolOutcome::err("missing 'url'");
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return ToolOutcome::err("only http(s) URLs are allowed");
    }
    match ctx
        .http
        .get(url)
        .header("user-agent", "Mozilla/5.0 (compatible; grok-rs)")
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let ctype = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            match resp.text().await {
                Ok(body) => {
                    let text = if ctype.contains("html") {
                        html_to_text(&body)
                    } else {
                        body
                    };
                    let clipped: String = text.chars().take(20_000).collect();
                    if !status.is_success() {
                        return ToolOutcome::err(format!(
                            "HTTP {status}: {}",
                            clipped.chars().take(500).collect::<String>()
                        ));
                    }
                    ToolOutcome::ok(format!("URL: {url}\n\n{clipped}"))
                }
                Err(e) => ToolOutcome::err(format!("reading body failed: {e}")),
            }
        }
        Err(e) => ToolOutcome::err(format!("fetch failed: {e}")),
    }
}

pub fn html_to_text(html: &str) -> String {
    // The regex crate has no backreferences, so each discardable block gets its own
    // open/close pair rather than a captured tag name.
    let blocks = regex::Regex::new(
        r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>|<noscript[^>]*>.*?</noscript>|<svg[^>]*>.*?</svg>",
    )
    .expect("literal pattern");
    let stripped = blocks.replace_all(html, " ");
    let breaks =
        regex::Regex::new(r"(?i)</(p|div|li|h[1-6]|tr|section|article)>").expect("literal pattern");
    let blocked = breaks.replace_all(&stripped, "\n");
    let tags = regex::Regex::new(r"<[^>]+>").expect("literal pattern");
    let no_tags = tags.replace_all(&blocked, " ");
    let decoded = no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…");
    let mut out = String::new();
    for line in decoded.lines() {
        let clean = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !clean.is_empty() {
            out.push_str(&clean);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

// ---- calculator: tokenizer + shunting-yard, no eval anywhere ----

#[derive(Debug, PartialEq)]
enum Tok {
    Num(f64),
    Op(char),
    LParen,
    RParen,
    Func(String),
    Const(String),
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().collect();
            let n: f64 = s.parse().map_err(|_| format!("invalid number '{s}'"))?;
            toks.push(Tok::Num(n));
            continue;
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect::<String>().to_lowercase();
            match name.as_str() {
                "pi" | "e" | "tau" => toks.push(Tok::Const(name)),
                "sqrt" | "sin" | "cos" | "tan" | "log" | "ln" | "abs" | "exp" | "floor"
                | "ceil" | "round" => toks.push(Tok::Func(name)),
                _ => return Err(format!("unknown identifier '{name}'")),
            }
            continue;
        }
        match c {
            '+' | '-' | '*' | '/' | '%' | '^' => {
                toks.push(Tok::Op(c));
                i += 1;
            }
            '(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            '×' => {
                toks.push(Tok::Op('*'));
                i += 1;
            }
            '÷' => {
                toks.push(Tok::Op('/'));
                i += 1;
            }
            _ => return Err(format!("unexpected character '{c}'")),
        }
    }
    Ok(toks)
}

fn precedence(op: char) -> u8 {
    match op {
        '+' | '-' => 1,
        '*' | '/' | '%' => 2,
        '^' => 3,
        _ => 0,
    }
}

fn apply(op: char, b: f64, a: f64) -> Result<f64, String> {
    let v = match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        '/' => {
            if b == 0.0 {
                return Err("division by zero".into());
            }
            a / b
        }
        '%' => {
            if b == 0.0 {
                return Err("modulo by zero".into());
            }
            a % b
        }
        '^' => a.powf(b),
        _ => return Err(format!("unknown operator '{op}'")),
    };
    if !v.is_finite() {
        return Err("result is not finite".into());
    }
    Ok(v)
}

fn call_func(name: &str, a: f64) -> Result<f64, String> {
    let v = match name {
        "sqrt" => {
            if a < 0.0 {
                return Err("sqrt of negative number".into());
            }
            a.sqrt()
        }
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "log" => {
            if a <= 0.0 {
                return Err("log of non-positive number".into());
            }
            a.log10()
        }
        "ln" => {
            if a <= 0.0 {
                return Err("ln of non-positive number".into());
            }
            a.ln()
        }
        "abs" => a.abs(),
        "exp" => a.exp(),
        "floor" => a.floor(),
        "ceil" => a.ceil(),
        "round" => a.round(),
        _ => return Err(format!("unknown function '{name}'")),
    };
    if !v.is_finite() {
        return Err("result is not finite".into());
    }
    Ok(v)
}

pub fn evaluate(expr: &str) -> Result<f64, String> {
    let toks = tokenize(expr)?;
    if toks.is_empty() {
        return Err("empty expression".into());
    }
    let mut out: Vec<f64> = Vec::new();
    let mut ops: Vec<char> = Vec::new();
    let mut funcs: Vec<String> = Vec::new();
    let mut prev_operand = false;

    let reduce = |out: &mut Vec<f64>, op: char| -> Result<(), String> {
        if out.len() < 2 {
            return Err("malformed expression".into());
        }
        let b = out.pop().unwrap();
        let a = out.pop().unwrap();
        out.push(apply(op, b, a)?);
        Ok(())
    };

    for tok in toks {
        match tok {
            Tok::Num(n) => {
                out.push(n);
                prev_operand = true;
            }
            Tok::Const(name) => {
                out.push(match name.as_str() {
                    "pi" => std::f64::consts::PI,
                    "tau" => std::f64::consts::TAU,
                    _ => std::f64::consts::E,
                });
                prev_operand = true;
            }
            Tok::Func(name) => {
                funcs.push(name);
                prev_operand = false;
            }
            Tok::LParen => {
                ops.push('(');
                prev_operand = false;
            }
            Tok::RParen => {
                let mut found = false;
                while let Some(op) = ops.pop() {
                    if op == '(' {
                        found = true;
                        break;
                    }
                    reduce(&mut out, op)?;
                }
                if !found {
                    return Err("unbalanced parenthesis".into());
                }
                if let Some(f) = funcs.pop() {
                    if out.is_empty() {
                        return Err("malformed expression".into());
                    }
                    let a = out.pop().unwrap();
                    out.push(call_func(&f, a)?);
                }
                prev_operand = true;
            }
            Tok::Op(op) => {
                // unary minus / plus
                if !prev_operand && (op == '-' || op == '+') {
                    out.push(0.0);
                    while let Some(&top) = ops.last() {
                        if top != '(' && precedence(top) >= 3 {
                            reduce(&mut out, ops.pop().unwrap())?;
                        } else {
                            break;
                        }
                    }
                    ops.push(op);
                    prev_operand = false;
                    continue;
                }
                while let Some(&top) = ops.last() {
                    if top == '(' {
                        break;
                    }
                    let right_assoc = op == '^';
                    if precedence(top) > precedence(op)
                        || (precedence(top) == precedence(op) && !right_assoc)
                    {
                        reduce(&mut out, ops.pop().unwrap())?;
                    } else {
                        break;
                    }
                }
                ops.push(op);
                prev_operand = false;
            }
        }
    }
    while let Some(op) = ops.pop() {
        if op == '(' {
            return Err("unbalanced parenthesis".into());
        }
        reduce(&mut out, op)?;
    }
    if out.len() != 1 {
        return Err("malformed expression".into());
    }
    Ok(out[0])
}

fn calculator(args: &Value) -> ToolOutcome {
    let Some(expr) = arg_str(args, "expression") else {
        return ToolOutcome::err("missing 'expression'");
    };
    match evaluate(expr) {
        Ok(v) => ToolOutcome::ok(format!("{expr} = {v}")),
        Err(e) => ToolOutcome::err(e),
    }
}

fn datetime(args: &Value) -> ToolOutcome {
    let utc = args.get("utc").and_then(|v| v.as_bool()).unwrap_or(true);
    let now = chrono::Utc::now();
    if utc {
        ToolOutcome::ok(
            json!({
                "utc": now.to_rfc3339(),
                "unix_ms": now.timestamp_millis(),
                "unix": now.timestamp(),
                "weekday": now.format("%A").to_string(),
            })
            .to_string(),
        )
    } else {
        let local = chrono::Local::now();
        ToolOutcome::ok(
            json!({
                "local": local.to_rfc3339(),
                "utc": now.to_rfc3339(),
                "unix_ms": now.timestamp_millis(),
            })
            .to_string(),
        )
    }
}

fn memory_store(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let Some(text) = arg_str(args, "text") else {
        return ToolOutcome::err("missing 'text'");
    };
    let m = Memory {
        id: new_id(),
        text: text.to_string(),
        created_at: now_ms(),
        source_conversation: None,
    };
    match ctx.store.put_memory(&m) {
        Ok(_) => ToolOutcome::ok(format!("stored memory {}", m.id)),
        Err(e) => ToolOutcome::err(format!("store failed: {e}")),
    }
}

fn memory_recall(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let query = arg_str(args, "query").unwrap_or("").to_lowercase();
    match ctx.store.memories() {
        Ok(all) => {
            let filtered: Vec<&Memory> = all
                .iter()
                .filter(|m| query.is_empty() || m.text.to_lowercase().contains(&query))
                .take(50)
                .collect();
            if filtered.is_empty() {
                return ToolOutcome::ok("no memories stored");
            }
            ToolOutcome::ok(
                filtered
                    .iter()
                    .map(|m| format!("- {}", m.text))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        Err(e) => ToolOutcome::err(format!("recall failed: {e}")),
    }
}

fn read_file(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let rel = arg_str(args, "path").unwrap_or(".");
    let path = match sandbox_path(&ctx.workspace, rel) {
        Ok(p) => p,
        Err(e) => return ToolOutcome::err(e),
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > 2_000_000 {
                return ToolOutcome::err("file larger than 2 MB");
            }
            match String::from_utf8(bytes) {
                Ok(text) => ToolOutcome::ok(text.chars().take(50_000).collect::<String>()),
                Err(_) => ToolOutcome::err("file is not valid UTF-8 text"),
            }
        }
        Err(e) => ToolOutcome::err(format!("read failed: {e}")),
    }
}

fn list_dir(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let rel = arg_str(args, "path").unwrap_or(".");
    let path = match sandbox_path(&ctx.workspace, rel) {
        Ok(p) => p,
        Err(e) => return ToolOutcome::err(e),
    };
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut lines = Vec::new();
            for e in entries.flatten().take(500) {
                let kind = if e.path().is_dir() { "dir " } else { "file" };
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                lines.push(format!(
                    "{kind} {:>10}  {}",
                    size,
                    e.file_name().to_string_lossy()
                ));
            }
            if lines.is_empty() {
                ToolOutcome::ok("(empty directory)")
            } else {
                ToolOutcome::ok(lines.join("\n"))
            }
        }
        Err(e) => ToolOutcome::err(format!("list failed: {e}")),
    }
}

fn write_file(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let Some(rel) = arg_str(args, "path") else {
        return ToolOutcome::err("missing 'path'");
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let path = match sandbox_path(&ctx.workspace, rel) {
        Ok(p) => p,
        Err(e) => return ToolOutcome::err(e),
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolOutcome::err(format!("creating parent dirs: {e}"));
        }
    }
    match std::fs::write(&path, content) {
        Ok(_) => ToolOutcome::ok(format!("wrote {} bytes to {rel}", content.len())),
        Err(e) => ToolOutcome::err(format!("write failed: {e}")),
    }
}

async fn http_request(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    let Some(url) = arg_str(args, "url") else {
        return ToolOutcome::err("missing 'url'");
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return ToolOutcome::err("only http(s) URLs are allowed");
    }
    let method = arg_str(args, "method").unwrap_or("GET").to_uppercase();
    let method = match reqwest::Method::from_bytes(method.as_bytes()) {
        Ok(m) => m,
        Err(_) => return ToolOutcome::err("invalid HTTP method"),
    };
    let mut builder = ctx.http.request(method, url);
    if let Some(headers) = args.get("headers").and_then(|h| h.as_object()) {
        for (k, v) in headers {
            if let Some(v) = v.as_str() {
                builder = builder.header(k.as_str(), v);
            }
        }
    }
    if let Some(body) = args.get("body").and_then(|b| b.as_str()) {
        builder = builder.body(body.to_string());
    }
    match builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(t) => ToolOutcome::ok(format!(
                    "HTTP {status}\n\n{}",
                    t.chars().take(20_000).collect::<String>()
                )),
                Err(e) => ToolOutcome::err(format!("reading body: {e}")),
            }
        }
        Err(e) => ToolOutcome::err(format!("request failed: {e}")),
    }
}

async fn run_shell(args: &Value, ctx: &ToolCtx) -> ToolOutcome {
    if !ctx.settings.host.allow_shell {
        return ToolOutcome::err("shell execution is disabled on this host");
    }
    let Some(command) = arg_str(args, "command") else {
        return ToolOutcome::err("missing 'command'");
    };
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let flag = if cfg!(windows) { "/C" } else { "-c" };
    let output = tokio::process::Command::new(shell)
        .arg(flag)
        .arg(command)
        .current_dir(&ctx.workspace)
        .output()
        .await;
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let body = format!(
                "exit: {}\nstdout:\n{}\nstderr:\n{}",
                o.status.code().unwrap_or(-1),
                stdout.chars().take(10_000).collect::<String>(),
                stderr.chars().take(4_000).collect::<String>()
            );
            if o.status.success() {
                ToolOutcome::ok(body)
            } else {
                ToolOutcome::err(body)
            }
        }
        Err(e) => ToolOutcome::err(format!("spawn failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_precedence_and_parens() {
        assert_eq!(evaluate("2+3*4").unwrap(), 14.0);
        assert_eq!(evaluate("(2+3)*4").unwrap(), 20.0);
        assert_eq!(evaluate("2^3^2").unwrap(), 512.0);
        assert_eq!(evaluate("10/4").unwrap(), 2.5);
        assert_eq!(evaluate("10%3").unwrap(), 1.0);
    }

    #[test]
    fn unary_minus_and_functions() {
        assert_eq!(evaluate("-5+3").unwrap(), -2.0);
        assert_eq!(evaluate("sqrt(16)").unwrap(), 4.0);
        assert_eq!(evaluate("abs(-7)").unwrap(), 7.0);
        assert!((evaluate("sin(pi/2)").unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(evaluate("2*-3").unwrap(), -6.0);
    }

    #[test]
    fn rejects_dangerous_or_malformed_input() {
        assert!(evaluate("1/0").is_err());
        assert!(evaluate("sqrt(-1)").is_err());
        assert!(evaluate("2+").is_err());
        assert!(evaluate("(1+2").is_err());
        assert!(evaluate("1+2)").is_err());
        assert!(evaluate("system('rm -rf /')").is_err());
        assert!(evaluate("__import__('os')").is_err());
        assert!(evaluate("").is_err());
        assert!(evaluate("1e3").unwrap() == 1000.0);
    }

    #[test]
    fn sandbox_blocks_traversal_and_absolute_paths() {
        let ws = Path::new("/tmp/workspace");
        assert!(sandbox_path(ws, "../etc/passwd").is_err());
        assert!(sandbox_path(ws, "/etc/passwd").is_err());
        assert!(sandbox_path(ws, "a/../../b").is_err());
        assert!(sandbox_path(ws, "~/secret").is_err());
        assert_eq!(
            sandbox_path(ws, "notes/a.txt").unwrap(),
            ws.join("notes/a.txt")
        );
        assert_eq!(sandbox_path(ws, "").unwrap(), ws.to_path_buf());
    }

    #[test]
    fn html_to_text_drops_scripts_and_keeps_structure() {
        let html = "<html><head><style>x{}</style></head><body><script>evil()</script><h1>Title</h1><p>Hello &amp; welcome</p><p>Second</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello & welcome"));
        assert!(!text.contains("evil"));
        assert!(text.contains("Second"));
    }

    #[test]
    fn ddg_redirect_href_is_decoded() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&rut=x";
        assert_eq!(decode_ddg_href(href), "https://example.com/a?b=1");
        assert_eq!(
            decode_ddg_href("https://direct.example"),
            "https://direct.example"
        );
    }

    #[test]
    fn specs_respect_enabled_list_and_shell_gate() {
        let enabled = vec!["calculator".to_string(), "run_shell".to_string()];
        let names: Vec<_> = specs(&enabled, false).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["calculator"]);
        let names: Vec<_> = specs(&enabled, true).into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"run_shell".to_string()));
    }

    #[test]
    fn risk_classification() {
        assert_eq!(risk_of("calculator"), Risk::Low);
        assert_eq!(risk_of("web_search"), Risk::Medium);
        assert_eq!(risk_of("write_file"), Risk::High);
        assert_eq!(risk_of("nonexistent"), Risk::High);
    }
}
