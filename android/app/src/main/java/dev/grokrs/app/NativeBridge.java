package dev.grokrs.app;

/**
 * JNI surface over libgrok_rs.so. Static natives so the symbols match the
 * Java_dev_grokrs_app_NativeBridge_* exports in crates/grok-ffi.
 *
 * The Rust core runs an HTTP server on loopback; everything above this class
 * speaks that same API, which is why on-device, phone-as-host and self-hosted
 * modes share one code path.
 */
public final class NativeBridge {
    static {
        System.loadLibrary("grok_rs");
    }

    private NativeBridge() {
    }

    /** Returns the bound port, or -1 for an empty data dir, -2 on failure. */
    public static native int startServer(String dataDir, String bind, int port, String token);

    public static native void stopServer();

    public static native String version();

    public static native String lastError();

    public static native String lanAddress();
}