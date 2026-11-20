package dev.grokrs.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private data class Segment(val code: Boolean, val lang: String, val text: String)

/**
 * Minimal markdown renderer. Covers what chat models actually emit — fenced code,
 * headings, lists, bold/italic/inline-code and links — without pulling a markdown
 * library into the APK.
 */
@Composable
fun Markdown(text: String, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        splitFences(text).forEach { segment ->
            if (segment.code) CodeBlock(segment.lang, segment.text) else Prose(segment.text)
        }
    }
}

private fun splitFences(text: String): List<Segment> {
    val out = mutableListOf<Segment>()
    val buf = StringBuilder()
    var inCode = false
    var lang = ""
    for (line in text.lines()) {
        val trimmed = line.trimStart()
        if (trimmed.startsWith("```")) {
            if (buf.isNotEmpty()) out.add(Segment(inCode, lang, buf.toString()))
            buf.setLength(0)
            if (!inCode) lang = trimmed.removePrefix("```").trim()
            inCode = !inCode
            continue
        }
        if (buf.isNotEmpty()) buf.append('\n')
        buf.append(line)
    }
    if (buf.isNotEmpty()) out.add(Segment(inCode, lang, buf.toString()))
    return out
}

@Composable
private fun CodeBlock(lang: String, code: String) {
    val clipboard = LocalClipboardManager.current
    val content = code.trim('\n')
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(lang.ifEmpty { "code" }, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            TextButton(onClick = { clipboard.setText(AnnotatedString(content)) }) {
                Text("copy", fontSize = 11.sp)
            }
        }
        Text(
            content,
            style = Mono,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.fillMaxWidth().padding(start = 10.dp, end = 10.dp, bottom = 10.dp),
        )
    }
}

private val OrderedItem = Regex("^\\d+[.)] .*")

private fun isBlockStart(s: String): Boolean =
    s.startsWith("#") || s.startsWith("- ") || s.startsWith("* ") || s.startsWith("> ") ||
        s == "---" || s == "***" || s.matches(OrderedItem)

@Composable
private fun Prose(text: String) {
    val lines = text.lines()
    var i = 0
    while (i < lines.size) {
        val trimmed = lines[i].trim()
        when {
            trimmed.isEmpty() -> Unit
            trimmed.startsWith("#") -> {
                val level = trimmed.takeWhile { it == '#' }.length.coerceAtMost(6)
                Text(
                    inline(trimmed.drop(level).trim()),
                    fontSize = (22 - level * 2).sp,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
            trimmed == "---" || trimmed == "***" -> HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            trimmed.startsWith("- ") || trimmed.startsWith("* ") || trimmed.matches(OrderedItem) -> {
                val ordered = trimmed.matches(OrderedItem)
                val marker = if (ordered) trimmed.substringBefore(' ') else "•"
                val content = if (ordered) trimmed.substringAfter(' ') else trimmed.drop(2)
                Row(Modifier.fillMaxWidth()) {
                    Text(marker, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(end = 6.dp))
                    Text(inline(content), color = MaterialTheme.colorScheme.onSurface, modifier = Modifier.weight(1f))
                }
            }
            trimmed.startsWith("> ") -> Text(
                inline(trimmed.drop(2)),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontStyle = FontStyle.Italic,
                modifier = Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surfaceVariant).padding(8.dp),
            )
            else -> {
                val para = StringBuilder(trimmed)
                var j = i + 1
                while (j < lines.size && lines[j].isNotBlank() && !isBlockStart(lines[j].trim())) {
                    para.append(' ').append(lines[j].trim())
                    j++
                }
                Text(inline(para.toString()), color = MaterialTheme.colorScheme.onSurface, lineHeight = 22.sp)
                i = j - 1
            }
        }
        i++
    }
}

private fun inline(source: String): AnnotatedString = buildAnnotatedString {
    var i = 0
    while (i < source.length) {
        when {
            source.startsWith("**", i) -> {
                val end = source.indexOf("**", i + 2)
                if (end > i) {
                    pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
                    append(source.substring(i + 2, end))
                    pop()
                    i = end + 2
                } else { append(source[i]); i++ }
            }
            source[i] == '`' -> {
                val end = source.indexOf('`', i + 1)
                if (end > i) {
                    pushStyle(SpanStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp))
                    append(source.substring(i + 1, end))
                    pop()
                    i = end + 1
                } else { append(source[i]); i++ }
            }
            source[i] == '*' || source[i] == '_' -> {
                val marker = source[i]
                val end = source.indexOf(marker, i + 1)
                if (end > i + 1) {
                    pushStyle(SpanStyle(fontStyle = FontStyle.Italic))
                    append(source.substring(i + 1, end))
                    pop()
                    i = end + 1
                } else { append(source[i]); i++ }
            }
            source[i] == '[' -> {
                val close = source.indexOf(']', i)
                val open = if (close >= 0) source.indexOf('(', close) else -1
                val shut = if (open >= 0) source.indexOf(')', open) else -1
                if (close > i && open == close + 1 && shut > open) {
                    pushStyle(SpanStyle(color = Accent, textDecoration = TextDecoration.Underline))
                    pushStringAnnotation("URL", source.substring(open + 1, shut))
                    append(source.substring(i + 1, close))
                    pop()
                    pop()
                    i = shut + 1
                } else { append(source[i]); i++ }
            }
            else -> { append(source[i]); i++ }
        }
    }
}