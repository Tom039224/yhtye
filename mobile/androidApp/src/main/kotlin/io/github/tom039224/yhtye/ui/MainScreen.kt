package io.github.tom039224.yhtye.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.background.OpenChatRequest
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.LocalActionScope
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.conversation.ConversationScreen
import io.github.tom039224.yhtye.ui.conversation.Mention
import io.github.tom039224.yhtye.ui.conversation.MentionsState
import io.github.tom039224.yhtye.ui.panels.LeftPanel
import io.github.tom039224.yhtye.ui.panels.LeftPanelMemory
import io.github.tom039224.yhtye.ui.panels.MainView
import io.github.tom039224.yhtye.ui.panels.RUNS_EMPTY
import io.github.tom039224.yhtye.ui.panels.RightPanel
import io.github.tom039224.yhtye.ui.panels.RightPanelMemory
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.SettingsScreen
import io.github.tom039224.yhtye.ui.settings.UnavailableDevicePane
import io.github.tom039224.yhtye.ui.shell.Panel
import io.github.tom039224.yhtye.ui.shell.PanelShell
import io.github.tom039224.yhtye.ui.shell.ScreenStack
import io.github.tom039224.yhtye.ui.shell.rememberPanelShellState
import io.github.tom039224.yhtye.ui.tasks.CancelConfirmDialog
import io.github.tom039224.yhtye.ui.tasks.CancelTarget
import io.github.tom039224.yhtye.ui.tasks.TaskDetail
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** 画面の作り直し (回転など) で消したくない画面の状態。 */
class MainViewModel : ViewModel() {
    /** チャットごとのコンポーザのメンション。 */
    val mentions = MentionsState()

    /** 左パネルの木の開閉 (パネルを閉じても残す)。 */
    val left = LeftPanelMemory()

    /** 右パネルの過去のグループの開閉。 */
    val right = RightPanelMemory()
}

/** 中央の上に積む画面 (1 枚だけ)。 */
sealed interface Stacked {
    data class Detail(val task: String) : Stacked

    data object Settings : Stacked

    /** 画面の作り直しで残すための文字列。 */
    val key: String get() = when (this) {
        is Detail -> "task:$task"
        Settings -> SETTINGS_KEY
    }

    companion object {
        private const val SETTINGS_KEY = "settings"

        fun parse(key: String): Stacked? = when {
            key == SETTINGS_KEY -> Settings
            key.startsWith("task:") -> Detail(key.removePrefix("task:"))
            else -> null
        }
    }
}

/** 左パネルのチャットの経過 (`3m`) とタスクの経過を数え直す間隔 (PC の useNow(30 秒))。 */
private const val NOW_TICK_MS = 30_000L

/**
 * ペアリング後のホーム: 3 パネル (左 = ビューの切り替え・プロジェクト・ブランチとチャット・使用量、中央 = 会話、
 * 右 = 選択中のチャットのグループ) と、その上に積む画面 (タスク詳細・設定)。状態は同期層 ([sync]) から読み、
 * 操作も同期層に送る。
 *
 * - 戻るは 積んだ画面 → パネル → 中央 の順 (積んだ画面の `PredictiveBackHandler` は後から合成されるので先に効く)。
 *   積んだ画面が開いている間はパネルのスワイプを止める。
 * - タスクの `@` (右パネルのボタン・行の右→左スワイプ・詳細のメニュー) はコンポーザにメンションを積んで中央に戻る。
 * - ■ (タスク・グループの中止) は確認してから同期層へ。
 * - 設定 ([SettingsScreen]) はレールの一番下の「設定」から [Stacked.Settings] で開く。
 * - 完了の通知のタップ ([openChat]) は、積んだ画面とパネルを閉じて、そのプロジェクトのチャットを中央に出す。
 */
