package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons

// 設定画面の小さな部品 (モックの `.sbody` 以下の見た目)。

internal val CARD_SHAPE = RoundedCornerShape(12.dp)
internal val FIELD_SHAPE = RoundedCornerShape(10.dp)
internal val PILL_SHAPE = RoundedCornerShape(18.dp)

/** 設定の節。 */
enum class SettingsTab(val key: String, val label: String, val icon: ImageVector) {
    Agents("agents", "エージェント", YIcons.Bot),
    Harnesses("harnesses", "ハーネス", YIcons.Terminal),
    Secrets("secrets", "秘密の環境変数", YIcons.Key),

    /** PC の「リモート接続」に当たる、スマホ固有の欄 (接続先・常駐・通知・ペアリング解除)。 */
    Device("device", "この端末", YIcons.Phone),
    ;

    companion object {
        fun parse(key: String?): SettingsTab = entries.find { it.key == key } ?: Agents
    }
}

/** 節の切り替え (モックの `.stabs2`)。4 つは 1 行に入らないので折り返す (横スクロールだと 4 つ目に気づけない)。 */
@Composable
fun SettingsTabRow(selected: SettingsTab, onSelect: (SettingsTab) -> Unit) {
    val c = Yhtye.colors
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        SettingsTab.entries.forEach { tab ->
            val on = tab == selected
            Row(
                Modifier
                    .heightIn(min = 36.dp)
                    .clip(PILL_SHAPE)
                    .then(if (on) Modifier.background(c.bgChip) else Modifier.border(1.dp, c.lineCard, PILL_SHAPE))
                    .clickable(role = Role.Tab) { onSelect(tab) }
                    .padding(horizontal = 14.dp)
                    .semantics { this.selected = on }
                    .testTag("settings-tab-${tab.key}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                YIcon(tab.icon, if (on) c.tx else c.tx3, 16.dp)
                Text(tab.label, color = if (on) c.tx else c.tx3, fontSize = 13.5.sp, maxLines = 1)
            }
        }
    }
}

/** 節の説明 (モックの `.snote`)。 */
@Composable
fun NoteText(text: String, modifier: Modifier = Modifier) {
    Text(text, color = Yhtye.colors.tx3, fontSize = 12.5.sp, lineHeight = 20.sp, modifier = modifier)
}

/** 注意の 1 行 (PC の `.agent-warning`): 警告のアイコンと文。 */
@Composable
fun WarningLine(text: String, modifier: Modifier = Modifier, tag: String? = null, icon: ImageVector = YIcons.Alert) {
    val c = Yhtye.colors
    Row(
        modifier
            .fillMaxWidth()
            .clip(FIELD_SHAPE)
            .background(c.handCardBg)
            .border(1.dp, c.handCardLine, FIELD_SHAPE)
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }
            .then(if (tag != null) Modifier.testTag(tag) else Modifier),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.Top,
    ) {
        YIcon(icon, c.handCardIcon, 14.dp, Modifier.padding(top = 3.dp))
        Text(text, color = c.handCardTx, fontSize = 12.5.sp, lineHeight = 19.sp, modifier = Modifier.weight(1f))
    }
}

/** 状態の 1 行 (読み込み中・取得できない理由)。 */
@Composable
fun StatusLine(text: String, modifier: Modifier = Modifier, error: Boolean = false, tag: String? = null) {
    val c = Yhtye.colors
    Text(
        text,
        color = if (error) c.handFg else c.tx3,
        fontSize = 12.5.sp,
        lineHeight = 19.sp,
        modifier = modifier.semantics { if (error) liveRegion = LiveRegionMode.Polite }.then(if (tag != null) Modifier.testTag(tag) else Modifier),
    )
}

/** 他の端末で設定が変わったが、編集中なので読み直していない (PC の StaleBanner)。 */
@Composable
fun StaleBanner(onReload: () -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .clip(FIELD_SHAPE)
            .background(c.handCardBg)
            .border(1.dp, c.handCardLine, FIELD_SHAPE)
            .padding(start = 10.dp, top = 4.dp, bottom = 4.dp, end = 4.dp)
            .semantics { liveRegion = LiveRegionMode.Polite }
            .testTag("settings-stale"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(YIcons.Refresh, c.handCardIcon, 14.dp)
        Text("他の端末で設定が変更されました。表示は古いままです。", color = c.handCardTx, fontSize = 12.5.sp, lineHeight = 19.sp, modifier = Modifier.weight(1f))
        Box(
            Modifier
                .heightIn(min = 40.dp)
                .clip(FIELD_SHAPE)
                .clickable(role = Role.Button, onClick = onReload)
                .padding(horizontal = 10.dp)
                .testTag("settings-reload"),
            contentAlignment = Alignment.Center,
        ) { Text("再読込", color = c.accent, fontSize = 13.sp) }
    }
}

/**
 * 節の枠: 他の端末で設定が変わった知らせ ([stale] のバナー) を上に固定し、中身は縦にスクロールする (入力欄がキーボードに
 * 隠れないよう、呼ぶ側が IME の余白を取る)。
 */
@Composable
fun SettingsPane(tag: String, stale: Boolean = false, onReload: () -> Unit = {}, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize().testTag(tag)) {
        if (stale) Box(Modifier.padding(horizontal = 14.dp, vertical = 8.dp)) { StaleBanner(onReload) }
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            content = content,
        )
    }
}

