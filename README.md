# grok-rs

A lightweight, independent Grok-style AI client with a **Rust core** and a **native Android UI**.
Bring your own endpoint: any base URL, any API key, any model. Run the agent on your own server,
or on the phone itself.

> Not affiliated with, endorsed by, or built from xAI's code. `grok-rs` is an independent client
> that talks to **public, documented OpenAI-compatible and Anthropic-compatible APIs** using your
> own credentials. It does not use grok.com's private cookie-authenticated API and does not attempt
> to bypass authentication, rate limits or paywalls. See [docs/RE_NOTES.md](docs/RE_NOTES.md) for
> exactly what was reverse engineered and what was deliberately not copied.

---

## Download the APK

Every push to `main` builds an arm64-v8a release APK in GitHub Actions.

- **Direct download: [grok-rs-arm64.apk](https://github.com/parsaplayer87-lang/grok-rs/releases/download/latest/grok-rs-arm64.apk)** (~7.4 MB)
- Release page: [Latest build](https://github.com/parsaplayer87-lang/grok-rs/releases/tag/latest)
- Or: **Actions → Android APK → latest run → Artifacts → `grok-rs-apk`**

The signing key is cached across runs, so a new build installs **over** the old one — no uninstall.
arm64-v8a only (every mainstream phone since 2017), R8-minified, resource-shrunk, `en` + `fa` only.

Requires Android 8.0 (API 26) or newer.

---

## Three ways to run it

| Mode | Where the agent runs | How you reach it |
|---|---|---|
| **On-device** | Inside the app, as an embedded Rust server on loopback | Just open the app |
| **Phone as host** | Inside the app, bound to your LAN | Other devices open `http://<phone-ip>:8787` with the host token |
| **Your own server** | Docker / bare metal | Browser, or point the app at it in Settings → Remote server |

All three speak **the same HTTP API**, so there is one implementation, not three.

### Your own server

```bash
docker compose up -d
docker logs grok-rs        # prints the LAN URL and the Bearer token
```

Or without Docker:

```bash
cargo run --release --bin grok-rs -- serve --lan --port 8787
```

Then open `http://<host>:8787` — a complete, dependency-free web client is served by the Rust binary
itself. No Node, no npm, no static asset pipeline.

Environment variables: `GROK_RS_DATA_DIR`, `GROK_RS_PORT`, `GROK_RS_LAN`, `GROK_RS_TOKEN`, `RUST_LOG`.

### Phone as host

Settings → **Use this phone as host**. The core rebinds to `0.0.0.0`, and the notification and
Settings screen show the exact LAN URL and token. A foreground service keeps the agent alive while
a turn is streaming or the screen is off.

Loopback callers are trusted (that is the app itself). **Every LAN caller must present the host
token**, compared in constant time.

---

## Bring your own endpoint

Settings accepts any combination of:

- **Base URL** — `https://api.x.ai/v1`, `https://openrouter.ai/api/v1`, `http://192.168.1.50:11434/v1`, anything
- **API key** — stored locally, redacted to `••••last4` in every API response, never logged
- **Model** — free text, plus a **Fetch models** button that lists what the endpoint actually offers
- **API type** — OpenAI-compatible or Anthropic-compatible

14 presets ship built in: xAI, OpenAI, OpenRouter, Anthropic, Groq, Together, Mistral, DeepSeek,
Gemini, Ollama, LM Studio, llama.cpp, vLLM, Custom. Multiple providers can be configured and switched.

A bare host like `https://api.openai.com` is normalised to `/v1` automatically; an explicit path is
left alone.

---

## Features

**Grok-style surface** (recovered from the web client's own contract, see RE notes):

- Streaming answers with a **separate reasoning channel** — thinking is collapsible, not mixed into the reply
- **Response tree**, not a flat list: regenerate creates a *sibling* branch, history is never mutated
- **Web search with split citations** — all results vs. the ones actually cited
- **Image generation** and image attachments (vision)
- **Agent steps** and tool-usage cards, streamed as they happen
- Temporary (unsaved) chats, personas, custom instructions, starred conversations
- Resumable streams: the assistant placeholder is persisted **before the first token**, so a dropped
  connection still leaves a recoverable response id

**Beyond the original:**

- **Any provider**, not one vendor — including fully local models on your own hardware
- **Real agent loop** with 11 tools: web search, URL fetch, calculator, datetime, memory store/recall,
  sandboxed file read/write/list, HTTP request, and opt-in shell
- **Per-tool risk tiers and an approval gate** — `auto` / `ask` / `deny`; low-risk tools pass in `ask`
  mode, high-risk tools always prompt; an unanswered prompt times out to **deny**
- **Persistent memory** the model can write to and recall across conversations
- **Full-text search** across every stored message
- **Three deployment modes** from one binary
- **Persian / RTL** first-class: UI, system prompt and storage all handle RTL and multi-byte text
- **Markdown rendering** with copyable code blocks, citations as tappable links
- Voice input via the system recogniser, share-to-app, file/image attachments, JSON export
- **No telemetry. No accounts. No analytics.** Everything lives in a local embedded database.

---

## Why it is small

The dependency tree is **pure Rust — zero C**:

| Need | Choice | Instead of |
|---|---|---|
| Storage | `redb` (embedded ACID KV) | SQLite bundled (needs a C compiler) |
| TLS crypto | `rustls` + `rustls-rustcrypto` | `ring` / `aws-lc-rs` (C + asm) |
| TLS roots | bundled Mozilla roots (`webpki-roots`) | platform verifier (needs a Java class on Android) |
| Android transport | `HttpURLConnection` | OkHttp + Okio |
| Android images | small bounded `LruCache` loader | Coil / Glide |
| Android icons | `material-icons-core` | `material-icons-extended` |
| Web UI | one hand-written HTML file | a JS framework + build pipeline |

Release profile: `opt-level = "z"`, LTO, `codegen-units = 1`, `panic = "abort"`, stripped symbols.
Android: arm64-v8a only, R8 minify + resource shrink, `en`/`fa` resources only, uncompressed JNI libs.

---

## Layout

```
crates/
  grok-core/     types, config, redb store, provider streaming, tools, agent loop, engine
  grok-server/   axum HTTP API + SSE + the embedded web client
  grok-cli/      grok-rs binary: serve / chat / models / config / conversations / export
  grok-ffi/      JNI cdylib -> libgrok_rs.so
android/         Kotlin + Jetpack Compose shell (thin: it speaks the same HTTP API)
docs/RE_NOTES.md what was reverse engineered, with evidence
```

The Kotlin layer is deliberately thin. `NativeBridge` starts the Rust server; everything else is
HTTP. That is why on-device, phone-as-host and remote-server modes need no separate code paths.

---

## Build

### APK (what CI does)

```bash
rustup target add aarch64-linux-android
cargo install cargo-ndk
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p grok-ffi
cd android && ./gradlew assembleRelease
```

Requires JDK 17, Android SDK 35, NDK 27.1.12297006, Gradle 8.9 (the wrapper fetches it).

### Desktop / server

```bash
cargo build --release --bin grok-rs
./target/release/grok-rs serve
./target/release/grok-rs chat "سلام" --agent --search
./target/release/grok-rs config preset OpenRouter
./target/release/grok-rs models
```

### Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Security

- API keys are redacted in `GET /api/settings`; echoing a redacted key back on `PUT` preserves the
  stored secret instead of overwriting it with `••••`.
- LAN requests require `Authorization: Bearer <host token>`, compared in constant time. Loopback is
  trusted because it is the app itself.
- File tools are sandboxed to the workspace: absolute paths, `~` and `..` are rejected.
- `run_shell` is **off** unless you explicitly enable it, and is marked high-risk.
- The calculator is a real tokenizer + shunting-yard evaluator. There is no `eval` anywhere.
- High-risk tools always require approval in `ask` mode; approval timeouts deny.
- No telemetry, no crash reporting, no third-party analytics.

**Self-hosted TLS:** the core trusts the bundled Mozilla root store. Put your server behind a
publicly-trusted certificate (Caddy, Let's Encrypt, a reverse proxy) rather than a self-signed one.

---

## Status and limitations

Being explicit about what has and has not been verified.

**Verified in CI and by inspecting the published artifact:**

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace --all-targets` all pass. **44 tests, 0 failures** (40 core, 4 server),
  covering the response-tree walk, multi-byte/Persian storage and titles, both providers' SSE
  parsing, tool-call fragment reassembly, approval timeouts, path-sandbox rejection, calculator
  injection rejection, key redaction and the loopback/LAN auth boundary.
- The core **cross-compiles to `aarch64-linux-android` with no C toolchain for crypto**. Its
  dependency graph contains no `ring`, `aws-lc`, `openssl`, `sqlite` or `cc` — only `libc`/`jni-sys`
  FFI declarations.
- The released APK is **7,763,257 bytes**, a well-formed ZIP with a single R8-minified
  `classes.dex`, and `lib/arm64-v8a/libgrok_rs.so` is a valid ELF64 AArch64 object.
- All five `Java_dev_grokrs_app_NativeBridge_*` symbols are **exported and defined** in that
  library, matching the Kotlin bridge one-for-one, so `System.loadLibrary` resolves them.
- The library's only `DT_NEEDED` entries are `libdl.so`, `liblog.so`, `libm.so`, `libc.so` — all
  standard Android system libraries. Nothing extra ships or needs installing.
- The server image builds and publishes to `ghcr.io/parsaplayer87-lang/grok-rs`.

**Not verified:**

- The **Android UI has never run on a device or emulator** — no Android SDK was available locally.
  Compiling, packaging and JNI linkage are confirmed; layout, navigation and streaming behaviour on
  a real screen are not. Treat the first install as a build verification, not a UI sign-off.
- Live streaming against a real paid endpoint was not exercised; the SSE parsers are covered by
  fixture-based unit tests for both OpenAI and Anthropic wire formats.
- Tests were not run on the Windows development machine, which has no `dlltool` for the
  `x86_64-pc-windows-gnu` target that `getrandom`/`windows-sys` need to link. CI runs them on Linux.
- Phone-as-host uses a `dataSync` foreground service. Android 15 caps that type at roughly six
  cumulative hours per 24-hour period, after which the system stops it. For an always-on agent,
  run the server mode on a machine you control and point the app at it.

## License

MIT — see [LICENSE](LICENSE).