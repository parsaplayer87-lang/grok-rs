package dev.grokrs.app

import android.app.Application
import android.content.Intent
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class Toggles(
    val agent: Boolean = false,
    val reasoning: Boolean = false,
    val search: Boolean = false,
    val images: Boolean = false,
    val temporary: Boolean = false,
)

class AppViewModel(app: Application) : AndroidViewModel(app) {

    val core: StateFlow<CoreStatus> = Core.status

    private val _settings = MutableStateFlow<Settings?>(null)
    val settings: StateFlow<Settings?> = _settings.asStateFlow()

    private val _conversations = MutableStateFlow<List<Conversation>>(emptyList())
    val conversations: StateFlow<List<Conversation>> = _conversations.asStateFlow()

    private val _messages = MutableStateFlow<List<Message>>(emptyList())
    val messages: StateFlow<List<Message>> = _messages.asStateFlow()

    private val _live = MutableStateFlow<Message?>(null)
    val live: StateFlow<Message?> = _live.asStateFlow()

    private val _streaming = MutableStateFlow(false)
    val streaming: StateFlow<Boolean> = _streaming.asStateFlow()

    private val _approval = MutableStateFlow<PendingApproval?>(null)
    val approval: StateFlow<PendingApproval?> = _approval.asStateFlow()

    private val _notice = MutableStateFlow<String?>(null)
    val notice: StateFlow<String?> = _notice.asStateFlow()

    private val _toggles = MutableStateFlow(Toggles())
    val toggles: StateFlow<Toggles> = _toggles.asStateFlow()

    private val _models = MutableStateFlow<List<String>>(emptyList())
    val models: StateFlow<List<String>> = _models.asStateFlow()

    private val _presets = MutableStateFlow<List<Preset>>(emptyList())
    val presets: StateFlow<List<Preset>> = _presets.asStateFlow()

    private val _tools = MutableStateFlow<List<ToolInfo>>(emptyList())
    val tools: StateFlow<List<ToolInfo>> = _tools.asStateFlow()

    private val _memories = MutableStateFlow<List<Memory>>(emptyList())
    val memories: StateFlow<List<Memory>> = _memories.asStateFlow()

    private val _hits = MutableStateFlow<List<Message>>(emptyList())
    val hits: StateFlow<List<Message>> = _hits.asStateFlow()

    fun searchMessages(query: String) {
        if (query.isBlank()) {
            _hits.value = emptyList()
            return
        }
        viewModelScope.launch {
            runCatching { Core.api.search(query) }.onSuccess { _hits.value = it }
        }
    }

    private val _conversationId = MutableStateFlow<String?>(null)
    val conversationId: StateFlow<String?> = _conversationId.asStateFlow()

    private val _attachments = MutableStateFlow<List<Attachment>>(emptyList())
    val attachments: StateFlow<List<Attachment>> = _attachments.asStateFlow()

    private val prefs = app.getSharedPreferences("grok-rs", android.content.Context.MODE_PRIVATE)

    private val _remote = MutableStateFlow(prefs.getString("remote_url", "").orEmpty())
    val remote: StateFlow<String> = _remote.asStateFlow()

    private val _ready = MutableStateFlow(false)
    val ready: StateFlow<Boolean> = _ready.asStateFlow()

    private var responseId: String? = null
    private var streamJob: Job? = null
    private var loaded = false

    init {
        if (_remote.value.isNotEmpty()) {
            Core.api.baseUrl = _remote.value
            Core.api.token = prefs.getString("remote_token", "")
            viewModelScope.launch { refresh() }
        }
        viewModelScope.launch {
            Core.status.collect { status ->
                if (status.running && !loaded && _remote.value.isEmpty()) {
                    loaded = true
                    refresh()
                }
                if (!status.running) loaded = false
            }
        }
    }

    /** Third deployment mode: the phone is a thin client for a server you run yourself. */
    fun connectRemote(url: String, token: String) {
        val trimmed = url.trim().trimEnd('/')
        if (trimmed.isEmpty()) return
        prefs.edit().putString("remote_url", trimmed).putString("remote_token", token).apply()
        _remote.value = trimmed
        Core.api.baseUrl = trimmed
        Core.api.token = token.ifEmpty { null }
        loaded = true
        viewModelScope.launch { refresh() }
    }

    fun useEmbedded() {
        prefs.edit().remove("remote_url").remove("remote_token").apply()
        _remote.value = ""
        Core.api.token = null
        loaded = false
        _ready.value = false
        if (Core.status.value.running) {
            Core.api.baseUrl = "http://127.0.0.1:${Core.status.value.port}"
            loaded = true
            viewModelScope.launch { refresh() }
        }
    }

    fun notify(message: String) {
        _notice.value = message
    }

    fun clearNotice() {
        _notice.value = null
    }

    fun setToggles(transform: (Toggles) -> Toggles) {
        _toggles.value = transform(_toggles.value)
    }

