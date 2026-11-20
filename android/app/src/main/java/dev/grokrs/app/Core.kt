package dev.grokrs.app

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

data class CoreStatus(
    val running: Boolean = false,
    val port: Int = 0,
    val lanUrl: String = "",
    val token: String = "",
    val version: String = "",
    val error: String = "",
)

/**
 * Owns the embedded Rust server. Starts on loopback first (trusted, no token
 * needed), reads the persisted host settings, then rebinds to the LAN only if the
 * user asked for phone-as-host mode.
 */
object Core {
    val api = ApiClient("http://127.0.0.1:1")

    private val _status = MutableStateFlow(CoreStatus())
    val status: StateFlow<CoreStatus> = _status.asStateFlow()

    private val gate = Mutex()
    private var dataDir: String = ""

    suspend fun ensureStarted(context: Context): CoreStatus = gate.withLock {
        if (_status.value.running) return@withLock _status.value
        withContext(Dispatchers.IO) {
            val dir = File(context.filesDir, "core").apply { mkdirs() }
            dataDir = dir.absolutePath
            try {
                val version = NativeBridge.version()
                val port = NativeBridge.startServer(dataDir, "127.0.0.1", 0, "")
                if (port <= 0) {
                    val msg = NativeBridge.lastError().ifEmpty { "startServer returned $port" }
                    _status.value = CoreStatus(error = msg, version = version)
                    return@withContext _status.value
                }
                api.baseUrl = "http://127.0.0.1:$port"
                api.token = null

                val host = runCatching { api.hostInfo() }.getOrNull()
                var boundPort = port
                var lanUrl = ""
                if (host != null && host.allowLan) {
                    val lanPort = NativeBridge.startServer(dataDir, "0.0.0.0", host.port, "")
                    if (lanPort > 0) {
                        boundPort = lanPort
                        api.baseUrl = "http://127.0.0.1:$lanPort"
                        val ip = NativeBridge.lanAddress()
                        if (ip.isNotEmpty()) lanUrl = "http://$ip:$lanPort"
                    }
                }
                _status.value = CoreStatus(
                    running = true,
                    port = boundPort,
                    lanUrl = lanUrl,
                    token = host?.token ?: "",
                    version = version,
                )
            } catch (t: Throwable) {
                _status.value = CoreStatus(error = t.message ?: t.javaClass.simpleName)
            }
            _status.value
        }
    }

    /** Rebind after the host toggle changed; the core stops the previous listener itself. */
    suspend fun rebind(): CoreStatus = gate.withLock {
        withContext(Dispatchers.IO) {
            if (dataDir.isEmpty()) return@withContext _status.value
            try {
                val host = api.hostInfo()
                val bind = if (host.allowLan) "0.0.0.0" else "127.0.0.1"
                val port = NativeBridge.startServer(dataDir, bind, if (host.allowLan) host.port else 0, "")
                if (port > 0) {
                    api.baseUrl = "http://127.0.0.1:$port"
                    val ip = if (host.allowLan) NativeBridge.lanAddress() else ""
                    _status.value = _status.value.copy(
                        running = true,
                        port = port,
                        lanUrl = if (ip.isNotEmpty()) "http://$ip:$port" else "",
                        token = host.token,
                        error = "",
                    )
                }
            } catch (t: Throwable) {
                _status.value = _status.value.copy(error = t.message ?: "rebind failed")
            }
            _status.value
        }
    }

    fun stop() {
        runCatching { NativeBridge.stopServer() }
        _status.value = CoreStatus(version = _status.value.version)
    }
}