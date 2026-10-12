package io.github.tom039224.yhtye.ui.common

import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.staticCompositionLocalOf
import kotlinx.coroutines.CoroutineScope

/**
 * 操作 (同期層への要求) を流すスコープ。ホーム ([io.github.tom039224.yhtye.ui.MainScreen]) が持ち、パネルや一覧を閉じても
 * 続ける (例: ドロップダウンでプロジェクトを選ぶと一覧は閉じるが、`open_project` は最後まで進める)。
 */
val LocalActionScope = staticCompositionLocalOf<CoroutineScope?> { null }

/** [LocalActionScope] (無ければこの合成のスコープ)。 */
@Composable
fun rememberActionScope(): CoroutineScope = LocalActionScope.current ?: rememberCoroutineScope()
