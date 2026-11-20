package dev.grokrs.app

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL

class ApiException(val status: Int, message: String) : Exception(message)

/**
 * Speaks the Rust core's HTTP API. Deliberately built on HttpURLConnection rather
 * than OkHttp: the core already does all the heavy lifting, so the shell carries
 * no extra transport dependency and the APK stays small.
 */
class ApiClient(
    @Volatile var baseUrl: String,
    @Volatile var token: String? = null,
) {
    val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        explicitNulls = false
        isLenient = true
    }

    private fun open(method: String, path: String): HttpURLConnection {
        val conn = URL(baseUrl.trimEnd('/') + path).openConnection() as HttpURLConnection
        conn.requestMethod = method
        conn.connectTimeout = 20_000
        conn.readTimeout = 180_000
        conn.setRequestProperty("Accept", "application/json")
        token?.takeIf { it.isNotEmpty() }?.let { conn.setRequestProperty("Authorization", "Bearer $it") }
        return conn
    }

    private fun writeBody(conn: HttpURLConnection, body: String) {
        conn.doOutput = true
        conn.setRequestProperty("Content-Type", "application/json; charset=utf-8")
        conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
    }

    private fun readAll(conn: HttpURLConnection): String {
        val code = conn.responseCode
        val stream = if (code >= 400) conn.errorStream ?: conn.inputStream else conn.inputStream
        val text = stream?.use { it.readBytes().toString(Charsets.UTF_8) } ?: ""
        if (code >= 400) {
            val detail = runCatching { json.decodeFromString<ErrorResponse>(text).error }
                .getOrNull()
                ?.takeIf { it.isNotEmpty() }
                ?: text.ifEmpty { "HTTP $code" }
            throw ApiException(code, detail)
        }
        return text
    }

    private suspend fun raw(method: String, path: String, body: String? = null): String =
        withContext(Dispatchers.IO) {
            val conn = open(method, path)
            try {
                if (body != null) writeBody(conn, body)
                coroutineContext.ensureActive()
                readAll(conn)
            } finally {
                conn.disconnect()
            }
        }

    suspend fun health(): Health = json.decodeFromString(raw("GET", "/api/health"))

    suspend fun settings(): Settings = json.decodeFromString(raw("GET", "/api/settings"))

    suspend fun putSettings(settings: Settings): Settings {
        val resp: SettingsResponse = json.decodeFromString(raw("PUT", "/api/settings", json.encodeToString(settings)))
        return resp.settings
    }

    suspend fun presets(): PresetsResponse = json.decodeFromString(raw("GET", "/api/presets"))

    suspend fun models(providerId: String? = null): List<String> {
        val path = if (providerId.isNullOrEmpty()) "/api/models"
        else "/api/models?provider_id=" + java.net.URLEncoder.encode(providerId, "UTF-8")
        return json.decodeFromString<ModelsResponse>(raw("GET", path)).models
    }

    suspend fun tools(): ToolsResponse = json.decodeFromString(raw("GET", "/api/tools"))

    suspend fun conversations(): List<Conversation> =
        json.decodeFromString<ConversationList>(raw("GET", "/api/conversations")).conversations

    suspend fun createConversation(title: String? = null, temporary: Boolean = false): Conversation {
        val body = buildJsonObject {
            if (title != null) put("title", title)
            put("temporary", temporary)
        }.toString()
        return json.decodeFromString(raw("POST", "/api/conversations", body))
    }

    suspend fun conversation(id: String): ConversationDetail =
        json.decodeFromString(raw("GET", "/api/conversations/$id"))

    suspend fun patchConversation(id: String, title: String? = null, starred: Boolean? = null): Conversation {
        val body = buildJsonObject {
            if (title != null) put("title", title)
            if (starred != null) put("starred", starred)
        }.toString()
        return json.decodeFromString(raw("POST", "/api/conversations/$id", body))
    }

    suspend fun deleteConversation(id: String) {
        raw("DELETE", "/api/conversations/$id")
    }

    suspend fun setLeaf(id: String, leafId: String) {
        raw("POST", "/api/conversations/$id/leaf", buildJsonObject { put("leaf_id", leafId) }.toString())
    }

    suspend fun export(id: String): String = raw("GET", "/api/conversations/$id/export")

    suspend fun memories(): List<Memory> =
        json.decodeFromString<MemoriesResponse>(raw("GET", "/api/memories")).memories

    suspend fun addMemory(text: String): Memory =
        json.decodeFromString(raw("POST", "/api/memories", buildJsonObject { put("text", text) }.toString()))

    suspend fun deleteMemory(id: String) {
        raw("DELETE", "/api/memories/$id")
    }

    suspend fun approvals(): List<PendingApproval> =
        json.decodeFromString<ApprovalsResponse>(raw("GET", "/api/approvals")).pending

    suspend fun resolveApproval(id: String, approved: Boolean) {
        raw("POST", "/api/approvals/$id", buildJsonObject { put("approved", approved) }.toString())
    }

    suspend fun images(prompt: String, count: Int = 2): List<String> {
        val body = buildJsonObject { put("prompt", prompt); put("count", count) }.toString()
        return json.decodeFromString<ImagesResponse>(raw("POST", "/api/images", body)).images
    }

    suspend fun search(query: String): List<Message> {
        val path = "/api/search?q=" + java.net.URLEncoder.encode(query, "UTF-8")
        return json.decodeFromString<SearchResponse>(raw("GET", path)).messages
    }

    suspend fun hostInfo(): HostInfo = json.decodeFromString(raw("GET", "/api/host"))

    suspend fun hostUpdate(allowLan: Boolean? = null, port: Int? = null, allowShell: Boolean? = null): HostUpdateResponse {
        val body = buildJsonObject {
            if (allowLan != null) put("allow_lan", allowLan)
            if (port != null) put("port", port)
            if (allowShell != null) put("allow_shell", allowShell)
        }.toString()
        return json.decodeFromString(raw("POST", "/api/host", body))
    }

    suspend fun stop(responseId: String) {
        runCatching { raw("POST", "/api/stop", buildJsonObject { put("response_id", responseId) }.toString()) }
    }

    /**
     * Streams a chat turn. Lines are split on raw bytes before any UTF-8 decode so
     * a multi-byte Persian or emoji character straddling two network reads is never
     * corrupted.
     */
    suspend fun chat(request: ChatRequest, onEvent: (StreamEvent) -> Unit) {
        withContext(Dispatchers.IO) {
            val conn = open("POST", "/api/chat")
            conn.setRequestProperty("Accept", "text/event-stream")
            conn.readTimeout = 300_000
            try {
                writeBody(conn, json.encodeToString(request))
                val code = conn.responseCode
                if (code >= 400) {
                    readAll(conn)
                }
                val input = conn.inputStream
                val buf = ByteArray(8192)
                val line = ByteArrayOutputStream(256)
                val data = StringBuilder()
                var finished = false
                while (!finished) {
                    coroutineContext.ensureActive()
                    val n = input.read(buf)
                    if (n < 0) break
                    for (i in 0 until n) {
                        val b = buf[i]
                        if (b == '\n'.code.toByte()) {
                            val text = line.toString("UTF-8").trimEnd('\r')
                            line.reset()
                            when {
                                text.isEmpty() -> {
                                    if (data.isNotEmpty()) {
                                        val payload = data.toString()
                                        data.setLength(0)
                                        val event = runCatching { json.decodeFromString<StreamEvent>(payload) }.getOrNull()
                                        if (event != null) {
                                            onEvent(event)
                                            if (event.type == "done" || event.type == "error") finished = true
                                        }
                                    }
                                }
                                text.startsWith("data:") -> {
                                    if (data.isNotEmpty()) data.append('\n')
                                    data.append(text.removePrefix("data:").removePrefix(" "))
                                }
                                else -> Unit
                            }
                            if (finished) break
                        } else {
                            line.write(b.toInt())
                        }
                    }
                }
            } finally {
                conn.disconnect()
            }
        }
    }
}