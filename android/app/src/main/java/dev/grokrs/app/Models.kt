package dev.grokrs.app

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Attachment(
    val id: String = "",
    val kind: String = "file",
    val name: String = "",
    val mime: String = "",
    val size: Long = 0,
    @SerialName("data_b64") val dataB64: String? = null,
    val url: String? = null,
    val text: String? = null,
)

@Serializable
data class SearchResult(
    val title: String = "",
    val url: String = "",
    val snippet: String = "",
    val published: String? = null,
)

@Serializable
data class ToolCall(
    val id: String = "",
    val name: String = "",
    val arguments: String = "{}",
)

@Serializable
data class ToolResult(
    @SerialName("call_id") val callId: String = "",
    val name: String = "",
    val ok: Boolean = true,
    val content: String = "",
    @SerialName("duration_ms") val durationMs: Long = 0,
)

@Serializable
data class AgentStep(
    val index: Int = 0,
    val kind: String = "",
    val title: String = "",
    val detail: String = "",
    val status: String = "running",
)

@Serializable
data class Usage(
    @SerialName("prompt_tokens") val promptTokens: Long = 0,
    @SerialName("completion_tokens") val completionTokens: Long = 0,
    @SerialName("reasoning_tokens") val reasoningTokens: Long = 0,
    @SerialName("total_tokens") val totalTokens: Long = 0,
)

@Serializable
data class Message(
    val id: String = "",
    @SerialName("conversation_id") val conversationId: String = "",
    @SerialName("parent_id") val parentId: String? = null,
    val sender: String = "user",
    val text: String = "",
    val reasoning: String = "",
    @SerialName("created_at") val createdAt: Long = 0,
    val partial: Boolean = false,
    val model: String? = null,
    val attachments: List<Attachment> = emptyList(),
    @SerialName("search_results") val searchResults: List<SearchResult> = emptyList(),
    @SerialName("cited_results") val citedResults: List<SearchResult> = emptyList(),
    @SerialName("generated_images") val generatedImages: List<String> = emptyList(),
    @SerialName("tool_calls") val toolCalls: List<ToolCall> = emptyList(),
    @SerialName("tool_results") val toolResults: List<ToolResult> = emptyList(),
    val steps: List<AgentStep> = emptyList(),
    val usage: Usage = Usage(),
    val error: String? = null,
    val starred: Boolean = false,
)

@Serializable
data class Conversation(
    val id: String = "",
    val title: String = "",
    @SerialName("created_at") val createdAt: Long = 0,
    @SerialName("modified_at") val modifiedAt: Long = 0,
    val starred: Boolean = false,
    val temporary: Boolean = false,
    val persona: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    @SerialName("leaf_id") val leafId: String? = null,
    @SerialName("system_prompt") val systemPrompt: String? = null,
)

@Serializable
data class Memory(
    val id: String = "",
    val text: String = "",
    @SerialName("created_at") val createdAt: Long = 0,
    @SerialName("source_conversation") val sourceConversation: String? = null,
)

@Serializable
data class Provider(
    val id: String = "",
    val name: String = "",
    val kind: String = "open_ai",
    @SerialName("base_url") val baseUrl: String = "",
    @SerialName("api_key") val apiKey: String = "",
    val model: String = "",
    @SerialName("known_models") val knownModels: List<String> = emptyList(),
    @SerialName("extra_headers") val extraHeaders: List<List<String>> = emptyList(),
    @SerialName("timeout_secs") val timeoutSecs: Long = 300,
    @SerialName("supports_tools") val supportsTools: Boolean = true,
    @SerialName("supports_vision") val supportsVision: Boolean = true,
    @SerialName("supports_reasoning") val supportsReasoning: Boolean = false,
    @SerialName("reasoning_effort") val reasoningEffort: String? = null,
)

@Serializable
data class SearchConfig(
    val backend: String = "duck_duck_go",
    @SerialName("api_key") val apiKey: String = "",
    val endpoint: String = "",
    @SerialName("max_results") val maxResults: Int = 6,
)

@Serializable
data class HostConfig(
    @SerialName("allow_lan") val allowLan: Boolean = false,
    val port: Int = 8787,
    val token: String = "",
    @SerialName("allow_shell") val allowShell: Boolean = false,
    @SerialName("workspace_dir") val workspaceDir: String = "",
)

@Serializable
data class Persona(
    val id: String = "",
    val name: String = "",
    val prompt: String = "",
)

