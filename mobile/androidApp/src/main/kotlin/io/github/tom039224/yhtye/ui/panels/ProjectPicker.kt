package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.backgroundProjects
import io.github.tom039224.yhtye.shared.sync.projectRing
import io.github.tom039224.yhtye.ui.common.RingMark
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

// 表示中のプロジェクトのドロップダウン (PC の ProjectPicker、モックの `.picker`): 知っているプロジェクトの一覧
// (タップで切り替え) と、別のリポジトリを開く欄。その下に PC の BackgroundProjects (表示していないが開いている
// プロジェクトのチップ。リングは PC の規則)。

private val TRIGGER_SHAPE = RoundedCornerShape(12.dp)
private val FIELD_SHAPE = RoundedCornerShape(8.dp)
private val CHIP_SHAPE = RoundedCornerShape(17.dp)
private val POPUP_GAP = 4.dp
private val OPTIONS_MAX_HEIGHT = 240.dp

/** 操作できない (つながっていない・開いている途中)。PC と同じ。 */
private fun blocked(app: AppState): Boolean = app.connection !is ConnectionState.Connected || app.busy.opening

/**
 * 「Project」の見出し・ドロップダウンと、その下の [below] を縦にスクロールする列にする。開いた一覧はその上に重ねて出し
 * (モックと同じ)、外をタップすると閉じる。
 */
@Composable
fun ProjectPickerArea(app: AppState, sync: AppStore, below: @Composable ColumnScope.() -> Unit) {
    var open by remember { mutableStateOf(false) }
    var popupTop by remember { mutableIntStateOf(0) }
    var area: LayoutCoordinates? by remember { mutableStateOf(null) }
    Box(Modifier.fillMaxSize().onGloballyPositioned { area = it }) {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 12.dp)) {
            SideLabel(YIcons.Folder, "Project")
            Trigger(
                app,
                open,
                onToggle = { open = !open },
                modifier = Modifier.onGloballyPositioned { trigger ->
                    val bottom = area?.takeIf { it.isAttached }?.localPositionOf(trigger, Offset(0f, trigger.size.height.toFloat()))?.y
                    if (bottom != null) popupTop = bottom.toInt()
                },
            )
            below()
        }
        if (open) {
            Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { open = false } }.testTag("project-popup-scrim"))
            Popup(app, sync, onClose = { open = false }, modifier = Modifier.offset { IntOffset(0, popupTop + POPUP_GAP.roundToPx()) })
        }
    }
}

/** 左パネルの小さな見出し (モックの `.side-label`)。 */
@Composable
fun SideLabel(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String, modifier: Modifier = Modifier) {
    val c = Yhtye.colors
    Row(
        modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        YIcon(icon, c.tx4, 14.dp)
        Text(text, color = c.tx4, fontSize = 12.sp)
    }
}

@Composable
private fun Trigger(app: AppState, open: Boolean, onToggle: () -> Unit, modifier: Modifier) {
    val c = Yhtye.colors
    val view = app.project
    val current = app.projects.find { it.id == view?.info?.id } ?: view?.info
    val label = current?.name ?: if (app.projects.isNotEmpty()) "プロジェクトを選択" else "プロジェクトを開く"
    Row(
        modifier
            .padding(horizontal = 12.dp)
            .fillMaxWidth()
            .height(46.dp)
            .clip(TRIGGER_SHAPE)
            .border(1.dp, c.lineCard, TRIGGER_SHAPE)
            .background(c.bgCard)
            .clickable(role = Role.DropdownList, onClick = onToggle)
            .semantics { stateDescription = if (open) "開いている" else "閉じている" }
            .padding(horizontal = 12.dp)
            .testTag("project-picker"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (current != null) RingMark(projectRing(current, view))
        Text(
            label,
            color = if (current != null) c.tx else c.tx3,
            fontSize = 15.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).testTag("project-name"),
        )
        YIcon(YIcons.ChevronDown, c.tx4, 16.dp, Modifier.rotate(if (open) 180f else 0f))
    }
}

