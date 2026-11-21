package com.cvby.cin.ui

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private val Green = Color(0xFF4ADE80)
private val Navy = Color(0xFF0F1B2A)
private val Steel = Color(0xFF1B2B3D)
private val Amber = Color(0xFFFBBF24)
private val Red = Color(0xFFF87171)

private val DarkColors = darkColorScheme(
    primary = Green,
    onPrimary = Navy,
    primaryContainer = Color(0xFF14432B),
    onPrimaryContainer = Color(0xFFB7F5CD),
    secondary = Color(0xFF8FB3D9),
    onSecondary = Navy,
    background = Navy,
    onBackground = Color(0xFFE6EDF5),
    surface = Steel,
    onSurface = Color(0xFFE6EDF5),
    surfaceVariant = Color(0xFF24384D),
    onSurfaceVariant = Color(0xFFB6C5D6),
    error = Red,
    tertiary = Amber,
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF0F7A45),
    onPrimary = Color.White,
    secondary = Color(0xFF3A5F87),
    background = Color(0xFFF7F9FB),
    surface = Color.White,
    error = Color(0xFFB3261E),
    tertiary = Color(0xFF9A6B00),
)

@Composable
fun CinTheme(
    dark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val colors = if (dark) DarkColors else LightColors
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            // 状态栏颜色交给 enableEdgeToEdge() 处理（API 35 起 statusBarColor 已废弃），
            // 这里只需要同步图标明暗，让深色主题下图标变白。
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !dark
            }
        }
    }
    MaterialTheme(
        colorScheme = colors,
        typography = Typography(),
        content = content,
    )
}