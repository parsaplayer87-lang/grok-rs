//! JNI bridge for the Android shell. The Kotlin layer stays thin: it starts the
//! Rust server on loopback and talks to the same HTTP API a self-hosted box uses.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};
use jni::JNIEnv;

use grok_core::Engine;
use grok_server::ServerHandle;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static ENGINE: OnceLock<Mutex<Option<Engine>>> = OnceLock::new();
static HANDLE: OnceLock<Mutex<Option<ServerHandle>>> = OnceLock::new();
static LAST_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn engine_slot() -> &'static Mutex<Option<Engine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn handle_slot() -> &'static Mutex<Option<ServerHandle>> {
    HANDLE.get_or_init(|| Mutex::new(None))
}

fn error_slot() -> &'static Mutex<String> {
    LAST_ERROR.get_or_init(|| Mutex::new(String::new()))
}

fn set_error(msg: String) {
    if let Ok(mut g) = error_slot().lock() {
        *g = msg;
    }
}

#[cfg(target_os = "android")]
fn log(msg: &str) {
    extern "C" {
        fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
    }
    let tag = b"grok-rs\0";
    let fmt = b"%s\0";
    let c = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        __android_log_print(4, tag.as_ptr(), fmt.as_ptr(), c.as_ptr());
    }
}

#[cfg(not(target_os = "android"))]
fn log(msg: &str) {
    eprintln!("[grok-rs] {msg}");
}

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(|v| v.into()).unwrap_or_default()
}

/// Returns the bound port, or a negative code on failure. Port 0 lets the OS pick
/// a free port so the app never collides with another service on the device.
#[no_mangle]
pub extern "system" fn Java_dev_grokrs_app_NativeBridge_startServer(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    bind: JString,
    port: jint,
    token: JString,
) -> jint {
    let dir = jstr(&mut env, &data_dir);
    let bind = jstr(&mut env, &bind);
    let token = jstr(&mut env, &token);
    if dir.is_empty() {
        set_error("dataDir is empty".into());
        return -1;
    }
    log(&format!(
        "starting server dir={dir} bind={bind} port={port}"
    ));

    let result = runtime().block_on(async {
        let engine = Engine::open(&PathBuf::from(&dir))?;
        if !token.is_empty() {
            let mut s = engine.settings();
            s.host.token = token;
            engine.update_settings(s)?;
        }
        let handle = grok_server::serve(engine.clone(), &bind, port as u16).await?;
        Ok::<_, anyhow::Error>((engine, handle))
    });

    match result {
        Ok((engine, handle)) => {
            let actual = handle.local_addr.port() as jint;
            if let Ok(mut g) = engine_slot().lock() {
                *g = Some(engine);
            }
            if let Ok(mut g) = handle_slot().lock() {
                if let Some(old) = g.take() {
                    old.stop();
                }
                *g = Some(handle);
            }
            set_error(String::new());
            log(&format!("server listening on port {actual}"));
            actual
        }
        Err(e) => {
            set_error(e.to_string());
            log(&format!("start failed: {e}"));
            -2
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_grokrs_app_NativeBridge_stopServer(_env: JNIEnv, _class: JClass) {
    if let Ok(mut g) = handle_slot().lock() {
        if let Some(h) = g.take() {
            h.stop();
        }
    }
    if let Ok(g) = engine_slot().lock() {
        if let Some(e) = g.as_ref() {
            let approvals = e.approvals.clone();
            runtime().spawn(async move { approvals.deny_all().await });
        }
    }
    log("server stopped");
}

#[no_mangle]
pub extern "system" fn Java_dev_grokrs_app_NativeBridge_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let v = env!("CARGO_PKG_VERSION");
    env.new_string(v)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_dev_grokrs_app_NativeBridge_lastError(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let msg = error_slot().lock().map(|g| g.clone()).unwrap_or_default();
    env.new_string(msg)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// LAN reachability probe so the shell can show the user the exact URL to open on
/// another device without guessing which interface is up.
#[no_mangle]
pub extern "system" fn Java_dev_grokrs_app_NativeBridge_lanAddress(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let addr = runtime()
        .block_on(async {
            let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
            socket.connect("8.8.8.8:80").await.ok()?;
            socket.local_addr().ok().map(|a| a.ip().to_string())
        })
        .unwrap_or_default();
    env.new_string(addr)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
