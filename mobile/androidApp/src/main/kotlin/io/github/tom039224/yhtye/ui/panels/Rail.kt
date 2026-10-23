package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.RailProject
import io.github.tom039224.yhtye.shared.sync.fillSlots
import io.github.tom039224.yhtye.shared.sync.railProjects
import io.github.tom039224.yhtye.ui.common.ProjectDot
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import kotlinx.coroutines.launch

// 左端のレール (PC の IconRail・RailProjects、Issue #18): 上からダッシュボード、開いているプロジェクト (状態のドット)、
// 下にコーディング・設定。畳んでいればアイコンとドットだけ、広げると ([RailExpansion]) パネルの上に重なって名前が出る。

/** 畳んだレールの幅 (レイアウト上の幅。広がってもこれは変わらず、パネルの中身の上に重なる)。 */
internal val RAIL_WIDTH = 56.dp

/** 広がったレールの幅 (PC の 232px)。パネルがこれより狭ければパネルの幅まで。 */
internal val RAIL_EXPANDED_WIDTH = 232.dp

/** レールの行 (アイコン 44dp の正方形 + 広がったときの名前)。内側の余白で 56dp の列の真ん中に来る。 */
private val ROW_SIZE = 44.dp
private val RAIL_PADDING = 6.dp
private val ROW_SHAPE = RoundedCornerShape(12.dp)
private val MARKER_SHAPE = RoundedCornerShape(topEnd = 2.dp, bottomEnd = 2.dp)

/** 広がったパネルが中身の上に落とす影 (PC の `--rail-shadow`)。 */
private val RAIL_SHADOW = 16.dp

/** プロジェクトの一覧の下端を地に溶かす高さ (続きがある合図。PC の 14px)。 */
private val LIST_FADE = 14.dp

/** 押せない行 (つながっていない・開いている途中) の薄さ (PC の 45%)。 */
private const val BLOCKED_ALPHA = 0.45f

/**
 * @param width 今の幅 (畳んだ [RAIL_WIDTH] から広がった幅まで)。
 * @param showLabels 名前を出す (広がり始めたら。パネルが伸びるまでは切り取られて見えない)。
 * @param onProjectChosen プロジェクトを選んだ (コーディングのビューに切り替えて、レールを畳む)。
 */
@Composable
internal fun Rail(
    app: AppState,
    sync: AppStore,
    view: MainView,
    onView: (MainView) -> Unit,
    onProjectChosen: () -> Unit,
    onOpenSettings: () -> Unit,
    updateAvailable: Boolean,
    width: Dp,
    showLabels: Boolean,
    modifier: Modifier = Modifier,
) {
    val c = Yhtye.colors
    Column(
        modifier
            .width(width)
            .fillMaxHeight()
            .then(if (width > RAIL_WIDTH) Modifier.shadow(RAIL_SHADOW, clip = false) else Modifier)
            .background(c.bgRail)
            .clipToBounds()
            .semantics { contentDescription = "views" }
            .testTag("rail")
            .padding(horizontal = RAIL_PADDING, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        RailButton(YIcons.Dashboard, "ダッシュボード", view == MainView.Dashboard, "rail-dashboard", showLabels) { onView(MainView.Dashboard) }
        RailProjects(app, sync, working = view == MainView.Work, showLabels = showLabels, onChosen = onProjectChosen)
        RailDivider()
        RailButton(YIcons.Code, "コーディング", view == MainView.Work, "rail-work", showLabels) { onView(MainView.Work) }
        RailButton(
            YIcons.Sliders,
            stringResource(if (updateAvailable) R.string.rail_settings_update else R.string.rail_settings),
            active = false,
            tag = "rail-settings",
            showLabel = showLabels,
            marked = updateAvailable,
            text = "設定",
            onClick = onOpenSettings,
        )
    }
}

@Composable
private fun RailDivider() {
    Box(Modifier.fillMaxWidth().padding(horizontal = 4.dp).height(1.dp).background(Yhtye.colors.line))
}

/** ビューのボタン: アイコンと (広がったとき) その名前。[label] は読み上げの名前、[text] は見える名前 (既定は [label])。 */
@Composable
private fun RailButton(
    icon: ImageVector,
    label: String,
    active: Boolean,
    tag: String,
    showLabel: Boolean,
    marked: Boolean = false,
    text: String = label,
    onClick: () -> Unit,
) {
    val c = Yhtye.colors
    RailRow(label, tag, active, onClick = onClick, icon = {
        YIcon(icon, if (active) c.accent else c.tx4, 20.dp)
        // 小さな印 (右上の点)。意味は読み上げの名前が言う
        if (marked) {
            Box(Modifier.align(Alignment.TopEnd).padding(top = 8.dp, end = 8.dp).size(8.dp).background(c.accent, CircleShape).testTag("$tag-mark"))
        }
    }) {
        if (showLabel) RailText(text, if (active) c.accent else c.tx3)
    }
}

/**
 * レールの 1 行: 44dp のアイコンの場所と、その右に名前 ([content]、広がったときだけ)。[active] は地 `bgChip`・琥珀・左外側の
 * マーカー (PC のアクティブ表現)、[current] は選択行の地 (表示中のプロジェクトが別のビューにいるとき)。
 */
@Composable
private fun RailRow(
    label: String,
    tag: String,
    active: Boolean,
    current: Boolean = false,
    enabled: Boolean = true,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
    icon: @Composable androidx.compose.foundation.layout.BoxScope.() -> Unit,
    content: @Composable RowScope.() -> Unit,
) {
    val c = Yhtye.colors
    Box(contentAlignment = Alignment.CenterStart) {
        if (active) {
            Box(Modifier.offset(x = -RAIL_PADDING).width(3.dp).height(18.dp).clip(MARKER_SHAPE).background(c.accent))
        }
        Row(
            Modifier
                .fillMaxWidth()
                .height(ROW_SIZE)
                .clip(ROW_SHAPE)
                .background(
                    when {
                        active -> c.bgChip
                        current -> c.bgSelected
                        else -> Color.Transparent
                    },
                )
                .combinedClickable(
                    enabled = enabled,
                    role = Role.Button,
                    onLongClickLabel = if (onLongClick != null) "メニューを開く" else null,
                    onLongClick = onLongClick,
                    onClick = onClick,
                )
                .semantics {
                    contentDescription = label
                    selected = active || current
                }
                .then(if (enabled) Modifier else Modifier.alpha(BLOCKED_ALPHA))
                .testTag(tag),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(ROW_SIZE), contentAlignment = Alignment.Center, content = icon)
            content()
        }
    }
}

