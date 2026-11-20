package dev.grokrs.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.grokrs.app.AppViewModel
import dev.grokrs.app.Message
import dev.grokrs.app.R
import dev.grokrs.app.ToolResult

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(
    vm: AppViewModel,
    seed: String?,
    onOpenConversations: () -> Unit,
    onOpenSettings: () -> Unit,
    onPickFile: () -> Unit,
    onVoice: () -> Unit,
) {
    val messages by vm.messages.collectAsState()
    val live by vm.live.collectAsState()
    val streaming by vm.streaming.collectAsState()
    val toggles by vm.toggles.collectAsState()
    val approval by vm.approval.collectAsState()
    val attachments by vm.attachments.collectAsState()
    val conversations by vm.conversations.collectAsState()
    val conversationId by vm.conversationId.collectAsState()
    val core by vm.core.collectAsState()
    val ready by vm.ready.collectAsState()

    var input by rememberSaveable { mutableStateOf("") }
    LaunchedEffect(seed) { if (!seed.isNullOrEmpty()) input = seed }
    val listState = rememberLazyListState()
    val clipboard = LocalClipboardManager.current
    val uriHandler = LocalUriHandler.current

    val title = conversations.firstOrNull { it.id == conversationId }?.title
        ?: stringResource(R.string.app_name)

    LaunchedEffect(messages.size, live?.text?.length) {
        val count = messages.size + if (live != null) 1 else 0
        if (count > 0 && !listState.canScrollForward) {
            listState.animateScrollToItem(count - 1)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 17.sp) },
                navigationIcon = {
                    IconButton(onClick = onOpenConversations) {
                        Icon(Icons.Filled.Menu, contentDescription = stringResource(R.string.conversations))
                    }
                },
                actions = {
                    IconButton(onClick = { vm.newChat() }) {
                        Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.new_chat))
                    }
                    IconButton(onClick = onOpenSettings) {
                        Icon(Icons.Filled.Settings, contentDescription = stringResource(R.string.settings))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                    titleContentColor = MaterialTheme.colorScheme.onBackground,
                ),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            if (!ready) {
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                    if (core.error.isEmpty()) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(10.dp))
                            Text(stringResource(R.string.core_starting), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    } else {
                        Text(
                            stringResource(R.string.core_failed) + "\n" + core.error,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }

            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(messages, key = { it.id }) { message ->
                    MessageView(message, clipboard::setText, uriHandler::openUri)
                }
                live?.let { partial ->
                    item(key = "live") {
                        MessageView(partial, clipboard::setText, uriHandler::openUri)
                    }
                }
                if (messages.isEmpty() && live == null && ready) {
                    item(key = "empty") {
                        Column(Modifier.fillMaxWidth().padding(24.dp)) {
                            Text(
                                stringResource(R.string.empty_title),
                                fontSize = 20.sp,
                                fontWeight = FontWeight.SemiBold,
                                color = MaterialTheme.colorScheme.onBackground,
                            )
                            Spacer(Modifier.height(6.dp))
                            Text(
                                stringResource(R.string.empty_subtitle),
                                fontSize = 13.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }

            if (attachments.isNotEmpty()) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    attachments.forEach { att ->
                        AssistChip(
                            onClick = { vm.removeAttachment(att.id) },
                            label = { Text(att.name, maxLines = 1, fontSize = 12.sp) },
                            trailingIcon = { Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(14.dp)) },
                        )
                    }
                }
            }

            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                FilterChip(toggles.agent, { vm.setToggles { it.copy(agent = !it.agent) } }, { Text(stringResource(R.string.agent), fontSize = 12.sp) })
                FilterChip(toggles.reasoning, { vm.setToggles { it.copy(reasoning = !it.reasoning) } }, { Text(stringResource(R.string.reasoning), fontSize = 12.sp) })
                FilterChip(toggles.search, { vm.setToggles { it.copy(search = !it.search) } }, { Text(stringResource(R.string.web_search), fontSize = 12.sp) })
                FilterChip(toggles.images, { vm.setToggles { it.copy(images = !it.images) } }, { Text(stringResource(R.string.image_gen), fontSize = 12.sp) })
                FilterChip(toggles.temporary, { vm.setToggles { it.copy(temporary = !it.temporary) } }, { Text(stringResource(R.string.temporary), fontSize = 12.sp) })
            }

            Row(
                Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                OutlinedTextField(
                    value = input,
                    onValueChange = { input = it },
                    modifier = Modifier.weight(1f).heightIn(min = 52.dp, max = 160.dp),
                    placeholder = { Text(stringResource(R.string.message_hint)) },
                    shape = RoundedCornerShape(14.dp),
                    maxLines = 6,
                )
                if (streaming) {
                    IconButton(onClick = { vm.stop() }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.stop), tint = MaterialTheme.colorScheme.error)
                    }
                } else {
                    IconButton(
                        onClick = { vm.send(input); input = "" },
                        enabled = input.isNotBlank() && ready,
                    ) {
                        Icon(Icons.Filled.Send, contentDescription = stringResource(R.string.send), tint = MaterialTheme.colorScheme.primary)
                    }
                }
            }

            Row(
                Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp),
                horizontalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                TextButton(onClick = onPickFile) { Text(stringResource(R.string.attach), fontSize = 12.sp) }
                TextButton(onClick = onVoice) { Text(stringResource(R.string.voice_input), fontSize = 12.sp) }
                TextButton(onClick = { vm.regenerateLast() }, enabled = !streaming && messages.any { it.sender == "user" }) {
                    Icon(Icons.Filled.Refresh, contentDescription = null, modifier = Modifier.size(14.dp))
                    Spacer(Modifier.width(4.dp))
                    Text(stringResource(R.string.regenerate), fontSize = 12.sp)
                }
                TextButton(onClick = { vm.exportCurrent { json -> clipboard.setText(AnnotatedString(json)) } }, enabled = conversationId != null) {
                    Text(stringResource(R.string.export), fontSize = 12.sp)
                }
            }
            Spacer(Modifier.height(4.dp))
        }
    }

    approval?.let { pending ->
        AlertDialog(
            onDismissRequest = { vm.resolveApproval(false) },
            title = { Text(stringResource(R.string.tool_approval)) },
            text = {
                Column {
                    Text(pending.call.name, fontWeight = FontWeight.SemiBold)
                    Text("risk: ${pending.risk}", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(6.dp))
                    Text(pending.call.arguments, style = Mono, fontSize = 12.sp)
                }
            },
            confirmButton = { TextButton(onClick = { vm.resolveApproval(true) }) { Text(stringResource(R.string.approve)) } },
            dismissButton = { TextButton(onClick = { vm.resolveApproval(false) }) { Text(stringResource(R.string.deny)) } },
        )
    }
}

