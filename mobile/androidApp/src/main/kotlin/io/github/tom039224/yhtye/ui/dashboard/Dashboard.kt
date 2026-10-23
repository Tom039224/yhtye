package io.github.tom039224.yhtye.ui.dashboard

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.Ring
import io.github.tom039224.yhtye.shared.sync.projectRing
import io.github.tom039224.yhtye.ui.common.ProjectDot
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import kotlinx.coroutines.launch

// ダッシュボード (PC の Dashboard、Issue #18。アプリの最初のビュー): ようこそ、ホストの登録済みの全プロジェクト (タップで開いて
// コーディングのビューへ) と「プロジェクトを追加」(ホストのフォルダを辿る全画面、`FolderBrowserScreen.kt`)、ホストの利用統計、
// 参考にした agentsview のクレジット。中身だけが縦にスクロールする。

/** 利用統計の考え方を借りたもの (フッターのクレジット)。 */
const val AGENTSVIEW_URL = "https://github.com/kenn-io/agentsview"

/** 内容の最大幅 (広い端末では中央に寄せる。PC の 1200px)。 */
private val CONTENT_MAX = 1200.dp

/** プロジェクトの一覧の高さの上限 (4 行半。それより多いと一覧の中がスクロールし、半分見える行が続きの合図)。 */
private val PROJECT_ROW = 64.dp
private val PROJECT_GAP = 8.dp
private val PROJECT_LIST_MAX = PROJECT_ROW * 4.5f + PROJECT_GAP * 4

private val CARD_SHAPE = RoundedCornerShape(10.dp)
private val STATE_SHAPE = RoundedCornerShape(4.dp)

/** 押せないカード (つながっていない・開いている途中) の薄さ (PC の 45%)。 */
private const val BLOCKED_ALPHA = 0.45f

/**
 * @param showMenu ☰ (左パネル) を出す (左右のパネルを常時表示していなければ)。
 * @param onOpenWork プロジェクトを開いた (コーディングのビューへ)。
 * @param onAddProject 「プロジェクトを追加」(ホストのフォルダを辿る画面を開く)。
 */
@Composable
fun DashboardCenter(
    app: AppState,
    sync: AppStore,
    showMenu: Boolean,
    onOpenLeft: () -> Unit,
    onOpenWork: () -> Unit,
    onAddProject: () -> Unit,
) {
    val c = Yhtye.colors
    Column(Modifier.fillMaxSize().background(c.bgCenter).windowInsetsPadding(WindowInsets.statusBars).testTag("dashboard")) {
        Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            if (showMenu) {
                IconButton(onClick = onOpenLeft, modifier = Modifier.semantics { contentDescription = "プロジェクトとチャット" }.testTag("open-left")) {
                    YIcon(YIcons.Menu, c.tx2, 20.dp)
                }
            } else {
                Spacer(Modifier.width(12.dp))
            }
            Text("ダッシュボード", color = c.tx, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 2.dp))
        }
        HorizontalDivider(color = c.line)
        Box(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).testTag("dashboard-scroll"), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier
                    .widthIn(max = CONTENT_MAX)
                    .fillMaxWidth()
                    .windowInsetsPadding(WindowInsets.navigationBars)
                    .padding(horizontal = 16.dp, vertical = 20.dp),
                verticalArrangement = Arrangement.spacedBy(28.dp),
            ) {
                Welcome()
                DashboardProjects(app, sync, onOpenWork, onAddProject)
                UsageStatsSection(app, sync)
                Credit()
            }
        }
    }
}

@Composable
private fun Welcome() {
    val c = Yhtye.colors
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("ようこそ", color = c.tx, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Text(
            "プロジェクトを選ぶか、git リポジトリを追加して、オーケストレータに依頼を始めましょう。",
            color = c.tx3,
            fontSize = 14.sp,
            lineHeight = 23.sp,
        )
    }
}

/**
 * 登録済みの全プロジェクト (並びはコアのまま = 最近使った順)。見出しの右に「プロジェクトを追加」、その下に追加先のホスト。
 * カードのタップでそのプロジェクトを開き (表示中なら開き直さない)、コーディングのビューへ。
 */
