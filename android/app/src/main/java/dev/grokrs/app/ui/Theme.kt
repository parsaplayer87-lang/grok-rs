package dev.grokrs.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp

val Bg = Color(0xFF0B0D10)
val Surface = Color(0xFF14181D)
val SurfaceHigh = Color(0xFF1C222A)
val Accent = Color(0xFF4C9AFF)
val TextPrimary = Color(0xFFE6EAF0)
val TextSecondary = Color(0xFF9AA4B2)
val Outline = Color(0xFF2A323C)
val UserBubble = Color(0xFF1B2735)
val Danger = Color(0xFFFF6B6B)
val Success = Color(0xFF4ADE80)
val Warning = Color(0xFFFBBF24)

private val Dark = darkColorScheme(
    primary = Accent,
    onPrimary = Color(0xFF06121F),
    secondary = Accent,
    background = Bg,
    onBackground = TextPrimary,
    surface = Surface,
    onSurface = TextPrimary,
    surfaceVariant = SurfaceHigh,
    onSurfaceVariant = TextSecondary,
    outline = Outline,
    error = Danger,
)

private val Light = lightColorScheme(
    primary = Color(0xFF1668C7),
    background = Color(0xFFF7F9FC),
    surface = Color.White,
    surfaceVariant = Color(0xFFEDF1F6),
    outline = Color(0xFFD3DAE3),
)

val Mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp, lineHeight = 19.sp)

@Composable
fun GrokTheme(theme: String = "dark", content: @Composable () -> Unit) {
    val dark = when (theme) {
        "light" -> false
        "system" -> isSystemInDarkTheme()
        else -> true
    }
    MaterialTheme(
        colorScheme = if (dark) Dark else Light,
        typography = Typography(),
        content = content,
    )
}