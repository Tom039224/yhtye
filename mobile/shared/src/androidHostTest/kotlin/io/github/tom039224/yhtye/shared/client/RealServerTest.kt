package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.PairingClient
import io.github.tom039224.yhtye.shared.pairing.PairingFailure
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.parsePairingUri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assume.assumeTrue
import java.io.File
import kotlin.io.path.createTempDirectory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

/**
 * 本物のサーバー (`cargo run -p yhtye-core --example remote_server`) と本物の Tailscale での結合テスト。
 * 手動で実行する:
 *
 * ```sh
 * cd mobile
 * YHTYE_REAL_SERVER_TEST=1 ./gradlew :shared:testAndroidHostTest --tests '*RealServerTest*'
 * ```
 *
 * この PC が Tailscale につながっていること (`tailscale ip -4`)。ポートは `YHTYE_REAL_SERVER_PORT` (既定 14230)。
 * サーバーのデータは一時ディレクトリ (アプリの DB は触らない)。
 */
class RealServerTest {
    @Test
    fun pairCallAndRevokeAgainstTheRealServer() {
        assumeTrue("set YHTYE_REAL_SERVER_TEST=1 to run", System.getenv("YHTYE_REAL_SERVER_TEST") == "1")
        val port = System.getenv("YHTYE_REAL_SERVER_PORT")?.toInt() ?: DEFAULT_TEST_PORT
        val project = createTempDirectory("yhtye-it-project").toFile()
        val data = createTempDirectory("yhtye-it-data").toFile()
        ExampleServer.run(project, "git", "init", "-q")
        val server = ExampleServer.start(ExampleServer.repositoryRoot(), port, data)
        val http = createHttpClient()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            runBlocking { roundTrip(server, http, scope, project) }
        } finally {
            scope.cancel()
            http.close()
            server.quit()
            project.deleteRecursively()
            data.deleteRecursively()
        }
    }

    private suspend fun roundTrip(
        server: ExampleServer,
        http: io.ktor.client.HttpClient,
        scope: CoroutineScope,
        project: File,
    ) {
        val status = server.firstLine()
        assertTrue("\"running\":true" in status, "the server did not start: $status")
        val opened = server.command("open ${project.absolutePath}")
        assertTrue("\"type\":\"project\"" in opened, opened)

        val code = ApiJson.decodeFromString(ApiResponse.Serializer, server.command("code"))
        val target = parsePairingUri(assertIs<ApiResponse.PairingCode>(code).pairing.uri).getOrThrow()
        val pairing = PairingClient(http)

        // コード違い → 401 (5 回までの 1 回を使う)
        val wrong = pairing.pair(target.copy(code = "ZZZZ2222"), "integration test")
        assertIs<PairingFailure.CodeRejected>(assertIs<PairingResult.Failed>(wrong).failure)

        val credentials = assertIs<PairingResult.Paired>(pairing.pair(target, "integration test")).credentials
        assertEquals(target.serverId, credentials.serverId)

        // 知らないトークンは 401 → 失効の扱い (ハンドシェイク失敗の理由を GET /ws で確かめる経路)
        val stranger = YhtyeClient(KtorWsTransport(http, credentials.copy(token = "0".repeat(64))), scope)
        stranger.start()
        withTimeout(STATE_TIMEOUT_MS) { stranger.state.first { it == ConnectionState.Revoked } }
        stranger.close()

        val client = YhtyeClient(KtorWsTransport(http, credentials), scope)
        client.start()
        assertIs<ApiResponse.Pong>(client.call(ApiCommand.Ping))
        val projects = assertIs<ApiResponse.Projects>(client.call(ApiCommand.ListProjects)).projects
        assertTrue(projects.any { File(it.path).canonicalPath == project.canonicalPath }, "$projects")
        val local = runCatching { client.call(ApiCommand.ListRemoteDevices) }.exceptionOrNull()
        assertEquals(ApiErrorCode.Forbidden, assertIs<ApiException>(local).error.code)

        revoke(server, credentials)
        withTimeout(STATE_TIMEOUT_MS) { client.state.first { it == ConnectionState.Revoked } }
        client.close()
        assertTrue(credentials.deviceId !in server.command("devices"))
    }

    private fun revoke(server: ExampleServer, credentials: Credentials) {
        val devices = server.command("revoke ${credentials.deviceId}")
        assertTrue("\"type\":\"remote_devices\"" in devices, devices)
    }

    private companion object {
        const val DEFAULT_TEST_PORT = 14230
        const val STATE_TIMEOUT_MS = 30_000L
    }
}
