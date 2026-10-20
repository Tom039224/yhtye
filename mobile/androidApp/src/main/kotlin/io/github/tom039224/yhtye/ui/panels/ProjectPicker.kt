package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.canSleepProject
import io.github.tom039224.yhtye.shared.sync.projectRing
import io.github.tom039224.yhtye.ui.common.ReorderMenuItems
import io.github.tom039224.yhtye.ui.common.RingMark
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import kotlinx.coroutines.launch

// 表示中のプロジェクト (PC の CurrentProject、Issue #18): 名前を見せるだけ。プロジェクトを選ぶ・並べ替える・スリープするのは
// レール (`Rail.kt`)、一覧と追加はダッシュボード (`ui/dashboard`)。プロジェクトの長押しメニュー ([ProjectMenu]) はレールが使う。

/**
 * 「Project」の見出しと表示中のプロジェクトの名前、その下の [below] (ブランチとチャットの木) を縦にスクロールする列にする。
 */
@Composable
fun ProjectPickerArea(app: AppState, below: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 12.dp)) {
        SideLabel(YIcons.Folder, "Project")
        CurrentProject(app)
        below()
    }
}

/** 左パネルの小さな見出し (モックの `.side-label`)。 */
@Composable
fun SideLabel(icon: ImageVector, text: String, modifier: Modifier = Modifier) {
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

/** 枠の無い見出しの行: リング (名前の 1 行目にそろえる) の右に名前、その下にパス。開いていなければその旨。 */
@Composable
private fun CurrentProject(app: AppState) {
    val c = Yhtye.colors
    val view = app.project
    val current = app.projects.find { it.id == view?.info?.id } ?: view?.info
    if (current == null) {
        Text(
            "プロジェクトが開かれていません",
            color = c.tx3,
            fontSize = 14.sp,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp).testTag("project-name"),
        )
        return
    }
    val ring = projectRing(current, view)
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 16.dp, top = 2.dp, bottom = 8.dp)
            .semantics(mergeDescendants = true) { contentDescription = "${current.name} (${RING_LABEL.getValue(ring)})" }
            .testTag("current-project"),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        RingMark(ring, Modifier.padding(top = 4.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                current.name,
                color = c.tx,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.testTag("project-name"),
            )
            Text(current.path, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** プロジェクトのメニューに足す「上へ / 下へ移動」: 画面に出ているプロジェクトの順 [keys] と、並べ替えた全体の順の受け手。 */
internal class ProjectOrder(val keys: List<String>, val tag: String, val onMove: (List<String>) -> Unit)

/**
 * プロジェクトの長押しメニュー (PC の右クリックのメニュー、チャットのメニューと同じ DropdownMenu): 「スリープ」はそのプロジェクトの
 * すべてのチャットのオーケストレータを止める。起きているチャットがなければ (停止中)、どれかが作業中なら押せない。
 */
@Composable
internal fun ProjectMenu(
    expanded: Boolean,
    project: ProjectInfo,
    view: ProjectView?,
    sync: AppStore,
    onDismiss: () -> Unit,
    order: ProjectOrder? = null,
) {
    val c = Yhtye.colors
    // メニューを閉じても要求は最後まで進める
    val scope = rememberActionScope()
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        if (order != null) ReorderMenuItems(onDismiss, order.keys, project.id, order.tag, order.onMove)
        DropdownMenuItem(
            text = { Text("スリープ") },
            leadingIcon = { YIcon(YIcons.Moon, c.tx3, 18.dp) },
            enabled = canSleepProject(project, view),
            onClick = {
                onDismiss()
                scope.launch { sync.sleepProject(project.id) }
            },
            modifier = Modifier.testTag("menu-sleep-${project.id}"),
        )
    }
}