@Composable
private fun DashboardProjects(app: AppState, sync: AppStore, onOpenWork: () -> Unit, onAddProject: () -> Unit) {
    val c = Yhtye.colors
    // 開く要求は画面が替わっても最後まで進める
    val scope = rememberActionScope()
    val connected = app.connection is ConnectionState.Connected
    val blocked = !connected || app.busy.opening
    Column(Modifier.fillMaxWidth().semantics { contentDescription = "プロジェクト" }, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SectionHeading(YIcons.Folder, "プロジェクト")
            Spacer(Modifier.weight(1f))
            DashButton(
                "プロジェクトを追加",
                onClick = onAddProject,
                tag = "add-project",
                primary = true,
                enabled = !blocked,
                icon = if (app.busy.opening) YIcons.Loader else YIcons.Plus,
                spin = app.busy.opening,
            )
        }
        // Android のホストはいつも別のマシン: どこに足すかを添える (PC のリモートのとき)
        app.host.name?.let { host ->
            Text(
                "追加先: $host",
                color = c.tx4,
                fontFamily = MonoFont,
                fontSize = 11.5.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.align(Alignment.End).testTag("add-project-target"),
            )
        }
        if (app.projects.isEmpty()) {
            DashedBox(minHeight = 120.dp, modifier = Modifier.testTag("projects-empty")) {
                Text(
                    "まだプロジェクトがありません。「プロジェクトを追加」から git リポジトリを選んでください。",
                    color = c.tx3,
                    fontSize = 14.sp,
                    lineHeight = 23.sp,
                )
            }
        } else {
            Column(
                Modifier.fillMaxWidth().heightIn(max = PROJECT_LIST_MAX).verticalScroll(rememberScrollState()).testTag("dash-project-list"),
                verticalArrangement = Arrangement.spacedBy(PROJECT_GAP),
            ) {
                val currentId = app.project?.info?.id
                app.projects.forEach { p ->
                    ProjectCard(p, projectRing(p, app.project), current = p.id == currentId, blocked = blocked) {
                        if (p.id != currentId) scope.launch { sync.openProject(p.path) }
                        onOpenWork()
                    }
                }
            }
        }
    }
}

/** カード: ドット + 名前とパス + 状態の語。表示中のカードは選択の地・琥珀の枠・左端の琥珀のマーカー。 */
@Composable
private fun ProjectCard(
    p: ProjectInfo,
    ring: Ring,
    current: Boolean,
    blocked: Boolean,
    onOpen: () -> Unit,
) {
    val c = Yhtye.colors
    val running = ring == Ring.Running
    Box(Modifier.fillMaxWidth().height(PROJECT_ROW)) {
        Row(
            Modifier
                .fillMaxSize()
                .clip(CARD_SHAPE)
                .border(1.dp, if (current) c.accent.copy(alpha = 0.4f) else c.lineCard, CARD_SHAPE)
                .background(if (current) c.bgSelected else c.bgCard)
                .clickable(enabled = !blocked, role = Role.Button, onClick = onOpen)
                .semantics {
                    contentDescription = "${p.name} を開く (${RING_LABEL.getValue(ring)})"
                    selected = current
                }
                .alpha(if (blocked) BLOCKED_ALPHA else 1f)
                .padding(start = 12.dp, end = 12.dp)
                .testTag("dash-project-${p.id}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            ProjectDot(ring, "dash-dot-${p.id}")
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(p.name, color = c.tx, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(p.path, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(
                RING_LABEL.getValue(ring),
                color = if (running) c.doneFg else c.tx4,
                fontFamily = MonoFont,
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier
                    .clip(STATE_SHAPE)
                    .then(if (running) Modifier.background(c.doneBg) else Modifier)
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            )
        }
        if (current) {
            Box(Modifier.align(Alignment.CenterStart).padding(vertical = 16.dp).width(2.dp).fillMaxHeight().background(c.accent))
        }
    }
}

/** 細い仕切りの下に、利用統計の考え方を借りた agentsview (MIT) へのリンク (端末のブラウザで開く)。 */
@Composable
private fun Credit() {
    val c = Yhtye.colors
    val text = buildAnnotatedString {
        append("利用統計は ")
        withLink(LinkAnnotation.Url(AGENTSVIEW_URL, TextLinkStyles(SpanStyle(color = c.tx3, textDecoration = TextDecoration.Underline)))) {
            append("agentsview")
        }
        append(" (MIT) を参考にしています")
    }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        HorizontalDivider(color = c.line)
        Text(text, color = c.tx4, fontSize = 12.sp, lineHeight = 19.sp, modifier = Modifier.testTag("dashboard-credit"))
    }
}