@Serializable
data class Settings(
    val providers: List<Provider> = emptyList(),
    @SerialName("active_provider") val activeProvider: String = "",
    val temperature: Float = 0.7f,
    @SerialName("max_tokens") val maxTokens: Int = 4096,
    @SerialName("top_p") val topP: Float = 1f,
    @SerialName("system_prompt") val systemPrompt: String = "",
    val language: String = "en",
    val theme: String = "dark",
    @SerialName("agent_enabled") val agentEnabled: Boolean = false,
    @SerialName("agent_max_steps") val agentMaxSteps: Int = 8,
    @SerialName("approval_mode") val approvalMode: String = "ask",
    @SerialName("enabled_tools") val enabledTools: List<String> = emptyList(),
    val search: SearchConfig = SearchConfig(),
    val host: HostConfig = HostConfig(),
    @SerialName("stream_resume") val streamResume: Boolean = true,
    @SerialName("save_history") val saveHistory: Boolean = true,
    val personas: List<Persona> = emptyList(),
) {
    val active: Provider? get() = providers.firstOrNull { it.id == activeProvider } ?: providers.firstOrNull()
}

@Serializable
data class Preset(
    val name: String = "",
    val kind: String = "open_ai",
    @SerialName("base_url") val baseUrl: String = "",
    val model: String = "",
)

@Serializable
data class ToolInfo(
    val name: String = "",
    val description: String = "",
    val risk: String = "low",
    @SerialName("shell_only") val shellOnly: Boolean = false,
    val enabled: Boolean = false,
    val available: Boolean = true,
)

@Serializable
data class PendingApproval(
    val id: String = "",
    val call: ToolCall = ToolCall(),
    val risk: String = "low",
    @SerialName("created_at") val createdAt: Long = 0,
)

@Serializable
data class ChatRequest(
    @SerialName("conversation_id") val conversationId: String? = null,
    val message: String = "",
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("provider_id") val providerId: String? = null,
    val model: String? = null,
    val attachments: List<Attachment> = emptyList(),
    val temporary: Boolean = false,
    val agent: Boolean = false,
    val reasoning: Boolean = false,
    val search: Boolean = false,
    @SerialName("image_generation") val imageGeneration: Boolean = false,
    val regenerate: Boolean = false,
    val persona: String? = null,
    val temperature: Float? = null,
    @SerialName("max_tokens") val maxTokens: Int? = null,
)

/** Mirrors the tagged `StreamEvent` enum emitted by the Rust core. */
@Serializable
data class StreamEvent(
    val type: String = "",
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("response_id") val responseId: String? = null,
    val model: String? = null,
    val text: String? = null,
    val call: ToolCall? = null,
    val result: ToolResult? = null,
    @SerialName("approval_id") val approvalId: String? = null,
    val risk: String? = null,
    val results: List<SearchResult>? = null,
    val urls: List<String>? = null,
    val step: AgentStep? = null,
    val usage: Usage? = null,
    val message: String? = null,
)

@Serializable
data class Health(
    val ok: Boolean = false,
    val version: String = "",
    @SerialName("allow_lan") val allowLan: Boolean = false,
    @SerialName("provider_count") val providerCount: Int = 0,
    @SerialName("active_model") val activeModel: String = "",
)

@Serializable
data class HostInfo(
    @SerialName("allow_lan") val allowLan: Boolean = false,
    val port: Int = 0,
    val token: String = "",
    val bound: String = "",
    @SerialName("allow_shell") val allowShell: Boolean = false,
    val workspace: String = "",
)

@Serializable
data class ConversationList(val conversations: List<Conversation> = emptyList())

@Serializable
data class ConversationDetail(
    val conversation: Conversation = Conversation(),
    val messages: List<Message> = emptyList(),
)

@Serializable
data class PresetsResponse(
    val presets: List<Preset> = emptyList(),
    val personas: List<Persona> = emptyList(),
)

@Serializable
data class ModelsResponse(val models: List<String> = emptyList())

@Serializable
data class ToolsResponse(
    val tools: List<ToolInfo> = emptyList(),
    @SerialName("approval_mode") val approvalMode: String = "ask",
)

@Serializable
data class MemoriesResponse(val memories: List<Memory> = emptyList())

@Serializable
data class ApprovalsResponse(val pending: List<PendingApproval> = emptyList())

@Serializable
data class ImagesResponse(val images: List<String> = emptyList())

@Serializable
data class SearchResponse(val messages: List<Message> = emptyList())

@Serializable
data class SettingsResponse(
    val ok: Boolean = false,
    val settings: Settings = Settings(),
)

@Serializable
data class HostUpdateResponse(
    val ok: Boolean = false,
    @SerialName("restart_required") val restartRequired: Boolean = false,
    val host: HostConfig = HostConfig(),
)

@Serializable
data class OkResponse(val ok: Boolean = false)

@Serializable
data class ErrorResponse(val error: String = "")