/** 開いた一覧: プロジェクト (タップで切り替え) と、別のリポジトリのパスを入れて開く欄。 */
@Composable
private fun Popup(app: AppState, sync: AppStore, onClose: () -> Unit, modifier: Modifier) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val view = app.project
    val currentId = view?.info?.id
    val isBlocked = blocked(app)
    val choose = { p: ProjectInfo ->
        if (!isBlocked) {
            if (p.id != currentId) scope.launch { sync.openProject(p.path) }
            onClose()
        }
    }
    Column(
        modifier
            .padding(horizontal = 12.dp)
            .fillMaxWidth()
            .shadow(12.dp, TRIGGER_SHAPE)
            .clip(TRIGGER_SHAPE)
            .background(c.bgCard)
            .border(1.dp, c.lineCard, TRIGGER_SHAPE)
            .padding(vertical = 6.dp)
            .testTag("project-popup"),
    ) {
        if (app.projects.isEmpty()) {
            Text("まだプロジェクトがありません。", color = c.tx3, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp))
        } else {
            Column(Modifier.heightIn(max = OPTIONS_MAX_HEIGHT).verticalScroll(rememberScrollState()).semantics { contentDescription = "プロジェクト" }) {
                app.projects.forEach { p ->
                    val ring = projectRing(p, view)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .height(48.dp)
                            .clickable(enabled = !isBlocked, role = Role.Button) { choose(p) }
                            .semantics {
                                selected = p.id == currentId
                                stateDescription = RING_LABEL.getValue(ring)
                            }
                            .padding(horizontal = 14.dp)
                            .testTag("project-option-${p.id}"),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        RingMark(ring)
                        Text(p.name, color = c.txBody, fontSize = 14.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                        if (p.id == currentId) YIcon(YIcons.Check, c.accent, 16.dp)
                    }
                }
            }
        }
        HorizontalDivider(color = c.line, modifier = Modifier.padding(top = 6.dp))
        OpenForm(app, isBlocked) { path ->
            scope.launch { sync.openProject(path) }
            onClose()
        }
    }
}

/** 別のリポジトリを開く欄 (PC の open-form)。 */
@Composable
private fun OpenForm(app: AppState, isBlocked: Boolean, onOpen: (String) -> Unit) {
    val c = Yhtye.colors
    var path by rememberSaveable { mutableStateOf("") }
    val submit = {
        val target = path.trim()
        if (target.isNotEmpty() && !isBlocked) {
            path = ""
            onOpen(target)
        }
    }
    Row(Modifier.fillMaxWidth().padding(start = 10.dp, end = 4.dp, top = 8.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        BasicTextField(
            value = path,
            onValueChange = { path = it },
            enabled = !isBlocked,
            singleLine = true,
            textStyle = TextStyle(color = c.tx, fontFamily = MonoFont, fontSize = 13.sp),
            cursorBrush = SolidColor(c.accent),
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                autoCorrectEnabled = false,
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Go,
            ),
            keyboardActions = KeyboardActions(onGo = { submit() }),
            modifier = Modifier
                .weight(1f)
                .semantics { contentDescription = "開くリポジトリのパス" }
                .testTag("open-path"),
            decorationBox = { inner ->
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(40.dp)
                        .clip(FIELD_SHAPE)
                        .border(1.dp, c.lineCard, FIELD_SHAPE)
                        .background(c.bgCenter)
                        .padding(horizontal = 10.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (path.isEmpty()) Text("/path/to/git/repository", color = c.tx4, fontFamily = MonoFont, fontSize = 13.sp, maxLines = 1)
                    inner()
                }
            },
        )
        val opening = app.busy.opening
        IconButton(
            onClick = submit,
            enabled = !isBlocked && path.isNotBlank(),
            modifier = Modifier.semantics { contentDescription = "開く" }.testTag("open-submit"),
        ) {
            YIcon(if (opening) YIcons.Loader else YIcons.ArrowRight, if (!isBlocked && path.isNotBlank()) c.tx else c.tx5, 16.dp, spin = opening)
        }
    }
}

/** 表示していないが開いているプロジェクト (コアで動き続けている)。タップで切り替え。 */
@Composable
fun BackgroundProjects(app: AppState, sync: AppStore) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val view = app.project
    val background = backgroundProjects(app.projects, view?.info?.id)
    if (background.isEmpty()) return
    val isBlocked = blocked(app)
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 12.dp, top = 10.dp, bottom = 2.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        YIcon(YIcons.Activity, c.doneBar, 14.dp, Modifier.padding(top = 10.dp).semantics { contentDescription = "バックグラウンドで動作中" })
        FlowRow(
            Modifier.weight(1f).semantics { contentDescription = "バックグラウンドで動作中のプロジェクト" },
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            background.forEach { p ->
                val ring = projectRing(p, view)
                Row(
                    Modifier
                        .height(34.dp)
                        .widthIn(max = 200.dp)
                        .clip(CHIP_SHAPE)
                        .border(1.dp, c.lineCard, CHIP_SHAPE)
                        .background(c.bgChipDim)
                        .clickable(enabled = !isBlocked, role = Role.Button) { scope.launch { sync.openProject(p.path) } }
                        .semantics { contentDescription = "${p.name} に切り替え (${RING_LABEL.getValue(ring)})" }
                        .padding(start = 10.dp, end = 12.dp)
                        .testTag("bg-project-${p.id}"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(7.dp),
                ) {
                    RingMark(ring)
                    Text(p.name, color = c.tx2, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
