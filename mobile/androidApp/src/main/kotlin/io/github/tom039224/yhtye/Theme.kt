package io.github.tom039224.yhtye

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily

/**
 * Yhtye の色 (PC の `src/styles/tokens.css`、[docs/design/tokens.md] とモックの `:root`)。名前はモックの変数に合わせた。
 * ステータス色は oklch の値を sRGB にしたもの。
 */
@Immutable
data class YhtyeColors(
    val isDark: Boolean,
    val bgApp: Color,
    val bgCenter: Color,
    val bgPanel: Color,
    val bgRail: Color,
    val bgCard: Color,
    val bgInset: Color,
    val bgSelected: Color,
    val bgChip: Color,
    val bgChipDim: Color,
    val bgPressed: Color,
    val line: Color,
    val lineCard: Color,
    val lineDashed: Color,
    val tx: Color,
    val txBody: Color,
    val tx2: Color,
    val tx3: Color,
    val tx4: Color,
    val tx5: Color,
    val accent: Color,
    val accentStrong: Color,
    val onAccent: Color,
    val implFg: Color,
    val implBg: Color,
    val implBar: Color,
    val revFg: Color,
    val revBg: Color,
    val revBar: Color,
    val handFg: Color,
    val handBg: Color,
    val handBar: Color,
    val doneFg: Color,
    val doneBg: Color,
    val doneBar: Color,
    val ringHead: Color,
    val ringTrack: Color,
    val ringStop: Color,
    val codeBg: Color,
    val scrim: Color,
    /** 対処中のタスクカード (モックの `--hand-card-*`)。 */
    val handCardBg: Color,
    val handCardLine: Color,
    val handCardTx: Color,
    val handCardTitle: Color,
    val handCardMeta: Color,
    val handCardMetaStrong: Color,
    val handCardIcon: Color,
    /** 進み具合のバーの上を流れる光 (`--impl-sheen` / `--rev-sheen`)。 */
    val implSheen: Color,
    val revSheen: Color,
    /** 使用量のメーター (`--usage-5h` / `--usage-week`)。 */
    val usage5h: Color,
    val usageWeek: Color,
    /** 積み重ねた画面 (タスク詳細・設定) の下を暗くする幕 (`--sheet-scrim`)。 */
    val sheetScrim: Color,
    /** 利用統計のハーネスの色 (`--chart-series-0` … `-5`。同じ番号 = 同じハーネス。最後は中立)。 */
    val chartSeries: List<Color>,
    /** 利用統計のヒートマップの段 (`--heat-0` … `-4`。0 = 使用なし、1–4 = 琥珀の単色の段階)。 */
    val heat: List<Color>,
) {
    /** `accent` の 14% (メンションのチップ・エージェントの印の地)。 */
    val accentSoft: Color get() = accent.copy(alpha = 0.14f)

    /** ユーザーの吹き出しの地と枠 (`accent` を会話の地に 11% / 26% 混ぜる)。 */
    val bubbleBg: Color get() = mix(accent, bgCenter, 0.11f)
    val bubbleLine: Color get() = mix(accent, bgCenter, 0.26f)

    /** タスクの行をずらしたとき下に見える地 (`accent` をパネルの地に 16% 混ぜる)。 */
    val mentionUnder: Color get() = mix(accent, bgPanel, 0.16f)
}

private fun mix(a: Color, b: Color, share: Float): Color = Color(
    red = a.red * share + b.red * (1 - share),
    green = a.green * share + b.green * (1 - share),
    blue = a.blue * share + b.blue * (1 - share),
)

