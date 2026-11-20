package dev.grokrs.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.grokrs.app.AppViewModel
import dev.grokrs.app.Provider
import dev.grokrs.app.R
import dev.grokrs.app.Settings

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(vm: AppViewModel, onBack: () -> Unit) {
    val settings by vm.settings.collectAsState()
    val presets by vm.presets.collectAsState()
    val models by vm.models.collectAsState()
    val tools by vm.tools.collectAsState()
    val memories by vm.memories.collectAsState()
    val core by vm.core.collectAsState()
    val remote by vm.remote.collectAsState()

    var draft by remember { mutableStateOf<Settings?>(null) }
    LaunchedEffect(settings) { if (draft == null) draft = settings }

    var remoteUrl by remember(remote) { mutableStateOf(remote) }
    var remoteToken by remember { mutableStateOf("") }
    var memoryText by remember { mutableStateOf("") }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, contentDescription = null) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        val s = draft
        if (s == null) {
            Column(Modifier.fillMaxSize().padding(padding), horizontalAlignment = Alignment.CenterHorizontally) {
                Spacer(Modifier.height(48.dp))
                CircularProgressIndicator()
            }
            return@Scaffold
        }

        fun updateActive(transform: (Provider) -> Provider) {
            val active = s.active ?: return
            draft = s.copy(providers = s.providers.map { if (it.id == active.id) transform(it) else it })
        }

        val active = s.active
        val hostChanged = settings?.host?.let {
            it.allowLan != s.host.allowLan || it.port != s.host.port
        } ?: false

        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Section(stringResource(R.string.provider))
            if (active != null) {
                OutlinedTextField(
                    value = active.name,
                    onValueChange = { v -> updateActive { it.copy(name = v) } },
                    label = { Text(stringResource(R.string.provider_name)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = active.baseUrl,
                    onValueChange = { v -> updateActive { it.copy(baseUrl = v) } },
                    label = { Text(stringResource(R.string.base_url)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = active.apiKey,
                    onValueChange = { v -> updateActive { it.copy(apiKey = v) } },
                    label = { Text(stringResource(R.string.api_key)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = active.model,
                    onValueChange = { v -> updateActive { it.copy(model = v) } },
                    label = { Text(stringResource(R.string.model)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    FilterChip(active.kind == "open_ai", { updateActive { it.copy(kind = "open_ai") } }, { Text("OpenAI") })
                    FilterChip(active.kind == "anthropic", { updateActive { it.copy(kind = "anthropic") } }, { Text("Anthropic") })
                }
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    OutlinedButton(onClick = { vm.loadModels() }) { Text(stringResource(R.string.fetch_models)) }
                    if (s.providers.size > 1) {
                        IconButton(onClick = {
                            val remaining = s.providers.filterNot { it.id == active.id }
                            draft = s.copy(providers = remaining, activeProvider = remaining.first().id)
                        }) { Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.delete)) }
                    }
                }
                if (models.isNotEmpty()) {
                    FlowChips(models) { model -> updateActive { it.copy(model = model) } }
                }
                Text(
                    stringResource(R.string.preset),
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                FlowChips(presets.map { it.name }) { name ->
                    presets.firstOrNull { it.name == name }?.let { p ->
                        updateActive { it.copy(name = p.name, kind = p.kind, baseUrl = p.baseUrl, model = p.model) }
                    }
                }
            }
            OutlinedButton(onClick = {
                val fresh = Provider(
                    id = java.util.UUID.randomUUID().toString(),
                    name = "Custom",
                    baseUrl = "",
                    model = "",
                )
                draft = s.copy(providers = s.providers + fresh, activeProvider = fresh.id)
            }) { Text(stringResource(R.string.add_provider)) }

            if (s.providers.size > 1) {
                FlowChips(s.providers.map { it.name.ifEmpty { it.id.take(6) } }) { name ->
                    s.providers.firstOrNull { it.name == name }?.let { draft = s.copy(activeProvider = it.id) }
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.agent))
            SwitchRow(stringResource(R.string.agent), s.agentEnabled) { draft = s.copy(agentEnabled = it) }
            IntField(stringResource(R.string.max_steps), s.agentMaxSteps) { draft = s.copy(agentMaxSteps = it) }
            Text(stringResource(R.string.approval_mode), fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("auto", "ask", "deny").forEach { mode ->
                    FilterChip(s.approvalMode == mode, { draft = s.copy(approvalMode = mode) }, { Text(mode) })
                }
            }
            if (tools.isNotEmpty()) {
                Text(stringResource(R.string.enabled_tools), fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                tools.forEach { tool ->
                    SwitchRow(
                        tool.name + if (tool.available) "" else " (shell disabled)",
                        s.enabledTools.contains(tool.name),
                    ) { on ->
                        draft = s.copy(
                            enabledTools = if (on) s.enabledTools + tool.name else s.enabledTools.filterNot { it == tool.name },
                        )
                    }
                    Text(tool.description, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.web_search))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("duck_duck_go", "brave", "tavily", "searxng", "none").forEach { backend ->
                    FilterChip(s.search.backend == backend, { draft = s.copy(search = s.search.copy(backend = backend)) }, { Text(backend, fontSize = 11.sp) })
                }
            }
            OutlinedTextField(
                value = s.search.apiKey,
                onValueChange = { draft = s.copy(search = s.search.copy(apiKey = it)) },
                label = { Text(stringResource(R.string.api_key)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = s.search.endpoint,
                onValueChange = { draft = s.copy(search = s.search.copy(endpoint = it)) },
                label = { Text(stringResource(R.string.base_url)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.persona))
            OutlinedTextField(
                value = s.systemPrompt,
                onValueChange = { draft = s.copy(systemPrompt = it) },
                label = { Text(stringResource(R.string.system_prompt)) },
                modifier = Modifier.fillMaxWidth().height(120.dp),
            )
            if (s.personas.isNotEmpty()) {
                FlowChips(s.personas.map { it.name }) { name ->
                    s.personas.firstOrNull { it.name == name }?.let { draft = s.copy(systemPrompt = it.prompt) }
                }
            }
            FloatField(stringResource(R.string.temperature), s.temperature) { draft = s.copy(temperature = it) }
            IntField(stringResource(R.string.max_tokens), s.maxTokens) { draft = s.copy(maxTokens = it) }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("en", "fa").forEach { lang ->
                    FilterChip(s.language == lang, { draft = s.copy(language = lang) }, { Text(if (lang == "fa") "فارسی" else "English") })
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("dark", "light", "system").forEach { theme ->
                    FilterChip(s.theme == theme, { draft = s.copy(theme = theme) }, { Text(theme) })
                }
            }
            SwitchRow(stringResource(R.string.save_history), s.saveHistory) { draft = s.copy(saveHistory = it) }

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.host_mode))
            Text(
                stringResource(R.string.host_mode_summary),
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            SwitchRow(stringResource(R.string.host_mode), s.host.allowLan) { draft = s.copy(host = s.host.copy(allowLan = it)) }
            IntField(stringResource(R.string.host_url) + " port", s.host.port) { draft = s.copy(host = s.host.copy(port = it)) }
            SwitchRow(stringResource(R.string.allow_shell), s.host.allowShell) { draft = s.copy(host = s.host.copy(allowShell = it)) }
            val token = s.host.token.ifEmpty { core.token }
            if (token.isNotEmpty()) {
                Text(stringResource(R.string.host_token) + ": " + token, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (core.lanUrl.isNotEmpty()) {
                Text(stringResource(R.string.host_running, core.lanUrl), fontSize = 12.sp, color = Accent)
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.remote_server))
            Text(
                stringResource(R.string.remote_server_summary),
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = remoteUrl,
                onValueChange = { remoteUrl = it },
                label = { Text(stringResource(R.string.server_url)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = remoteToken,
                onValueChange = { remoteToken = it },
                label = { Text(stringResource(R.string.host_token)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                OutlinedButton(onClick = { vm.connectRemote(remoteUrl, remoteToken) }) { Text(stringResource(R.string.connect)) }
                OutlinedButton(onClick = { vm.useEmbedded(); remoteUrl = "" }) { Text(stringResource(R.string.use_embedded)) }
            }

            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Section(stringResource(R.string.memories))
            memories.forEach { memory ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(memory.text, fontSize = 13.sp, modifier = Modifier.weight(1f))
                    IconButton(onClick = { vm.deleteMemory(memory.id) }) {
                        Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.delete), modifier = Modifier.size(18.dp))
                    }
                }
            }
            OutlinedTextField(
                value = memoryText,
                onValueChange = { memoryText = it },
                label = { Text(stringResource(R.string.add_memory)) },
                modifier = Modifier.fillMaxWidth(),
            )
            TextButton(onClick = { vm.addMemory(memoryText); memoryText = "" }) { Text(stringResource(R.string.add_memory)) }

            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { vm.saveSettings(s, rebindHost = hostChanged) },
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.save)) }
            Text(
                "grok-rs core " + core.version,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun Section(title: String) {
    Text(title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = MaterialTheme.colorScheme.primary)
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, fontSize = 13.sp, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
private fun FlowChips(items: List<String>, onClick: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.chunked(3).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                row.forEach { item ->
                    FilterChip(false, { onClick(item) }, { Text(item, fontSize = 11.sp, maxLines = 1) })
                }
            }
        }
    }
}

@Composable
private fun IntField(label: String, value: Int, onChange: (Int) -> Unit) {
    var text by remember(value) { mutableStateOf(value.toString()) }
    OutlinedTextField(
        value = text,
        onValueChange = { text = it; it.toIntOrNull()?.let(onChange) },
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun FloatField(label: String, value: Float, onChange: (Float) -> Unit) {
    var text by remember(value) { mutableStateOf(value.toString()) }
    OutlinedTextField(
        value = text,
        onValueChange = { text = it; it.toFloatOrNull()?.let(onChange) },
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        modifier = Modifier.fillMaxWidth(),
    )
}