/** 枠だけのボタン (モックの `.btn.outline`)。 */
@Composable
fun OutlineButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    spin: Boolean = false,
    tag: String? = null,
) {
    val c = Yhtye.colors
    val tint = if (enabled) c.tx2 else c.tx5
    Row(
        modifier
            .heightIn(min = 42.dp)
            .clip(FIELD_SHAPE)
            .border(BorderStroke(1.dp, c.lineCard), FIELD_SHAPE)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 14.dp)
            .then(if (tag != null) Modifier.testTag(tag) else Modifier),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally),
    ) {
        if (icon != null) YIcon(icon, tint, 14.dp, spin = spin)
        Text(text, color = tint, fontSize = 13.5.sp, maxLines = 1)
    }
}

/** アイコンだけの押せる所 (40dp)。[description] は読み上げとテストの名前。 */
@Composable
fun IconAction(
    icon: ImageVector,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tint: Color = Yhtye.colors.tx3,
    spin: Boolean = false,
    tag: String? = null,
) {
    Box(
        modifier
            .size(40.dp)
            .clip(CircleShape)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { contentDescription = description }
            .then(if (tag != null) Modifier.testTag(tag) else Modifier),
        contentAlignment = Alignment.Center,
    ) { YIcon(icon, if (enabled) tint else Yhtye.colors.tx5, 16.dp, spin = spin) }
}

/** 小さなラベル (モックの `.chip`)。 */
@Composable
fun SmallChip(text: String, bg: Color, fg: Color, modifier: Modifier = Modifier, icon: ImageVector? = null) {
    Row(
        modifier.clip(RoundedCornerShape(6.dp)).background(bg).padding(horizontal = 7.dp, vertical = 3.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (icon != null) YIcon(icon, fg, 12.dp)
        Text(text, color = fg, fontSize = 11.5.sp, maxLines = 1)
    }
}

/** 角丸の枠の中身 (役割・ハーネス・秘密の行のカード)。 */
@Composable
fun SettingsCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Column(modifier.fillMaxWidth().clip(CARD_SHAPE).border(1.dp, Yhtye.colors.lineCard, CARD_SHAPE)) { content() }
}

/**
 * 入力欄 (モックの `.sform input`)。文字は呼び出し側の状態で持つ (IME の途中の文字を壊さないよう、その場で更新する)。
 * [onFocus] はフォーカスが入った・出たとき (編集中かの手がかり)。
 */
@Composable
fun SettingsField(
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    label: String,
    tag: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    mono: Boolean = true,
    password: Boolean = false,
    maxLength: Int = Int.MAX_VALUE,
    onDone: (() -> Unit)? = null,
    onFocus: (Boolean) -> Unit = {},
) {
    val c = Yhtye.colors
    val style = TextStyle(color = if (enabled) c.tx else c.tx4, fontFamily = if (mono) MonoFont else null, fontSize = 13.sp)
    BasicTextField(
        value = value,
        onValueChange = { if (it.length <= maxLength) onChange(it) },
        enabled = enabled,
        singleLine = true,
        textStyle = style,
        cursorBrush = SolidColor(c.accent),
        visualTransformation = if (password) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.None,
            autoCorrectEnabled = false,
            keyboardType = if (password) KeyboardType.Password else KeyboardType.Text,
            imeAction = ImeAction.Done,
        ),
        keyboardActions = KeyboardActions(onDone = { onDone?.invoke() }),
        modifier = modifier
            .fillMaxWidth()
            .onFocusChanged { onFocus(it.isFocused) }
            .semantics { contentDescription = label }
            .testTag(tag),
        decorationBox = { inner ->
            Box(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 42.dp)
                    .clip(FIELD_SHAPE)
                    .border(1.dp, c.lineCard, FIELD_SHAPE)
                    .background(c.bgInset)
                    .padding(horizontal = 10.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (value.isEmpty()) {
                    Text(placeholder, color = c.tx4, fontFamily = if (mono) MonoFont else null, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                inner()
            }
        },
    )
}

/** 選択肢。[value] は選んだときに返す値、[label] は表示、[enabled] が false なら選べない。 */
data class Choice(val value: String, val label: String, val enabled: Boolean = true)

/**
 * 値の欄を押すとメニューが開く選択 (PC の `<select>`)。メニューの項目のテストタグは `<tag>-<value>`。
 */
@Composable
fun SelectField(
    shown: String,
    choices: List<Choice>,
    onSelect: (String) -> Unit,
    label: String,
    tag: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    hint: String? = null,
) {
    val c = Yhtye.colors
    var open by remember { mutableStateOf(false) }
    Box(modifier) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 40.dp)
                .clip(FIELD_SHAPE)
                .border(1.dp, c.lineCard, FIELD_SHAPE)
                .background(c.bgInset)
                .clickable(enabled = enabled, role = Role.DropdownList) { open = true }
                .padding(horizontal = 10.dp)
                .semantics { contentDescription = label }
                .testTag(tag),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                shown,
                color = if (enabled) c.tx else c.tx4,
                fontFamily = MonoFont,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            YIcon(YIcons.ChevronDown, c.tx4, 14.dp)
        }
        DropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }) {
            choices.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label, fontFamily = MonoFont, fontSize = 13.sp) },
                    enabled = choice.enabled,
                    onClick = {
                        open = false
                        onSelect(choice.value)
                    },
                    modifier = Modifier.testTag("$tag-${choice.value}"),
                )
            }
        }
    }
    if (hint != null) StatusLine(hint, Modifier.padding(top = 2.dp))
}
