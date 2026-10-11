package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.shared.sync.TranscriptItem
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.flow.distinctUntilChanged

/** 上端の「さらに前の履歴」の行の鍵。 */
private const val OLDER_KEY = "older"
private const val STREAM_THOUGHT_KEY = "stream-thought"
private const val STREAM_MESSAGE_KEY = "stream-message"

/** 下端からこれより近ければ「最新を見ている」(PC の useAutoScroll の 48px)。 */
private val NEAR_BOTTOM = 48.dp

private fun itemKey(item: TranscriptItem): String = "${item.seq}-${item::class.simpleName}"

/**
 * 会話。新しい発言・本文の断片で最新に付いていく (読み返している間は動かさない。PC の useAutoScroll)。
 * 上端まで来たら古い履歴を読む (PC の OlderHistory。読み込みで表示している所は動かない)。
 * チャットが変わったら呼ぶ側が作り直す (最新から始まる)。
 */
@Composable
fun TranscriptList(model: ConversationModel, onLoadOlder: () -> Unit, modifier: Modifier = Modifier) {
    val items = model.items
    val streaming = model.streaming
    val keys = buildList {
        if (model.hasOlder) add(OLDER_KEY)
        items.forEach { add(itemKey(it)) }
        if (streaming?.thought?.isNotEmpty() == true) add(STREAM_THOUGHT_KEY)
        if (streaming?.message?.isNotEmpty() == true) add(STREAM_MESSAGE_KEY)
    }
    val listState = rememberLazyListState(initialFirstVisibleItemIndex = maxOf(0, keys.size - 1))
    val follow = rememberFollow(listState, keys)
    val contentKey = listOf(keys.size, keys.lastOrNull(), streaming?.message?.length, streaming?.thought?.length)
    LaunchedEffect(contentKey) { follow.onContentChanged() }

    LazyColumn(
        state = listState,
        modifier = modifier.testTag("transcript"),
        contentPadding = PaddingValues(top = 12.dp, bottom = 8.dp),
    ) {
        if (model.hasOlder) {
            item(key = OLDER_KEY) { OlderHistoryRow(model, onLoadOlder) }
        }
        items(items, key = ::itemKey) { item -> TranscriptRow(item, model.queued, model.state) }
        if (streaming?.thought?.isNotEmpty() == true) item(key = STREAM_THOUGHT_KEY) { StreamingThought(streaming.thought) }
        if (streaming?.message?.isNotEmpty() == true) item(key = STREAM_MESSAGE_KEY) { StreamingMessage(streaming.message) }
    }
}

/** 最新に付いていくか (PC の useAutoScroll の `pinned`)。 */
private class Follow(private val listState: LazyListState, private val keys: () -> List<String>) {
    /** ユーザーが下端の近くにいる。ユーザーのスクロールでだけ変わる (内容が伸びても変えない)。 */
    var pinned by mutableStateOf(true)

    /** 古い履歴が前に入る前の、見えていた最初の行 (鍵と、その行の上端の位置)。 */
    var anchor: Pair<String, Int>? = null
    var head: String? = null

    suspend fun onContentChanged() {
        val all = keys()
        val newHead = all.firstOrNull { it != OLDER_KEY }
        val prepended = head != null && newHead != head
        head = newHead
        if (all.isEmpty()) return
        if (pinned) {
            listState.scrollToItem(all.lastIndex, Int.MAX_VALUE)
            return
        }
        // 前に足された: 読んでいた行を同じ位置に残す (先頭の「読み込み中…」の行は高さが変わらない)
        val (key, offset) = anchor?.takeIf { prepended } ?: return
        val index = all.indexOf(key).takeIf { it >= 0 } ?: return
        listState.scrollToItem(index)
        if (offset != 0) listState.scrollBy(-offset.toFloat())
    }
}

@Composable
private fun rememberFollow(listState: LazyListState, keys: List<String>): Follow {
    val current by rememberUpdatedState(keys)
    val follow = remember(listState) { Follow(listState) { current } }
    val near = with(LocalDensity.current) { NEAR_BOTTOM.toPx() }
    LaunchedEffect(listState) {
        snapshotFlow { distanceToBottom(listState) to listState.isScrollInProgress }
            .distinctUntilChanged()
            .collect { (distance, scrolling) -> if (scrolling) follow.pinned = distance < near }
    }
    LaunchedEffect(listState) {
        // 最初に見えている行が変わったときだけ覚える (前に足されて位置がずれたときは覚え直さない)
        snapshotFlow { listState.firstVisibleItemIndex to listState.firstVisibleItemScrollOffset }.collect {
            follow.anchor = listState.layoutInfo.visibleItemsInfo
                .firstOrNull { it.key != OLDER_KEY }
                ?.let { (it.key as String) to it.offset }
        }
    }
    LaunchedEffect(listState) {
        // 入力欄・キーボードで高さが変わった: 最新を見ていたら最新に付け直す
        snapshotFlow { listState.layoutInfo.viewportEndOffset }.distinctUntilChanged().collect {
            if (follow.pinned) follow.onContentChanged()
        }
    }
    return follow
}

/** 下端までの距離 (px。最後の行が見えていなければ大きな値)。 */
private fun distanceToBottom(state: LazyListState): Int {
    val info = state.layoutInfo
    val last = info.visibleItemsInfo.lastOrNull() ?: return 0
    if (last.index < info.totalItemsCount - 1) return Int.MAX_VALUE
    return maxOf(0, last.offset + last.size - info.viewportEndOffset + info.afterContentPadding)
}

/**
 * 会話の上端の行 (PC の OlderHistory)。見えたら前のページを読み、読んだ後もまだ見えていれば次も読む。失敗したら
 * 理由と再試行。どの状態でも同じ高さ (読み込みで会話が動かない)。
 */
@Composable
private fun OlderHistoryRow(model: ConversationModel, onLoadOlder: () -> Unit) {
    val c = Yhtye.colors
    val view = model.project ?: return
    val waiting = view.phase == LoadPhase.Ready && !view.loadingOlder && view.olderError == null
    LaunchedEffect(waiting, view.historyStart) { if (waiting) onLoadOlder() }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 40.dp).padding(horizontal = 16.dp).testTag("older-history"),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val error = view.olderError
        if (error != null) {
            YIcon(YIcons.Alert, c.handBar, 12.dp)
            Text(
                "履歴を読み込めませんでした: $error",
                color = c.tx4,
                fontFamily = MonoFont,
                fontSize = 11.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false).padding(start = 6.dp),
            )
            IconButton(onClick = onLoadOlder, modifier = Modifier.semantics { contentDescription = "再試行" }) {
                YIcon(YIcons.Refresh, c.tx3, 14.dp)
            }
        } else {
            YIcon(YIcons.Loader, c.tx4, 12.dp, spin = true)
            Text(
                "読み込み中…",
                color = c.tx4,
                fontFamily = MonoFont,
                fontSize = 11.sp,
                modifier = Modifier.padding(start = 6.dp).semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
    }
}