    fun refresh() {
        viewModelScope.launch {
            runCatching { Core.api.settings() }.onSuccess {
                _settings.value = it
                _ready.value = true
            }.onFailure {
                _ready.value = false
                notify(it.message ?: "settings failed")
            }
            runCatching { Core.api.conversations() }.onSuccess { _conversations.value = it }
            runCatching { Core.api.presets() }.onSuccess { _presets.value = it.presets }
            runCatching { Core.api.tools() }.onSuccess { _tools.value = it.tools }
            runCatching { Core.api.memories() }.onSuccess { _memories.value = it }
            _conversationId.value?.let { openConversation(it) }
        }
    }

    fun loadModels() {
        viewModelScope.launch {
            runCatching { Core.api.models(_settings.value?.activeProvider) }
                .onSuccess { _models.value = it }
                .onFailure { notify(it.message ?: "could not list models") }
        }
    }

    fun newChat() {
        stop()
        _conversationId.value = null
        _messages.value = emptyList()
        _live.value = null
        _attachments.value = emptyList()
    }

    fun openConversation(id: String) {
        viewModelScope.launch {
            runCatching { Core.api.conversation(id) }.onSuccess { detail ->
                _conversationId.value = id
                _messages.value = detail.messages
                _live.value = null
            }.onFailure { notify(it.message ?: "could not open conversation") }
        }
    }

    fun deleteConversation(id: String) {
        viewModelScope.launch {
            runCatching { Core.api.deleteConversation(id) }
            if (_conversationId.value == id) newChat()
            runCatching { Core.api.conversations() }.onSuccess { _conversations.value = it }
        }
    }

    fun renameConversation(id: String, title: String) {
        viewModelScope.launch {
            runCatching { Core.api.patchConversation(id, title = title) }
            runCatching { Core.api.conversations() }.onSuccess { _conversations.value = it }
        }
    }

    fun starConversation(id: String, starred: Boolean) {
        viewModelScope.launch {
            runCatching { Core.api.patchConversation(id, starred = starred) }
            runCatching { Core.api.conversations() }.onSuccess { _conversations.value = it }
        }
    }

    fun addAttachment(attachment: Attachment) {
        _attachments.value = _attachments.value + attachment
    }

    fun removeAttachment(id: String) {
        _attachments.value = _attachments.value.filterNot { it.id == id }
    }

    fun send(text: String, regenerate: Boolean = false, parentId: String? = null) {
        val trimmed = text.trim()
        if (trimmed.isEmpty() && !regenerate) return
        if (_streaming.value) return
        val active = _settings.value?.active
        if (active == null) {
            notify(getApplication<Application>().getString(R.string.no_provider))
            return
        }

        val t = _toggles.value
        val attachments = _attachments.value
        _attachments.value = emptyList()

        if (!regenerate) {
            val optimistic = Message(
                id = "local-" + System.nanoTime(),
                sender = "user",
                text = trimmed,
                attachments = attachments,
                createdAt = System.currentTimeMillis(),
            )
            _messages.value = _messages.value + optimistic
        }

        val request = ChatRequest(
            conversationId = _conversationId.value,
            message = trimmed,
            parentId = parentId,
            model = active.model,
            attachments = attachments,
            temporary = t.temporary,
            agent = t.agent || _settings.value?.agentEnabled == true,
            reasoning = t.reasoning,
            search = t.search,
            imageGeneration = t.images,
            regenerate = regenerate,
        )
        startStream(request)
    }

    fun regenerateLast() {
        val lastUser = _messages.value.lastOrNull { it.sender == "user" } ?: return
        send(lastUser.text, regenerate = true, parentId = lastUser.id)
    }

