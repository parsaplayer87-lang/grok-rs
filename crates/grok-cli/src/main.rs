use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use grok_core::{ChatRequest, Engine, StreamEvent};

const HELP: &str = "\
grok-rs — bring-your-own-endpoint AI client with an agent host

USAGE:
  grok-rs serve [--port N] [--lan] [--data-dir DIR] [--token T]
  grok-rs chat <message> [--model M] [--agent] [--reasoning] [--search] [--conversation ID]
  grok-rs models [--provider ID]
  grok-rs config show
  grok-rs config set <base_url|api_key|model|language|system_prompt> <value>
  grok-rs config preset <name>
  grok-rs conversations
  grok-rs export <conversation-id>
  grok-rs presets

Serve prints a URL you can open in any browser. With --lan the phone or box becomes
an agent host for other devices; they must send the printed token as a Bearer header.
";

fn data_dir(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--data-dir") {
        if let Some(v) = args.get(i + 1) {
            return PathBuf::from(v);
        }
    }
    if let Ok(v) = std::env::var("GROK_RS_DATA_DIR") {
        return PathBuf::from(v);
    }
    let base = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(".grok-rs")
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") {
            if matches!(
                a.as_str(),
                "--port" | "--data-dir" | "--token" | "--model" | "--provider" | "--conversation"
            ) {
                skip = true;
            }
            continue;
        }
        out.push(a.clone());
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let pos = positional(&args);
    let cmd = pos.first().map(|s| s.as_str()).unwrap_or("serve");
    let dir = data_dir(&args);
    let engine = Engine::open(&dir)?;

    match cmd {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        "serve" => {
            let mut s = engine.settings();
            let port = opt(&args, "--port").or_else(|| std::env::var("GROK_RS_PORT").ok());
            if let Some(p) = port {
                s.host.port = p.parse().unwrap_or(8787);
            }
            let lan_env = std::env::var("GROK_RS_LAN")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            if flag(&args, "--lan") || lan_env {
                s.host.allow_lan = true;
            }
            if let Some(t) = opt(&args, "--token").or_else(|| std::env::var("GROK_RS_TOKEN").ok()) {
                s.host.token = t;
            }
            engine.update_settings(s.clone())?;
            let bind = grok_server::bind_address(&s);
            let handle = grok_server::serve(engine.clone(), bind, s.host.port).await?;
            let port = handle.local_addr.port();
            println!(
                "grok-rs listening on http://{}:{port}",
                handle.local_addr.ip()
            );
            if s.host.allow_lan {
                println!(
                    "LAN mode: other devices must send  Authorization: Bearer {}",
                    s.host.token
                );
            }
            println!("data dir: {}", dir.display());
            println!("Ctrl+C to stop.");
            tokio::signal::ctrl_c().await?;
            handle.stop();
            Ok(())
        }
        "chat" => {
            let message = pos
                .get(1)
                .cloned()
                .ok_or_else(|| anyhow!("chat needs a message"))?;
            let req = ChatRequest {
                conversation_id: opt(&args, "--conversation"),
                message,
                model: opt(&args, "--model"),
                provider_id: opt(&args, "--provider"),
                agent: flag(&args, "--agent"),
                reasoning: flag(&args, "--reasoning"),
                search: flag(&args, "--search"),
                ..Default::default()
            };
            let mut stream = Box::pin(engine.chat(req));
            let mut stdout = std::io::stdout();
            while let Some(ev) = stream.next().await {
                match ev {
                    StreamEvent::Delta { text } => {
                        print!("{text}");
                        let _ = stdout.flush();
                    }
                    StreamEvent::Reasoning { text } => {
                        eprint!("\x1b[2m{text}\x1b[0m");
                    }
                    StreamEvent::ToolCall { call } => {
                        eprintln!("\n[tool] {} {}", call.name, call.arguments);
                    }
                    StreamEvent::ToolResult { result } => {
                        eprintln!(
                            "[{}] {} ({}ms)",
                            if result.ok { "ok" } else { "err" },
                            result.name,
                            result.duration_ms
                        );
                    }
                    StreamEvent::ApprovalRequired {
                        approval_id,
                        call,
                        risk,
                    } => {
                        eprintln!(
                            "\nApprove {} (risk {risk})? {} [y/N]",
                            call.name, call.arguments
                        );
                        let mut line = String::new();
                        std::io::stdin().read_line(&mut line)?;
                        let yes = line.trim().eq_ignore_ascii_case("y");
                        engine.approvals.resolve(&approval_id, yes).await;
                    }
                    StreamEvent::Error { message } => eprintln!("\nerror: {message}"),
                    StreamEvent::Usage { usage } => eprintln!("\n[{} tokens]", usage.total_tokens),
                    StreamEvent::Done { .. } => println!(),
                    _ => {}
                }
            }
            Ok(())
        }
        "models" => {
            let models = engine
                .list_models(opt(&args, "--provider").as_deref())
                .await?;
            for m in models {
                println!("{m}");
            }
            Ok(())
        }
        "presets" => {
            for (name, kind, url, model) in grok_core::config::presets() {
                println!("{name:22} {url:52} {model}  ({kind:?})");
            }
            Ok(())
        }
        "config" => {
            let sub = pos.get(1).map(|s| s.as_str()).unwrap_or("show");
            let mut s = engine.settings();
            match sub {
                "show" => {
                    println!("{}", serde_json::to_string_pretty(&s.redacted())?);
                }
                "preset" => {
                    let name = pos.get(2).ok_or_else(|| anyhow!("preset needs a name"))?;
                    let p = grok_core::config::presets()
                        .into_iter()
                        .find(|(n, _, _, _)| n.eq_ignore_ascii_case(name))
                        .ok_or_else(|| anyhow!("unknown preset '{name}'"))?;
                    let provider = grok_core::Provider::new(p.0, p.1, p.2, p.3);
                    s.active_provider = provider.id.clone();
                    s.providers.push(provider);
                    engine.update_settings(s)?;
                    println!("added preset {}", p.0);
                }
                "set" => {
                    let key = pos.get(2).ok_or_else(|| anyhow!("set needs a key"))?;
                    let value = pos.get(3).cloned().unwrap_or_default();
                    let provider = s
                        .providers
                        .iter_mut()
                        .find(|p| p.id == s.active_provider)
                        .ok_or_else(|| anyhow!("no active provider"))?;
                    match key.as_str() {
                        "base_url" => provider.base_url = value,
                        "api_key" => provider.api_key = value,
                        "model" => provider.model = value,
                        "language" => s.language = value,
                        "system_prompt" => s.system_prompt = value,
                        _ => return Err(anyhow!("unknown key '{key}'")),
                    }
                    engine.update_settings(s)?;
                    println!("set {key}");
                }
                _ => return Err(anyhow!("unknown config subcommand '{sub}'")),
            }
            Ok(())
        }
        "conversations" => {
            for c in engine.conversations()? {
                println!(
                    "{}  {}  {}",
                    c.id,
                    if c.starred { "★" } else { " " },
                    c.title
                );
            }
            Ok(())
        }
        "export" => {
            let id = pos
                .get(1)
                .ok_or_else(|| anyhow!("export needs a conversation id"))?;
            println!("{}", serde_json::to_string_pretty(&engine.export(id)?)?);
            Ok(())
        }
        _ => Err(anyhow!("unknown command '{cmd}'\n\n{HELP}")),
    }
}
