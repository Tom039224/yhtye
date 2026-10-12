package io.github.tom039224.yhtye.shared.client

import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.test.assertEquals
import kotlin.test.fail

/**
 * `cargo run --example remote_server` (本物のコアと本物の Tailscale) を標準入出力で操作する。結合テスト用。
 * [extraArgs] で偽エージェント (`--fake-agent` など) を渡せる。
 */
class ExampleServer private constructor(private val process: Process) {
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
        const val BUILD_TIMEOUT_MINUTES = 10L
        const val COMMAND_TIMEOUT_SECONDS = 60L
        const val QUIT_TIMEOUT_SECONDS = 15L

        fun start(root: File, port: Int, data: File, extraArgs: List<String> = emptyList()): ExampleServer {
            val command = listOf(
                "cargo", "run", "-q", "-p", "yhtye-core", "--example", "remote_server", "--",
                "--port", port.toString(), "--data-dir", data.absolutePath,
            ) + extraArgs
            val process = ProcessBuilder(command)
                .directory(root)
                .redirectError(ProcessBuilder.Redirect.INHERIT)
                .start()
            return ExampleServer(process)
        }

        /** リポジトリの最上位 (Cargo.toml と mobile/ がある)。 */
        fun repositoryRoot(): File {
            var dir: File? = File(System.getProperty("user.dir").orEmpty()).absoluteFile
            while (dir != null) {
                if (File(dir, "Cargo.toml").isFile && File(dir, "mobile").isDirectory) return dir
                dir = dir.parentFile
            }
            fail("the repository root (Cargo.toml + mobile/) was not found above ${System.getProperty("user.dir")}")
        }

        /** [dir] で [command] を実行し、成功を確かめる。 */
        fun run(dir: File, vararg command: String) {
            val exit = ProcessBuilder(*command).directory(dir).inheritIO().start().waitFor()
            assertEquals(0, exit, command.joinToString(" "))
        }
    }
}
