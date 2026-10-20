package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.SemanticsPropertyReceiver
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.ClientKind
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.CLIENTS_SHOWN
import io.github.tom039224.yhtye.shared.sync.ClientRow
import io.github.tom039224.yhtye.shared.sync.HostTone
import io.github.tom039224.yhtye.shared.sync.clientKindLabel
import io.github.tom039224.yhtye.shared.sync.clientRows
import io.github.tom039224.yhtye.shared.sync.clientState
import io.github.tom039224.yhtye.shared.sync.formatPingAge
import io.github.tom039224.yhtye.shared.sync.hostTone
import io.github.tom039224.yhtye.shared.sync.hostToneLabel
import io.github.tom039224.yhtye.shared.sync.pingAgeSeconds
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.delay
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

// 左パネルの下端のサーバーとクライアントの一覧 (PC の `src/ui/HostFooter.tsx`、T-73 / T-74)。先頭はサーバー (PC) の行、
// その下にペアリング済みの端末。アイコン (サーバー / スマホ) で区別し、オフラインはグレーにする。

/** ホストの最後の ping からの経過を数え直す間隔 (PC の useNow(1000))。 */
private const val HOST_TICK_MS = 1_000L

/** 開いた端末の一覧はこの高さまでで、残りはその中でスクロールする (PC の 112px。ブランチの木を押しのけない)。 */
private val CLIENT_LIST_MAX = 112.dp
private val SELF_CHIP_SHAPE = RoundedCornerShape(4.dp)
private const val CARET_CLOSED_DEGREES = -90f

/** 行がオフラインでグレーになっているか (テストが色の代わりに見る)。 */
val Dimmed = SemanticsPropertyKey<Boolean>("Dimmed")
private var SemanticsPropertyReceiver.dimmed by Dimmed

private val DATE_TIME: DateTimeFormatter = DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT)

/** 日時を端末のロケールの短い形で (設定の「最後に確認」と共有)。 */
internal fun formatDateTime(ms: Long): String = DATE_TIME.format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))

/** サーバーの行と端末の行。[selfId] はペアリングで保存したこの端末の ID (その行に「この端末」を付ける)。 */
@Composable
internal fun HostAndClients(app: AppState, selfId: String?) {
    val connected = app.connection is ConnectionState.Connected
    ServerRow(app, connected)
    ClientRows(clientRows(app.clients, selfId, connected))
}

/**
 * コアのホスト (ホスト名は ping の答え)、接続の点、最後の ping からの経過。このアプリが PC につながっていなければ
 * アイコンと名前をグレーにする (点は切断・確認中の色のまま)。GUI 無しのホスト (`yhtye --headless`) は別のアイコン。
 */
@Composable
private fun ServerRow(app: AppState, connected: Boolean) {
    val c = Yhtye.colors
    var tick by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(HOST_TICK_MS)
            tick = System.currentTimeMillis()
        }
    }
    val age = pingAgeSeconds(tick, app.host.lastPingAt)
    val tone = hostTone(app.connection, age)
    val state = hostToneLabel(tone, app.connection)
    val dot = when (tone) {
        HostTone.Ok -> c.doneBar
        HostTone.Busy -> c.accent
        HostTone.Bad -> c.handBar
    }
    Row(
        Modifier.fillMaxWidth().semantics { dimmed = !connected }.testTag("host"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        if (app.host.headless) {
            YIcon(
                YIcons.ServerHeadless,
                if (connected) c.tx3 else c.tx5,
                12.dp,
                Modifier.semantics { contentDescription = "ヘッドレスのサーバー" }.testTag("headless-server-icon"),
            )
        } else {
            YIcon(YIcons.Server, if (connected) c.tx3 else c.tx5, 12.dp, Modifier.semantics { contentDescription = "サーバー" }.testTag("server-icon"))
        }
        Box(Modifier.size(7.dp).background(dot, CircleShape).semantics { contentDescription = state }.testTag("host-dot"))
        Text(
            app.host.name ?: "—",
            color = if (connected) c.tx2 else c.tx4,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        YIcon(YIcons.Clock, c.tx3, 12.dp)
        Text(
            age?.let(::formatPingAge) ?: "—",
            color = c.tx3,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            modifier = Modifier.semantics { contentDescription = if (age == null) "まだ応答がありません" else "最後の ping からの経過" },
        )
    }
}

/**
 * ペアリング済みの端末 (接続中が先)。[CLIENTS_SHOWN] 台より多ければ残りを「他 N 台」に畳み、開くと高さを限った枠の中で
 * スクロールする。端末が無ければ何も出さない。
 */
@Composable
private fun ClientRows(rows: List<ClientRow>) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    if (rows.isEmpty()) return
    val hidden = rows.size - CLIENTS_SHOWN
    val shown = if (expanded || hidden <= 0) rows else rows.take(CLIENTS_SHOWN)
    val list = Modifier.fillMaxWidth().testTag("client-list")
    Column(
        if (expanded) list.heightIn(max = CLIENT_LIST_MAX).verticalScroll(rememberScrollState()) else list,
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        shown.forEach { ClientLine(it) }
    }
    if (hidden > 0) MoreButton(expanded, hidden) { expanded = !expanded }
}

@Composable
private fun ClientLine(row: ClientRow) {
    val c = Yhtye.colors
    val d = row.device
    val state = clientState(row, ::formatDateTime)
    val self = if (row.isSelf) " (この端末)" else ""
    Row(
        Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) {
                contentDescription = "${d.name}$self · ${clientKindLabel(d.kind)} · $state"
                dimmed = !row.connected
            }
            .testTag("client-${d.id}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        // 別の PC のアプリはモニター、Android と未知の種類は携帯
        val (icon, iconTag) = if (d.kind == ClientKind.Desktop) YIcons.Monitor to "monitor-icon" else YIcons.Phone to "phone-icon"
        YIcon(icon, if (row.connected) c.tx3 else c.tx5, 12.dp, Modifier.testTag(iconTag))
        Box(Modifier.size(7.dp).background(if (row.connected) c.doneBar else c.ringStop, CircleShape))
        Text(
            d.name,
            color = if (row.connected) c.tx2 else c.tx4,
            fontFamily = MonoFont,
            fontSize = 11.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false),
        )
        if (row.isSelf) {
            Text(
                "この端末",
                color = if (row.connected) c.tx3 else c.tx4,
                fontSize = 10.sp,
                modifier = Modifier.clip(SELF_CHIP_SHAPE).background(c.bgChip).padding(horizontal = 5.dp, vertical = 1.dp).testTag("client-self"),
            )
        }
    }
}

/** 「› 他 N 台」/「⌄ 折りたたむ」。 */
@Composable
private fun MoreButton(expanded: Boolean, hidden: Int, onClick: () -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .clip(SELF_CHIP_SHAPE)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(vertical = 2.dp)
            .testTag("client-more"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        YIcon(YIcons.ChevronDown, c.tx3, 12.dp, if (expanded) Modifier else Modifier.rotate(CARET_CLOSED_DEGREES))
        Text(if (expanded) "折りたたむ" else "他 $hidden 台", color = c.tx3, fontFamily = MonoFont, fontSize = 11.sp)
    }
}