    private fun startStream(request: ChatRequest) {
        _streaming.value = true
        responseId = null
        val events = Channel<StreamEvent>(Channel.UNLIMITED)

        streamJob = viewModelScope.launch {
            val producer = launch(Dispatchers.IO) {
                try {
                    Core.api.chat(request) { events.trySend(it) }
                } catch (t: Throwable) {
                    events.trySend(StreamEvent(type = "error", message = t.message ?: t.javaClass.simpleName))
                } finally {
                    events.close()
                }
            }

            var live = Message(sender = "assistant", createdAt = System.currentTimeMillis())
            val text = StringBuilder()
            val reasoning = StringBuilder()
            var dirty = false

            // Both the consumer below and this ticker run on the main dispatcher, so
            // the buffers need no locking; publishing is throttled to keep scrolling
            // smooth at high token rates.
            val ticker = launch {
                while (isActive) {
                    delay(80)
                    if (dirty) {
                        dirty = false
                        _live.value = live.copy(text = text.toString(), reasoning = reasoning.toString())
                    }
                }
            }

            try {
                for (event in events) {
                    when (event.type) {
                        "start" -> {
                            event.conversationId?.let { _conversationId.value = it }
                            responseId = event.responseId
                            live = live.copy(model = event.model)
                            dirty = true
                        }
                        "delta" -> { text.append(event.text.orEmpty()); dirty = true }
                        "reasoning" -> { reasoning.append(event.text.orEmpty()); dirty = true }
                        "tool_call" -> event.call?.let { live = live.copy(toolCalls = live.toolCalls + it); dirty = true }
                        "tool_result" -> event.result?.let { live = live.copy(toolResults = live.toolResults + it); dirty = true }
                        "approval_required" -> {
                            _approval.value = PendingApproval(
                                id = event.approvalId.orEmpty(),
                                call = event.call ?: ToolCall(),
                                risk = event.risk ?: "medium",
                            )
                        }
                        "search" -> { live = live.copy(searchResults = event.results ?: emptyList()); dirty = true }
                        "images" -> { live = live.copy(generatedImages = live.generatedImages + (event.urls ?: emptyList())); dirty = true }
                        "step" -> event.step?.let { step ->
                            live = live.copy(steps = live.steps.filterNot { it.index == step.index } + step)
                            dirty = true
                        }
                        "usage" -> event.usage?.let { live = live.copy(usage = it); dirty = true }
                        "error" -> { live = live.copy(error = event.message ?: "stream error"); dirty = true }
                        "done" -> Unit
                    }
                }
            } finally {
                ticker.cancel()
                producer.cancel()
                _live.value = null
                _streaming.value = false
                _approval.value = null
                // NonCancellable so a user-initiated stop still reloads the branch the
                // core just finalised instead of leaving a stale partial on screen.
                withContext(NonCancellable) {
                    responseId?.let { runCatching { Core.api.stop(it) } }
                    val id = _conversationId.value
                    if (id != null) {
                        runCatching { Core.api.conversation(id) }.onSuccess { _messages.value = it.messages }
                    }
                    runCatching { Core.api.conversations() }.onSuccess { _conversations.value = it }
                }
            }
        }
    }

    fun stop() {
        streamJob?.cancel()
        streamJob = null
    }

    fun resolveApproval(approved: Boolean) {
        val pending = _approval.value ?: return
        _approval.value = null
        viewModelScope.launch {
            runCatching { Core.api.resolveApproval(pending.id, approved) }
                .onFailure { notify(it.message ?: "approval failed") }
        }
    }

    fun saveSettings(settings: Settings, rebindHost: Boolean = false) {
        viewModelScope.launch {
            runCatching { Core.api.putSettings(settings) }
                .onSuccess {
                    _settings.value = it
                    if (rebindHost) Core.rebind()
                    notify(getApplication<Application>().getString(R.string.save))
                }
                .onFailure { notify(it.message ?: "save failed") }
        }
    }

    fun applyPreset(preset: Preset) {
        val current = _settings.value ?: return
        val active = current.active ?: return
        val updated = active.copy(
            name = preset.name,
            kind = preset.kind,
            baseUrl = preset.baseUrl,
            model = preset.model,
            apiKey = "",
        )
        saveSettings(current.copy(providers = current.providers.map { if (it.id == active.id) updated else it }))
    }

    fun addMemory(text: String) {
        if (text.isBlank()) return
        viewModelScope.launch {
            runCatching { Core.api.addMemory(text) }
            runCatching { Core.api.memories() }.onSuccess { _memories.value = it }
        }
    }

    fun deleteMemory(id: String) {
        viewModelScope.launch {
            runCatching { Core.api.deleteMemory(id) }
            runCatching { Core.api.memories() }.onSuccess { _memories.value = it }
        }
    }

    fun exportCurrent(onReady: (String) -> Unit) {
        val id = _conversationId.value ?: return
        viewModelScope.launch {
            runCatching { Core.api.export(id) }.onSuccess(onReady).onFailure { notify(it.message ?: "export failed") }
        }
    }

    suspend fun readAttachment(name: String, mime: String, bytes: ByteArray): Attachment = withContext(Dispatchers.IO) {
        val isImage = mime.startsWith("image/")
        val isText = mime.startsWith("text/") || name.endsWith(".md") || name.endsWith(".txt") ||
            name.endsWith(".json") || name.endsWith(".csv") || name.endsWith(".kt") || name.endsWith(".rs")
        Attachment(
            id = java.util.UUID.randomUUID().toString(),
            kind = if (isImage) "image" else "file",
            name = name,
            mime = mime,
            size = bytes.size.toLong(),
            dataB64 = if (isImage) android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP) else null,
            text = if (isText) bytes.toString(Charsets.UTF_8).take(200_000) else null,
        )
    }

    fun shareIntent(text: String): Intent = Intent.createChooser(
        Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text),
        null,
    )

    override fun onCleared() {
        streamJob?.cancel()
        super.onCleared()
    }
}