@Composable
private fun RowScope.RailText(text: String, color: Color) {
    Text(
        text,
        color = color,
        fontSize = 14.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f).padding(end = 10.dp),
    )
}

/**
 * 「プロジェクト」の節 (PC の RailProjects): 開いているプロジェクトと表示中のもの ([railProjects])。畳んでいればドットだけ、
 * 広がると名前。タップでそのプロジェクトに切り替えてコーディングのビューへ (表示中なら開き直さない)。長押しでメニュー
 * (上へ / 下へ移動・スリープ)。並べ替えはレールに出ている行同士が、全体の中で持っていた場所を入れ替える。
 */
@Composable
private fun ColumnScope.RailProjects(app: AppState, sync: AppStore, working: Boolean, showLabels: Boolean, onChosen: () -> Unit) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val rows = railProjects(app.projects, app.project)
    val blocked = app.connection !is ConnectionState.Connected || app.busy.opening
    val shown = rows.map { it.project.id }
    val move = { next: List<String> -> scope.launch { sync.reorderProjects(fillSlots(app.projects.map { it.id }, next)) }; Unit }
    Column(Modifier.weight(1f).fillMaxWidth().semantics { contentDescription = "プロジェクト" }) {
        RailDivider()
        // 見出しの場所は畳んでいても高さを保つ (広げてもドットが動かない)
        Row(Modifier.padding(top = 6.dp).height(24.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(ROW_SIZE), contentAlignment = Alignment.Center) { YIcon(YIcons.Folder, c.tx5, 14.dp) }
            if (showLabels) Text("プロジェクト", color = c.tx4, fontFamily = MonoFont, fontSize = 11.sp, letterSpacing = 0.14.em, maxLines = 1)
        }
        if (rows.isEmpty() && showLabels) {
            Text(
                "開いているプロジェクトはありません。",
                color = c.tx4,
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = ROW_SIZE, top = 2.dp).testTag("rail-empty"),
            )
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(top = 2.dp, bottom = LIST_FADE),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                rows.forEach { row ->
                    RailProjectRow(row, app, sync, working, showLabels, blocked, shown, move, onChosen)
                }
            }
            Box(
                Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .height(LIST_FADE)
                    .background(Brush.verticalGradient(listOf(Color.Transparent, c.bgRail))),
            )
        }
    }
}

@Composable
private fun RailProjectRow(
    row: RailProject,
    app: AppState,
    sync: AppStore,
    working: Boolean,
    showLabel: Boolean,
    blocked: Boolean,
    shown: List<String>,
    onMove: (List<String>) -> Unit,
    onChosen: () -> Unit,
) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val p = row.project
    var menu by remember { mutableStateOf(false) }
    val active = row.current && working
    Box {
        RailRow(
            label = "${p.name} に切り替え (${RING_LABEL.getValue(row.ring)})",
            tag = "rail-project-${p.id}",
            active = active,
            current = row.current,
            enabled = !blocked,
            onLongClick = { menu = true },
            onClick = {
                if (!row.current) scope.launch { sync.openProject(p.path) }
                onChosen()
            },
            icon = { ProjectDot(row.ring, "rail-dot-${p.id}") },
        ) {
            if (showLabel) RailText(p.name, if (active) c.accent else if (row.current) c.tx else c.tx2)
        }
        ProjectMenu(menu, p, app.project, sync, onDismiss = { menu = false }, order = ProjectOrder(shown, "rail-move-${p.id}", onMove))
    }
}