/** PC と同じダーク (tokens.md の値をそのまま)。 */
val DarkYhtyeColors = YhtyeColors(
    isDark = true,
    bgApp = Color(0xFF141312),
    bgCenter = Color(0xFF1E1D1B),
    bgPanel = Color(0xFF1B1A18),
    bgRail = Color(0xFF141312),
    bgCard = Color(0xFF23211F),
    bgInset = Color(0xFF1A1917),
    bgSelected = Color(0xFF2C2824),
    bgChip = Color(0xFF2B2825),
    bgChipDim = Color(0xFF232120),
    bgPressed = Color(0xFF33302B),
    line = Color(0xFF2A2724),
    lineCard = Color(0xFF34302B),
    lineDashed = Color(0xFF3A352F),
    tx = Color(0xFFE8E5DF),
    txBody = Color(0xFFDDD9D2),
    tx2 = Color(0xFFB5AFA5),
    tx3 = Color(0xFF9A948A),
    tx4 = Color(0xFF7D776E),
    tx5 = Color(0xFF57524B),
    accent = Color(0xFFD9A066),
    accentStrong = Color(0xFFE0A45C),
    onAccent = Color(0xFF1E1A15),
    implFg = Color(0xFFAFD5FE),
    implBg = Color(0xFF213C59),
    implBar = Color(0xFF5B97D3),
    revFg = Color(0xFFDACCF7),
    revBg = Color(0xFF413754),
    revBar = Color(0xFF9D82CC),
    handFg = Color(0xFFFFDDCF),
    handBg = Color(0xFF86281D),
    handBar = Color(0xFFBF5546),
    doneFg = Color(0xFFA4DFAF),
    doneBg = Color(0xFF223D27),
    doneBar = Color(0xFF62BB78),
    ringHead = Color(0xFF5DC879),
    ringTrack = Color(0x4762BB78),
    ringStop = Color(0xFF4A453F),
    codeBg = Color(0xFF161514),
    scrim = Color(0x59000000),
    handCardBg = Color(0xFF291714),
    handCardLine = Color(0xFF703229),
    handCardTx = Color(0xFFD8C4B4),
    handCardTitle = Color(0xFFF0E7DF),
    handCardMeta = Color(0xFF8E7F74),
    handCardMetaStrong = Color(0xFFBFA48F),
    handCardIcon = Color(0xFFC7AB97),
    implSheen = Color(0x8CC1E2FF),
    revSheen = Color(0x8CE4D6FF),
    usage5h = Color(0xFFE0843E),
    usageWeek = Color(0xFF6FB880),
    sheetScrim = Color(0x80000000),
    // oklch(0.7 0.11 250) / (0.72 0.11 185) / (0.7 0.11 300) / (0.7 0.13 20) / (0.8 0.11 95) / --text-muted
    chartSeries = listOf(Color(0xFF67A3E0), Color(0xFF3DBBAE), Color(0xFFA98ED9), Color(0xFFE47B7C), Color(0xFFD4BD67), Color(0xFF9A948A)),
    // #2b2825 に accent を 28% / 50% / 74% 混ぜる
    heat = listOf(Color(0xFF2B2825), Color(0xFF5C4A37), Color(0xFF826446), Color(0xFFAC8155), Color(0xFFD9A066)),
)

/**
 * ライト。PC にもモックにも無いので、ダークと同じ流儀 (わずかに暖色に寄せたグレー、琥珀のアクセント、
 * ステータスは色相を固定して明度と彩度だけ振る) で起こした。
 */
