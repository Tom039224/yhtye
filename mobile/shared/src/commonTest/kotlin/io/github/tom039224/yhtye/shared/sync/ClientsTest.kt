package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientKind
import io.github.tom039224.yhtye.shared.api.generated.ClientServer
import io.github.tom039224.yhtye.shared.api.generated.Clients
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * クライアント一覧 (T-74。PC の `Sidebar.test.tsx` の「the clients under the host」の同期層の部分): つながったら `list_clients`、
 * `clients_changed` で置き換え、resync と再接続で読み直し、読んでいる途中に来たイベントの方を残す。行の並び・この端末・状態の言葉。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class ClientsTest {
    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }

    private fun names(s: AppStore) = s.state.value.clients.map { it.name }

    private fun clientsChanged(vararg devices: ClientDevice) =
        appWide(ApiEventBody.ClientsChanged(Clients(ClientServer("test-host"), devices.toList())))

    @Test
    fun readsThePairedDevicesWhenItConnects() = runTest {
        core.devices = listOf(PIXEL, TABLET)
        val s = newStore(core)
        assertEquals(listOf("Pixel 9", "Tablet"), names(s))
        assertEquals(1, core.callsOf<ApiCommand.ListClients>().size)
    }

    @Test
    fun replacesTheListWithEveryClientsChanged() = runTest {
        core.devices = listOf(PIXEL, TABLET)
        val s = newStore(core)
        push(core, clientsChanged(PIXEL, TABLET.copy(connected = true)))
        assertEquals(listOf(true, true), s.state.value.clients.map { it.connected })
        push(core, clientsChanged(TABLET))
        assertEquals(listOf("Tablet"), names(s))
        push(core, clientsChanged())
        assertEquals(emptyList(), s.state.value.clients)
    }

    @Test
    fun readsTheDevicesAgainOnAResyncAndAfterAReconnect() = runTest {
        val s = newStore(core)
        assertEquals(emptyList(), names(s))
        core.devices = listOf(TABLET)
        push(core, appWide(ApiEventBody.Resync))
        assertEquals(listOf("Tablet"), names(s))
        core.devices = listOf(TABLET, PIXEL)
        core.state.value = ConnectionState.WaitingToReconnect(1, 100, "gone")
        runCurrent()
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(listOf("Tablet", "Pixel 9"), names(s))
    }

    @Test
    fun keepsAClientsChangedThatCameWhileAnOlderReadWasOnItsWay() = runTest {
        val release = CompletableDeferred<Unit>()
        core.devices = listOf(TABLET)
        core.before = { if (it == ApiCommand.ListClients) release.await() }
        val s = newStore(core)
        push(core, clientsChanged(PIXEL))
        release.complete(Unit)
        runCurrent()
        assertEquals(listOf("Pixel 9"), names(s))
    }

    @Test
    fun aFailedReadKeepsTheLastListAndShowsNoError() = runTest {
        core.devices = listOf(PIXEL)
        val s = newStore(core)
        core.fail(ApiCommand.ListClients::class, ApiErrorCode.InvalidArgument, "unknown command")
        push(core, appWide(ApiEventBody.Resync))
        assertEquals(listOf("Pixel 9"), names(s))
        assertEquals(emptyList(), s.state.value.errors)
    }

    // ---- the rows ----

    @Test
    fun putsConnectedDevicesFirstThenTheMostRecentlySeen() {
        val rows = clientRows(listOf(NEVER, TABLET, PIXEL), self = null, selfConnected = true)
        assertEquals(listOf("Pixel 9", "Tablet", "Old phone"), rows.map { it.device.name })
        assertEquals(listOf(true, false, false), rows.map { it.connected })
        assertEquals(listOf(false, false, false), rows.map { it.isSelf })
    }

    @Test
    fun theDeviceItselfIsMarkedAndFollowsThisAppsConnectionNotTheLastList() {
        val online = clientRows(listOf(PIXEL, TABLET.copy(connected = false)), self = TABLET.id, selfConnected = true)
        // 一覧ではオフラインでも、つながっているこの端末は接続中 (接続中の中では最終接続の新しい順)
        assertEquals(listOf("Pixel 9" to false, "Tablet" to true), online.map { it.device.name to it.isSelf })
        assertEquals(listOf(true, true), online.map { it.connected })
        // 切れている間: 最後に読んだ一覧では接続中でも、この端末はオフライン
        val offline = clientRows(listOf(PIXEL), self = PIXEL.id, selfConnected = false)
        assertEquals(listOf(false), offline.map { it.connected })
        assertEquals(listOf(true), offline.map { it.isSelf })
    }

    @Test
    fun saysHowADeviceIsInWords() {
        val format = { ms: Long -> "at $ms" }
        val (pixel, tablet, never) = clientRows(listOf(PIXEL, TABLET, NEVER), self = null, selfConnected = true)
        assertEquals("接続中", clientState(pixel, format))
        assertEquals("オフライン · 最終接続 at ${TABLET.lastSeenMs}", clientState(tablet, format))
        assertEquals("オフライン · 未接続", clientState(never, format))
        assertEquals("Android", clientKindLabel(ClientKind.Android))
    }

    private companion object {
        val PIXEL = ClientDevice("dev-1", "Pixel 9", ClientKind.Android, connected = true, lastSeenMs = 2_000)
        val TABLET = ClientDevice("dev-2", "Tablet", ClientKind.Android, connected = false, lastSeenMs = 1_000)
        val NEVER = ClientDevice("dev-3", "Old phone", ClientKind.Android, connected = false, lastSeenMs = null)
    }
}