@Composable
fun MainScreen(
    deviceId: String,
    sync: AppStore,
    onReconnect: () -> Unit,
    onUnpair: () -> Unit,
    modifier: Modifier = Modifier,
    gestureInsets: WindowInsets = WindowInsets.systemGestures,
    device: DevicePane = UnavailableDevicePane,
    openChat: OpenChatRequest? = null,
    onOpenChatHandled: (OpenChatRequest) -> Unit = {},
) {
    val model: MainViewModel = viewModel(key = deviceId) { MainViewModel() }
    val app by sync.state.collectAsStateWithLifecycle()
    val shell = rememberPanelShellState()
    val scope = rememberCoroutineScope()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(NOW_TICK_MS)
            now = System.currentTimeMillis()
        }
    }
    var view by rememberSaveable { mutableStateOf(MainView.Work) }
    var stackedKey by rememberSaveable { mutableStateOf<String?>(null) }
    val stacked = stackedKey?.let(Stacked::parse)
    var cancelKey by rememberSaveable { mutableStateOf<String?>(null) }
    val requestCancel = { t: CancelTarget -> cancelKey = t.key }
    val openTask = { task: String -> stackedKey = Stacked.Detail(task).key }
    val addMention = { m: Mention ->
        app.project?.selectedChat?.let { chat ->
            model.mentions.add(chat, m)
            stackedKey = null
            shell.hide()
        }
        Unit
    }
    LaunchedEffect(openChat) {
        val request = openChat ?: return@LaunchedEffect
        stackedKey = null
        cancelKey = null
        view = MainView.Work
        shell.hide()
        sync.showChat(request.project, request.chat)
        onOpenChatHandled(request)
    }
    CompositionLocalProvider(LocalActionScope provides scope) {
        Box(modifier.fillMaxSize()) {
            PanelShell(
                state = shell,
                gestureInsets = gestureInsets,
                swipeEnabled = stacked == null,
                rightEnabled = view == MainView.Work,
                left = {
                    LeftPanel(
                        app = app,
                        sync = sync,
                        now = now,
                        view = view,
                        onView = { view = it },
                        memory = model.left,
                        onChatChosen = shell::hide,
                        onOpenSettings = { stackedKey = Stacked.Settings.key },
                    )
                },
                right = {
                    if (view == MainView.Work) {
                        RightPanel(app, sync, now, model.right, gestureInsets, onOpenTask = openTask, onMention = addMention, onCancel = requestCancel)
                    }
                },
                center = {
                    if (view == MainView.Runs) {
                        RunsCenter(showMenu = !shell.wide, onOpenLeft = { shell.show(Panel.Left) })
                    } else {
                        ConversationScreen(
                            app = app,
                            sync = sync,
                            mentions = model.mentions,
                            showPanelButtons = !shell.wide,
                            onOpenLeft = { shell.show(Panel.Left) },
                            onOpenRight = { shell.show(Panel.Right) },
                            onReconnect = onReconnect,
                        )
                    }
                },
            )
            ScreenStack(stacked, onClose = { stackedKey = null }, gestureInsets = gestureInsets) { screen ->
                when (screen) {
                    is Stacked.Detail -> TaskDetail(
                        app = app,
                        sync = sync,
                        taskId = screen.task,
                        now = now,
                        onBack = { stackedKey = null },
                        onMention = addMention,
                        onCancel = requestCancel,
                    )
                    Stacked.Settings -> SettingsScreen(app, sync.settings, device, onBack = { stackedKey = null }, onUnpair = onUnpair)
                }
            }
        }
    }
    cancelKey?.let(CancelTarget::parse)?.let { target ->
        CancelConfirmDialog(
            target,
            onConfirm = {
                cancelKey = null
                scope.launch {
                    when (target) {
                        is CancelTarget.OfTask -> sync.cancelTask(target.id)
                        is CancelTarget.OfGroup -> sync.cancelGroup(target.id)
                    }
                }
            },
            onDismiss = { cancelKey = null },
        )
    }
}

/** 履歴のビューの中央 (PC の RunsView): まだ設計していないので空の状態だけ。 */
@Composable
private fun RunsCenter(showMenu: Boolean, onOpenLeft: () -> Unit) {
    val c = Yhtye.colors
    Column(Modifier.fillMaxSize().background(c.bgCenter).windowInsetsPadding(WindowInsets.statusBars)) {
        Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            if (showMenu) {
                IconButton(onClick = onOpenLeft, modifier = Modifier.semantics { contentDescription = "プロジェクトとチャット" }.testTag("open-left")) {
                    YIcon(YIcons.Menu, c.tx2, 20.dp)
                }
            } else {
                Spacer(Modifier.width(12.dp))
            }
            Text("履歴", color = c.tx, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 2.dp))
        }
        HorizontalDivider(color = c.line)
        Box(Modifier.weight(1f).fillMaxWidth()) {
            EmptyState(YIcons.History, RUNS_EMPTY, Modifier.align(Alignment.Center), testTag = "runs-center")
        }
    }
}
