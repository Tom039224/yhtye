package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientKind
import io.github.tom039224.yhtye.shared.api.generated.UsageReport
import io.github.tom039224.yhtye.shared.api.generated.UsageWindow
import io.github.tom039224.yhtye.shared.api.generated.UsageWindowKind
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlin.math.roundToLong

// 左パネルの下端が出すもの: コアのホストと最後の ping からの経過、ペアリング済みの端末 (PC の `src/ui/hostStatus.ts`)、
// プランと使用量 (PC の `src/ui/StatusBar.tsx` の計算部分)。

/** この秒数以内の ping の応答は「now」。 */
const val PING_NOW_SECONDS: Long = 5

/** 開いている接続でこの秒数応答が無ければ「応答なし」。 */
const val PING_STALE_SECONDS: Long = 12

private const val MINUTE = 60L
private const val HOUR = 60 * MINUTE
private const val DAY = 24 * HOUR
private const val MILLIS_PER_SECOND = 1_000.0

/** 最後の ping の応答からの秒数 (null: まだ応答が無い。負にならない)。 */
fun pingAgeSeconds(nowMs: Long, lastPingAt: Long?): Long? =
    lastPingAt?.let { maxOf(0L, ((nowMs - it) / MILLIS_PER_SECOND).roundToLong()) }

/** 5 秒までは `now`、その後は `12s` / `3m` / `2h` / `4d`。 */
fun formatPingAge(seconds: Long): String = when {
    seconds <= PING_NOW_SECONDS -> "now"
    seconds < MINUTE -> "${seconds}s"
    seconds < HOUR -> "${seconds / MINUTE}m"
    seconds < DAY -> "${seconds / HOUR}h"
    else -> "${seconds / DAY}d"
}

/** 接続の点の色: 緑 = 応答している、琥珀 = 接続中・最初の応答の前、赤 = 切れた・しばらく応答が無い。 */
enum class HostTone { Ok, Busy, Bad }

/** PC の `ConnectionStatus["state"]` に当たる接続の段階。 */
private enum class Link { Open, Connecting, Closed }

private fun linkOf(connection: ConnectionState): Link = when (connection) {
    is ConnectionState.Connected -> Link.Open
    ConnectionState.Idle, is ConnectionState.Connecting -> Link.Connecting
    is ConnectionState.WaitingToReconnect, ConnectionState.Revoked, is ConnectionState.Rejected -> Link.Closed
}

fun hostTone(connection: ConnectionState, ageSeconds: Long?): HostTone {
    val link = linkOf(connection)
    if (link == Link.Closed) return HostTone.Bad
    if (link == Link.Connecting || ageSeconds == null) return HostTone.Busy
    return if (ageSeconds > PING_STALE_SECONDS) HostTone.Bad else HostTone.Ok
}

/** 点の状態の言葉 (読み上げ)。 */
fun hostToneLabel(tone: HostTone, connection: ConnectionState): String {
    val link = linkOf(connection)
    return when {
        tone == HostTone.Ok -> "接続中"
        link == Link.Closed -> "切断"
        link == Link.Open && tone == HostTone.Bad -> "応答なし"
        else -> "接続を確認中"
    }
}

/** ホストの下に出す端末の数。残りは「他 N 台」に畳む。 */
const val CLIENTS_SHOWN: Int = 3

/** 端末の 1 行: この端末か、つながっているか (この端末は一覧の値ではなく、このアプリの接続で決める)。 */
data class ClientRow(val device: ClientDevice, val isSelf: Boolean, val connected: Boolean)

/**
 * 端末の行。接続中が先、次に最終接続の新しい順、ほかはコアの並び (PC の `orderClients`)。[self] (ペアリングで保存した
 * この端末の ID) の行は [selfConnected] (このアプリが PC につながっているか) で決める: 切れている間は一覧が古く、
 * 最後に読んだときの「接続中」が残っているため。
 */
fun clientRows(devices: List<ClientDevice>, self: String?, selfConnected: Boolean): List<ClientRow> = devices
    .map { ClientRow(it, it.id == self, if (it.id == self) selfConnected else it.connected) }
    .sortedWith(compareByDescending<ClientRow> { it.connected }.thenByDescending { it.device.lastSeenMs ?: -1L })

/** 端末の種類の言葉。 */
fun clientKindLabel(kind: ClientKind): String = when (kind) {
    ClientKind.Android -> "Android"
    ClientKind.Unrecognized -> "不明な端末"
}

/** 端末の状態の言葉 (読み上げ)。[formatDateTime] は最終接続の日時の書き方 (端末のロケールの短い形)。 */
fun clientState(row: ClientRow, formatDateTime: (Long) -> String): String {
    if (row.connected) return "接続中"
    val seen = row.device.lastSeenMs
    return if (seen == null) "オフライン · 未接続" else "オフライン · 最終接続 ${formatDateTime(seen)}"
}

/** ハーネスのプラン名 (`max`) をサブスクリプションの名前にする: `Claude Max`。 */
fun planLabel(plan: String): String {
    val name = plan.trim()
        .replace(Regex("^claude\\s+", RegexOption.IGNORE_CASE), "")
        .replace(Regex("(^|\\s)\\S")) { it.value.uppercase() }
    return "Claude $name"
}

/** 報告のうち [kind] の窓 (無ければ null)。 */
fun usageWindow(report: UsageReport?, kind: UsageWindowKind): UsageWindow? = report?.windows?.find { it.kind == kind }

private const val MINUTES_PER_DAY = 1_440L
private const val MILLIS_PER_MINUTE = 60_000L

/** 窓がリセットされるまで: `3h 35m` / `2d 4h` / `12m`。過ぎていれば「リセット済み」。 */
fun formatRemaining(ms: Long): String {
    if (ms <= 0) return "リセット済み"
    val minutes = (ms + MILLIS_PER_MINUTE - 1) / MILLIS_PER_MINUTE
    val days = minutes / MINUTES_PER_DAY
    val hours = (minutes % MINUTES_PER_DAY) / MINUTE
    val mins = minutes % MINUTE
    return when {
        days > 0 -> "${days}d ${hours}h"
        hours > 0 -> "${hours}h ${mins}m"
        else -> "${mins}m"
    }
}
