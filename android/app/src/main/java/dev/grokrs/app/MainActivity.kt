package dev.grokrs.app

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.speech.RecognizerIntent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Snackbar
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import dev.grokrs.app.ui.ChatScreen
import dev.grokrs.app.ui.ConversationsScreen
import dev.grokrs.app.ui.GrokTheme
import dev.grokrs.app.ui.SettingsScreen
import kotlinx.coroutines.launch

enum class Screen { Chat, Conversations, Settings }

class MainActivity : ComponentActivity() {

    private lateinit var vm: AppViewModel
    private var seed by mutableStateOf<String?>(null)

    private val pickFile = registerForActivityResult(ActivityResultContracts.GetContent()) { uri: Uri? ->
        if (uri == null) return@registerForActivityResult
        val resolver = contentResolver
        val mime = resolver.getType(uri) ?: "application/octet-stream"
        var name = uri.lastPathSegment ?: "attachment"
        runCatching {
            resolver.query(uri, null, null, null, null)?.use { cursor ->
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0 && cursor.moveToFirst()) name = cursor.getString(index)
            }
        }
        val bytes = runCatching { resolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull()
        if (bytes == null) {
            vm.notify("could not read attachment")
            return@registerForActivityResult
        }
        if (bytes.size > 12 * 1024 * 1024) {
            vm.notify("attachment larger than 12 MB")
            return@registerForActivityResult
        }
        lifecycleScope.launch {
            vm.addAttachment(vm.readAttachment(name, mime, bytes))
        }
    }

    private val voice = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val text = result.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
        if (!text.isNullOrEmpty()) seed = text
    }

    private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        vm = ViewModelProvider(this)[AppViewModel::class.java]
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            notifications.launch(android.Manifest.permission.POST_NOTIFICATIONS)
        }
        if (intent?.action == Intent.ACTION_SEND) {
            seed = intent.getStringExtra(Intent.EXTRA_TEXT)
        }

        setContent {
            val settings by vm.settings.collectAsState()
            val remote by vm.remote.collectAsState()
            val notice by vm.notice.collectAsState()
            var screen by rememberSaveable { mutableStateOf(Screen.Chat) }

            LaunchedEffect(remote) {
                if (remote.isEmpty()) CoreService.start(this@MainActivity)
                else CoreService.stop(this@MainActivity)
            }

            GrokTheme(theme = settings?.theme ?: "dark") {
                val direction = if (settings?.language == "fa") LayoutDirection.Rtl else LayoutDirection.Ltr
                CompositionLocalProvider(LocalLayoutDirection provides direction) {
                    Box(Modifier.fillMaxSize()) {
                        when (screen) {
                            Screen.Chat -> ChatScreen(
                                vm = vm,
                                seed = seed,
                                onOpenConversations = { screen = Screen.Conversations },
                                onOpenSettings = { screen = Screen.Settings },
                                onPickFile = { pickFile.launch("*/*") },
                                onVoice = {
                                    val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                                        .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                                        .putExtra(RecognizerIntent.EXTRA_PROMPT, getString(R.string.voice_input))
                                    runCatching { voice.launch(intent) }.onFailure { vm.notify("no speech recognizer available") }
                                },
                            )
                            Screen.Conversations -> ConversationsScreen(
                                vm = vm,
                                onBack = { screen = Screen.Chat },
                                onOpen = { vm.openConversation(it) },
                            )
                            Screen.Settings -> SettingsScreen(vm = vm, onBack = { screen = Screen.Chat })
                        }

                        notice?.let { message ->
                            Snackbar(
                                modifier = Modifier.align(Alignment.BottomCenter),
                                action = {
                                    TextButton(onClick = { vm.clearNotice() }) { Text("OK") }
                                },
                                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                            ) { Text(message) }
                        }
                    }
                }
            }
        }
    }
}