val LightYhtyeColors = YhtyeColors(
    isDark = false,
    bgApp = Color(0xFFEDEAE5),
    bgCenter = Color(0xFFFAF8F5),
    bgPanel = Color(0xFFF3F1ED),
    bgRail = Color(0xFFE8E5DF),
    bgCard = Color(0xFFFFFFFF),
    bgInset = Color(0xFFF1EEE9),
    bgSelected = Color(0xFFE6E0D7),
    bgChip = Color(0xFFE9E5DE),
    bgChipDim = Color(0xFFEFECE7),
    bgPressed = Color(0xFFDFD9D0),
    line = Color(0xFFE2DED7),
    lineCard = Color(0xFFD9D3CA),
    lineDashed = Color(0xFFCFC8BD),
    tx = Color(0xFF1F1C18),
    txBody = Color(0xFF2B2722),
    tx2 = Color(0xFF4F4943),
    tx3 = Color(0xFF6B655D),
    tx4 = Color(0xFF857E74),
    tx5 = Color(0xFFADA69B),
    accent = Color(0xFFA9692A),
    accentStrong = Color(0xFF9A5D1F),
    onAccent = Color(0xFFFFFAF3),
    implFg = Color(0xFF0E4F86),
    implBg = Color(0xFFD1E7FF),
    implBar = Color(0xFF347EC4),
    revFg = Color(0xFF563E7B),
    revBg = Color(0xFFE8E0F9),
    revBar = Color(0xFF8668B6),
    handFg = Color(0xFF8F3126),
    handBg = Color(0xFFFDD8CB),
    handBar = Color(0xFFC35141),
    doneFg = Color(0xFF195C2E),
    doneBg = Color(0xFFCEEFD3),
    doneBar = Color(0xFF399D57),
    ringHead = Color(0xFF25984D),
    ringTrack = Color(0x47399D57),
    ringStop = Color(0xFFB9B2A7),
    codeBg = Color(0xFFF2EFEA),
    scrim = Color(0x33000000),
    handCardBg = Color(0xFFFCEEE8),
    handCardLine = Color(0xFFE7B9A9),
    handCardTx = Color(0xFF6E4436),
    handCardTitle = Color(0xFF47281D),
    handCardMeta = Color(0xFF9A7869),
    handCardMetaStrong = Color(0xFF7E5848),
    handCardIcon = Color(0xFFA0705E),
    implSheen = Color(0x8CFFFFFF),
    revSheen = Color(0x8CFFFFFF),
    usage5h = Color(0xFFC86A22),
    usageWeek = Color(0xFF3E9A5A),
    sheetScrim = Color(0x40000000),
    // ダークと同じ色相で明度だけ下げた (oklch(0.55 C H)、黄は 0.62)
    chartSeries = listOf(Color(0xFF3A75AF), Color(0xFF008479), Color(0xFF7C61A8), Color(0xFFB14D51), Color(0xFF9B852B), Color(0xFF6B655D)),
    // bgChip に accent を 28% / 50% / 74% 混ぜる
    heat = listOf(Color(0xFFE9E5DE), Color(0xFFD7C2AC), Color(0xFFC9A784), Color(0xFFBA8959), Color(0xFFA9692A)),
)

val LocalYhtyeColors = staticCompositionLocalOf { DarkYhtyeColors }

/** 機械が出す文字 (ID・ブランチ・ログ・数値・コード) の書体。人が読む日本語は既定の書体。 */
val MonoFont: FontFamily = FontFamily.Monospace

/** 今の Yhtye の色 (`MaterialTheme` と並べて使う)。 */
object Yhtye {
    val colors: YhtyeColors
        @Composable get() = LocalYhtyeColors.current
}

/**
 * 端末のダーク/ライト設定に従う。Dynamic Color は使わず Yhtye の色で統一する (PC と同じ見た目)。
 * Material 3 の部品 (ダイアログ・メニュー・入力欄) の配色もここから起こす。
 */
@Composable
fun YhtyeTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val c = if (darkTheme) DarkYhtyeColors else LightYhtyeColors
    val base = if (darkTheme) darkColorScheme() else lightColorScheme()
    val scheme = base.copy(
        primary = c.accent,
        onPrimary = c.onAccent,
        primaryContainer = c.bgChip,
        onPrimaryContainer = c.tx,
        secondary = c.tx2,
        onSecondary = c.bgCenter,
        tertiary = c.implBar,
        background = c.bgApp,
        onBackground = c.txBody,
        surface = c.bgCenter,
        onSurface = c.tx,
        surfaceVariant = c.bgInset,
        onSurfaceVariant = c.tx3,
        surfaceContainerLowest = c.bgInset,
        surfaceContainerLow = c.bgPanel,
        surfaceContainer = c.bgCard,
        surfaceContainerHigh = c.bgCard,
        surfaceContainerHighest = c.bgChip,
        outline = c.lineCard,
        outlineVariant = c.line,
        error = c.handBar,
        onError = c.onAccent,
        errorContainer = c.handBg,
        onErrorContainer = c.handFg,
        scrim = Color.Black,
    )
    CompositionLocalProvider(LocalYhtyeColors provides c) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