@Composable
private fun MessageView(
    message: Message,
    onCopy: (AnnotatedString) -> Unit,
    onOpenUri: (String) -> Unit,
) {
    val isUser = message.sender == "user"
    var reasoningOpen by remember(message.id) { mutableStateOf(false) }

    Column(
        Modifier.fillMaxWidth(),
        horizontalAlignment = if (isUser) Alignment.End else Alignment.Start,
    ) {
        if (isUser) {
            Column(
                Modifier.clip(RoundedCornerShape(14.dp)).background(UserBubble).padding(12.dp),
            ) {
                if (message.text.isNotEmpty()) {
                    Text(message.text, color = MaterialTheme.colorScheme.onSurface)
                }
                message.attachments.forEach { att ->
                    Text(att.name, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (att.kind == "image" && att.dataB64 != null) {
                        RemoteImage("data:${att.mime};base64,${att.dataB64}")
                    }
                }
            }
            return@Column
        }

        if (message.reasoning.isNotEmpty()) {
            TextButton(onClick = { reasoningOpen = !reasoningOpen }) {
                Icon(
                    if (reasoningOpen) Icons.Filled.KeyboardArrowUp else Icons.Filled.KeyboardArrowDown,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(4.dp))
                Text(stringResource(R.string.reasoning), fontSize = 12.sp)
            }
            if (reasoningOpen) {
                Text(
                    message.reasoning,
                    fontSize = 13.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant).padding(10.dp),
                )
            }
        }

        if (message.steps.isNotEmpty()) {
            Column(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                message.steps.sortedBy { it.index }.forEach { step ->
                    val tint = when (step.status) {
                        "done" -> Success
                        "failed" -> Danger
                        "skipped" -> MaterialTheme.colorScheme.onSurfaceVariant
                        else -> Warning
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(6.dp).clip(RoundedCornerShape(3.dp)).background(tint))
                        Spacer(Modifier.width(6.dp))
                        Text(step.title.ifEmpty { step.kind }, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }

        message.toolCalls.forEach { call ->
            Text(
                "→ ${call.name}",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 1.dp),
            )
        }
        message.toolResults.forEach { result -> ToolResultView(result) }

        if (message.text.isNotEmpty()) {
            Markdown(message.text, Modifier.fillMaxWidth())
        }

        message.generatedImages.forEach { url -> RemoteImage(url) }

        val citations = message.citedResults.ifEmpty { message.searchResults }
        if (citations.isNotEmpty()) {
            HorizontalDivider(Modifier.padding(vertical = 6.dp), color = MaterialTheme.colorScheme.outline)
            citations.forEachIndexed { index, result ->
                Text(
                    "[${index + 1}] ${result.title.ifEmpty { result.url }}",
                    fontSize = 12.sp,
                    color = Accent,
                    modifier = Modifier.fillMaxWidth().clickable { onOpenUri(result.url) }.padding(vertical = 2.dp),
                )
            }
        }

        message.error?.let { error ->
            Text(error, color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
        }

        Row(verticalAlignment = Alignment.CenterVertically) {
            if (message.usage.totalTokens > 0) {
                Text(
                    stringResource(R.string.tokens, message.usage.totalTokens),
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.width(8.dp))
            }
            message.model?.let {
                Text(it, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
            }
            if (message.text.isNotEmpty()) {
                TextButton(onClick = { onCopy(AnnotatedString(message.text)) }) {
                    Text(stringResource(R.string.copy), fontSize = 11.sp)
                }
            }
        }
    }
}

@Composable
private fun ToolResultView(result: ToolResult) {
    var open by remember(result.callId) { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().padding(vertical = 2.dp).clip(RoundedCornerShape(10.dp))
            .background(MaterialTheme.colorScheme.surface).padding(8.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().clickable { open = !open },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                if (result.ok) "✓" else "✗",
                color = if (result.ok) Success else Danger,
                fontSize = 12.sp,
            )
            Spacer(Modifier.width(6.dp))
            Text(result.name, fontSize = 12.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.weight(1f))
            Text("${result.durationMs} ms", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (open) {
            Text(
                result.content.take(8000),
                style = Mono,
                fontSize = 12.sp,
                modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
            )
        }
    }
}