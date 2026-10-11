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
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.io.path.createTempDirectory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue
import kotlin.test.fail

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
        run(project, "git", "init", "-q")
        val server = ExampleServer.start(repositoryRoot(), port, data)
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

    private fun repositoryRoot(): File {
        var dir: File? = File(System.getProperty("user.dir").orEmpty()).absoluteFile
        while (dir != null) {
            if (File(dir, "Cargo.toml").isFile && File(dir, "mobile").isDirectory) return dir
            dir = dir.parentFile
        }
        fail("the repository root (Cargo.toml + mobile/) was not found above ${System.getProperty("user.dir")}")
    }

    /** `cargo run --example remote_server` を標準入出力で操作する。 */
    private class ExampleServer(private val process: Process) {
        private val lines = LinkedBlockingQueue<String>()

        init {
            Thread {
                process.inputStream.bufferedReader().forEachLine { lines.put(it) }
            }.apply { isDaemon = true }.start()
        }

        /** 起動時の `set_remote_settings` の応答 (ビルドを含むので長く待つ)。 */
        fun firstLine(): String = lines.poll(BUILD_TIMEOUT_MINUTES, TimeUnit.MINUTES)
            ?: fail("the example server printed nothing")

        /** 標準入力のコマンドを 1 つ送り、応答の 1 行を返す。 */
        fun command(line: String): String {
            process.outputStream.write("$line\n".toByteArray())
            process.outputStream.flush()
            return lines.poll(COMMAND_TIMEOUT_SECONDS, TimeUnit.SECONDS) ?: fail("no answer to `$line`")
        }

        fun quit() {
            runCatching {
                process.outputStream.write("quit\n".toByteArray())
                process.outputStream.flush()
            }
            if (!process.waitFor(QUIT_TIMEOUT_SECONDS, TimeUnit.SECONDS)) process.destroyForcibly()
        }

        companion object {
            fun start(root: File, port: Int, data: File): ExampleServer {
                val process = ProcessBuilder(
                    "cargo", "run", "-q", "-p", "yhtye-core", "--example", "remote_server", "--",
                    "--port", port.toString(), "--data-dir", data.absolutePath,
                )
                    .directory(root)
                    .redirectError(ProcessBuilder.Redirect.INHERIT)
                    .start()
                return ExampleServer(process)
            }
        }
    }

    private fun run(dir: File, vararg command: String) {
        val exit = ProcessBuilder(*command).directory(dir).inheritIO().start().waitFor()
        assertEquals(0, exit, command.joinToString(" "))
    }

    private companion object {
        const val DEFAULT_TEST_PORT = 14230
        const val STATE_TIMEOUT_MS = 30_000L
        const val BUILD_TIMEOUT_MINUTES = 10L
        const val COMMAND_TIMEOUT_SECONDS = 60L
        const val QUIT_TIMEOUT_SECONDS = 15L
    }
}
