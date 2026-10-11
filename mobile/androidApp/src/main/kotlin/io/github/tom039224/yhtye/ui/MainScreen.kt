package io.github.tom039224.yhtye.ui

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.ui.conversation.ConversationScreen
import io.github.tom039224.yhtye.ui.conversation.Mention
import io.github.tom039224.yhtye.ui.conversation.MentionsState
import io.github.tom039224.yhtye.ui.panels.LeftPanelPlaceholder
import io.github.tom039224.yhtye.ui.panels.RightPanelPlaceholder
import io.github.tom039224.yhtye.ui.shell.Panel
import io.github.tom039224.yhtye.ui.shell.PanelShell
import io.github.tom039224.yhtye.ui.shell.rememberPanelShellState
import kotlinx.coroutines.delay

/** 画面の作り直し (回転など) で消したくない画面の状態。 */
class MainViewModel : ViewModel() {
    /** チャットごとのコンポーザのメンション。 */
    val mentions = MentionsState()
}

/** 左パネルのチャットの経過 (`3m`) を数え直す間隔 (PC の useNow(30 秒))。 */
private const val NOW_TICK_MS = 30_000L

/**
 * ペアリング後のホーム: 3 パネル (左 = プロジェクトとチャット、中央 = 会話、右 = グループ)。状態は同期層 ([sync]) から読み、
 * 操作も同期層に送る。
 *
 * T-69 の差し込み口: [LeftPanelPlaceholder] / [RightPanelPlaceholder] を本番のパネルに替える。右パネルのタスクの
 * 行のスワイプは [addMention] を呼ぶ (積んだら中央に戻る)。タスク詳細は中央の上に積む画面として [PanelShell] の
 * 外側 (この関数の最後) に置き、開いている間は `swipeEnabled = false`、詳細の `PredictiveBackHandler` は
 * PanelShell より後に合成されるので先に効く (戻るは 詳細 → パネル → 中央)。
 */
@Composable
fun MainScreen(
    deviceId: String,
    sync: AppStore,
    onReconnect: () -> Unit,
    onUnpair: () -> Unit,
    modifier: Modifier = Modifier,
    gestureInsets: WindowInsets = WindowInsets.systemGestures,
) {
    val model: MainViewModel = viewModel(key = deviceId) { MainViewModel() }
    val app by sync.state.collectAsStateWithLifecycle()
    val shell = rememberPanelShellState()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(NOW_TICK_MS)
            now = System.currentTimeMillis()
        }
    }
    val addMention = { m: Mention ->
        app.project?.selectedChat?.let { chat ->
            model.mentions.add(chat, m)
            shell.hide()
        }
        Unit
    }
    PanelShell(
        state = shell,
        modifier = modifier,
        gestureInsets = gestureInsets,
        left = { LeftPanelPlaceholder(app, sync, now, onChatChosen = shell::hide, onUnpair = onUnpair) },
        right = { RightPanelPlaceholder(app, onMention = addMention) },
        center = {
            ConversationScreen(
                app = app,
                sync = sync,
                mentions = model.mentions,
                showPanelButtons = !shell.wide,
                onOpenLeft = { shell.show(Panel.Left) },
                onOpenRight = { shell.show(Panel.Right) },
                onReconnect = onReconnect,
            )
        },
    )
}
