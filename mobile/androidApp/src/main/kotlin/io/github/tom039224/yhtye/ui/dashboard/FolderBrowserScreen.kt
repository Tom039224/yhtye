package io.github.tom039224.yhtye.ui.dashboard

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.settings.SettingsField
import kotlinx.coroutines.launch

// 「プロジェクトを追加」のホストのフォルダブラウザ (PC の FolderBrowser。Android はいつも別のマシンのホストに話すので、端末の
// ファイルではなくホストのフォルダを `list_directory` で辿る)。積み重ねの全画面 (入力欄をダイアログに入れない)。最初は
// ホストのホーム。パス欄から直接移動、親フォルダ (..)・サブフォルダ (git リポジトリには印) のタップで移動、「このフォルダを開く」
// でいま見ているフォルダをプロジェクトとして開く。読めない・開けない理由はこの画面に出す (バナーには出さない)。

private val ROW_SHAPE = RoundedCornerShape(8.dp)
private val LIST_SHAPE = RoundedCornerShape(10.dp)
private val CHIP_SHAPE = RoundedCornerShape(4.dp)
private val ERROR_SHAPE = RoundedCornerShape(8.dp)

/** 読み込み中の一覧の薄さ (PC の 60%)。 */
private const val LOADING_ALPHA = 0.6f

/**
 * @param onBack ヘッダの戻る・「閉じる」。
 * @param onOpened フォルダをプロジェクトとして開けた (コーディングのビューへ)。
 */
@Composable
fun FolderBrowserScreen(app: AppState, sync: AppStore, onBack: () -> Unit, onOpened: () -> Unit) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val browser = sync.folders
    val state by browser.state.collectAsStateWithLifecycle()
    var path by rememberSaveable { mutableStateOf("") }
    var opening by remember { mutableStateOf(false) }
    var openError by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) { browser.home() }
    LaunchedEffect(state.path) { state.path?.let { path = it } }
    val go = { target: String? ->
        openError = null
        scope.launch { browser.open(target) }
        Unit
    }
    val connected = app.connection is ConnectionState.Connected
    val host = app.host.name
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("folder-browser")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.semantics { contentDescription = "戻る" }.testTag("folder-back")) {
                YIcon(YIcons.Back, c.tx2, 20.dp)
            }
            YIcon(YIcons.Folder, c.accent, 18.dp)
            Text(
                if (host != null) "$host 上のフォルダを選択" else "フォルダを選択",
                color = c.tx,
                fontSize = 15.5.sp,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 8.dp).weight(1f),
            )
        }
        HorizontalDivider(color = c.line)
        Column(
            Modifier.weight(1f).fillMaxWidth().windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars)).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val jump = { path.trim().takeIf { it.isNotEmpty() }?.let(go) }
                SettingsField(
                    path,
                    onChange = { path = it },
                    placeholder = "/path/to/git/repository",
                    label = "フォルダのパス",
                    tag = "folder-path",
                    modifier = Modifier.weight(1f),
                    onDone = { jump() },
                )
                DashButton("移動", onClick = { jump() }, tag = "folder-go", enabled = !state.loading && path.isNotBlank())
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    state.path ?: "…",
                    color = c.tx2,
                    fontFamily = MonoFont,
                    fontSize = 12.5.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).testTag("folder-current"),
                )
                if (state.isGitRepo) GitChip(Modifier.testTag("folder-current-git"))
            }
            FolderList(state.parent, state.entries, state.path != null, state.loading, Modifier.weight(1f), go)
            (openError ?: state.error)?.let { FormError(it) }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                DashButton("閉じる", onClick = onBack, tag = "folder-close")
                DashButton(
                    "このフォルダを開く",
                    onClick = {
                        val dir = state.path ?: return@DashButton
                        openError = null
                        opening = true
                        scope.launch {
                            val reason = sync.tryOpenProject(dir)
                            opening = false
                            if (reason == null) onOpened() else openError = reason
                        }
                    },
                    tag = "folder-open",
                    primary = true,
                    enabled = connected && state.path != null && !state.loading && !opening && !app.busy.opening,
                    icon = if (opening) YIcons.Loader else YIcons.Check,
                    spin = opening,
                )
            }
        }
    }
}

/** 一覧: 親フォルダ (..)・サブフォルダ。何も無ければその旨。 */
@Composable
private fun FolderList(
    parent: String?,
    entries: List<io.github.tom039224.yhtye.shared.api.generated.DirectoryEntry>,
    listed: Boolean,
    loading: Boolean,
    modifier: Modifier,
    onGo: (String) -> Unit,
) {
    val c = Yhtye.colors
    LazyColumn(
        modifier
            .fillMaxWidth()
            .clip(LIST_SHAPE)
            .border(1.dp, c.line, LIST_SHAPE)
            .background(c.bgInset)
            .alpha(if (loading) LOADING_ALPHA else 1f)
            .padding(4.dp)
            .semantics { contentDescription = "フォルダ" }
            .testTag("folder-list"),
    ) {
        if (parent != null) {
            item(key = "..") {
                FolderRow("..", "親フォルダ ($parent)", git = false, parentRow = true, tag = "folder-parent") { onGo(parent) }
            }
        }
        items(entries, key = { it.path }) { e ->
            FolderRow(e.name, if (e.isGitRepo) "${e.name} (git リポジトリ)" else e.name, git = e.isGitRepo, parentRow = false, tag = "folder-entry-${e.name}") {
                onGo(e.path)
            }
        }
        if (listed && entries.isEmpty()) {
            item(key = "empty") {
                Text("サブフォルダはありません。", color = c.tx4, fontSize = 12.5.sp, modifier = Modifier.padding(10.dp).testTag("folder-empty"))
            }
        }
    }
}

@Composable
private fun FolderRow(name: String, label: String, git: Boolean, parentRow: Boolean, tag: String, onClick: () -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .height(48.dp)
            .clip(ROW_SHAPE)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics(mergeDescendants = true) { contentDescription = label }
            .padding(horizontal = 10.dp)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        YIcon(if (parentRow) YIcons.ArrowUp else YIcons.Folder, if (git) c.doneBar else c.tx4, 17.dp)
        Text(
            name,
            color = if (parentRow) c.tx4 else c.tx2,
            fontFamily = if (parentRow) MonoFont else null,
            fontSize = 14.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (git) GitChip()
    }
}

/** git リポジトリの印 (緑のチップ)。 */
@Composable
private fun GitChip(modifier: Modifier = Modifier) {
    val c = Yhtye.colors
    Row(
        modifier.clip(CHIP_SHAPE).background(c.doneBg).padding(horizontal = 6.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        YIcon(YIcons.Branch, c.doneFg, 11.dp)
        Text("git", color = c.doneFg, fontFamily = MonoFont, fontSize = 11.sp, fontWeight = FontWeight.Medium)
    }
}

/** その場の失敗 (PC の `.form-error`: 対処中の色の枠付きの一行)。 */
@Composable
private fun FormError(text: String) {
    val c = Yhtye.colors
    Text(
        text,
        color = c.handFg,
        fontFamily = MonoFont,
        fontSize = 12.sp,
        modifier = Modifier
            .fillMaxWidth()
            .clip(ERROR_SHAPE)
            .border(1.dp, c.handCardLine, ERROR_SHAPE)
            .background(c.handCardBg)
            .padding(horizontal = 10.dp, vertical = 7.dp)
            .testTag("folder-error"),
    )
}
