package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.ExampleServer
import io.github.tom039224.yhtye.shared.client.KtorWsTransport
import io.github.tom039224.yhtye.shared.client.YhtyeClient
import io.github.tom039224.yhtye.shared.client.createHttpClient
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.PairingClient
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.parsePairingUri
import io.ktor.client.HttpClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assume.assumeTrue
import java.io.File
import java.util.Collections
import kotlin.io.path.createTempDirectory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

/**
 * 同期層 ([AppStore]) と本物のコアの結合テスト。example のリモートサーバー (本物の Tailscale) を偽エージェントで動かし、
 * 2 つの端末をペアリングしてそれぞれの同期層で同じプロジェクトを見る。手動で実行する:
 *
 * ```sh
 * cd mobile
 * YHTYE_REAL_SERVER_TEST=1 ./gradlew :shared:testAndroidHostTest --tests '*SyncRealServerTest*'
 * ```
 *
 * この PC が Tailscale につながっていること。ポートは `YHTYE_SYNC_SERVER_PORT` (既定 14231)。偽エージェントは
 * テストが `cargo build -p yhtye-fake-agent` で作る (シナリオは core_facade.rs の `full_run_scripts` と同じ:
 * 依頼 → グループとタスク → 実装 → レビュー → マージ → オーケストレータの報告)。
 */
class SyncRealServerTest {
    @Test
    fun twoClientsFollowTheRealCoreThroughARequestToTheMerge() {
        assumeTrue("set YHTYE_REAL_SERVER_TEST=1 to run", System.getenv("YHTYE_REAL_SERVER_TEST") == "1")
        val port = System.getenv("YHTYE_SYNC_SERVER_PORT")?.toInt() ?: DEFAULT_TEST_PORT
        val root = ExampleServer.repositoryRoot()
        ExampleServer.run(root, "cargo", "build", "-q", "-p", "yhtye-fake-agent")
        val fakeAgent = File(System.getenv("CARGO_TARGET_DIR") ?: File(root, "target").path, "debug/yhtye-fake-agent")
        val project = createTempDirectory("yhtye-sync-project").toFile()
        val data = createTempDirectory("yhtye-sync-data").toFile()
        initRepository(project)
        val server = ExampleServer.start(root, port, data, fakeAgentArgs(fakeAgent))
        val http = createHttpClient()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            runBlocking { followTheRun(server, http, scope, project) }
        } finally {
            scope.cancel()
            http.close()
            server.quit()
            project.deleteRecursively()
            data.deleteRecursively()
        }
    }

    private suspend fun followTheRun(server: ExampleServer, http: HttpClient, scope: CoroutineScope, project: File) {
        val status = server.firstLine()
        assertTrue("\"running\":true" in status, "the server did not start: $status")
        // 端末を 2 つペアリングする (コードは同時に 1 つなので順に)
        val a = connect(pair(server, http, "sync test A"), http, scope)
        val b = connect(pair(server, http, "sync test B"), http, scope)
        val turnsA = collect(scope, a)
        val turnsB = collect(scope, b)
        val statusesSeenByB = Collections.synchronizedSet(LinkedHashSet<TaskStatus>())
        scope.launch {
            b.state.collect { s -> s.project?.state?.tasks?.firstOrNull()?.let { statusesSeenByB += it.status } }
        }
        for (store in listOf(a, b)) await(store, "connected") { it.connection is ConnectionState.Connected && it.projectsLoaded }

        // A がプロジェクトを開く: B には projects_changed が届き、一覧に出る
        a.openProject(project.absolutePath)
        await(a, "A loaded the project") { it.project?.phase == LoadPhase.Ready }
        val listed = await(b, "B lists the project") { s -> s.projects.any { it.path.samePath(project) } }
            .projects.first { it.path.samePath(project) }
        b.openProject(listed.path)
        await(b, "B loaded the project") { it.project?.phase == LoadPhase.Ready }

        // A がチャットを作る: B にも出る (チャットの無かったプロジェクトなので B も選ぶ)
        assertTrue(a.createChat(branch = "main"))
        await(a, "A selected its chat") { it.project?.selectedChat == "C-1" }
        await(b, "B shows the chat") { it.project?.selectedChat == "C-1" }

        // A が送る: 両方の会話にユーザーの発言とオーケストレータの返事が載る
        assertTrue(a.sendMessage("Add hello.txt please"))
        for ((name, store) in listOf("A" to a, "B" to b)) {
            await(store, "$name shows the conversation") { s ->
                val items = s.project?.transcripts?.get(orchestratorKey("C-1")).orEmpty()
                items.any { it is TranscriptItem.User && it.text == "Add hello.txt please" } &&
                    items.any { it is TranscriptItem.Text && "I will create a group for this." in it.text }
            }
            // グループとタスクが作られ、タスクが終わってグループがマージされる
            await(store, "$name follows the task to the merge", RUN_TIMEOUT_MS) { s ->
                val state = s.project?.state
                state?.tasks?.singleOrNull()?.status == TaskStatus.Done && state.groups.singleOrNull()?.status == GroupStatus.Done
            }
        }
        assertTrue(TaskStatus.Running in statusesSeenByB, "B saw the task run: $statusesSeenByB")
        assertEquals("hello\n", File(project, "hello.txt").readText())

        // オーケストレータの報告でユーザーの番になる (両方の端末が気づく)
        for ((name, turns) in listOf("A" to turnsA, "B" to turnsB)) {
            withTimeout(RUN_TIMEOUT_MS) { while (turns.none { it.chat == "C-1" }) kotlinx.coroutines.delay(POLL_MS) }
            val turn = turns.first { it.chat == "C-1" }
            assertTrue("Done: hello.txt is on main." in turn.lastMessage.orEmpty(), "$name: $turn")
            assertEquals(1, turns.size, "$name: only the final turn is the user's: $turns")
        }

        // 逆向き: B の改名が A に届く
        b.renameChat("C-1", "Greeting file")
        await(a, "A shows B's title") { s -> s.project?.chats?.find { it.id == "C-1" }?.title == "Greeting file" }
        a.close()
        b.close()
    }

    private suspend fun pair(server: ExampleServer, http: HttpClient, name: String): Credentials {
        val code = ApiJson.decodeFromString(ApiResponse.Serializer, server.command("code"))
        val target = parsePairingUri(assertIs<ApiResponse.PairingCode>(code).pairing.uri).getOrThrow()
        return assertIs<PairingResult.Paired>(PairingClient(http).pair(target, name)).credentials
    }

    private fun connect(credentials: Credentials, http: HttpClient, scope: CoroutineScope): AppStore {
        val client = YhtyeClient(KtorWsTransport(http, credentials), scope)
        val store = AppStore(client, scope, MemoryPrefs())
        store.setForeground(false)
        store.start()
        client.start()
        return store
    }

    private fun collect(scope: CoroutineScope, store: AppStore): List<UsersTurn> {
        val turns = Collections.synchronizedList(mutableListOf<UsersTurn>())
        scope.launch { store.usersTurns.collect { turns += it } }
        return turns
    }

    private suspend fun await(
        store: AppStore,
        what: String,
        timeoutMs: Long = STATE_TIMEOUT_MS,
        predicate: (AppState) -> Boolean,
    ): AppState = try {
        withTimeout(timeoutMs) { store.state.first(predicate) }
    } catch (e: kotlinx.coroutines.TimeoutCancellationException) {
        throw AssertionError("timed out waiting until $what: ${store.state.value.summary()}", e)
    }

    private fun AppState.summary(): String {
        val view = project
        return "connection=$connection projects=${projects.map { it.path }} errors=${errors.map { it.message }} " +
            "phase=${view?.phase} chats=${view?.chats?.map { it.id }} selected=${view?.selectedChat} " +
            "tasks=${view?.state?.tasks?.map { it.id to it.status }} groups=${view?.state?.groups?.map { it.id to it.status }}"
    }

    private fun String.samePath(dir: File): Boolean = File(this).canonicalPath == dir.canonicalPath

    private fun initRepository(dir: File) {
        ExampleServer.run(dir, "git", "init", "-q", "-b", "main")
        ExampleServer.run(dir, "git", "config", "user.name", "Test")
        ExampleServer.run(dir, "git", "config", "user.email", "test@example.com")
        File(dir, "README.md").writeText("# test\n")
        ExampleServer.run(dir, "git", "add", "-A")
        ExampleServer.run(dir, "git", "commit", "-qm", "initial")
    }

    /** core_facade.rs の `full_run_scripts` と同じシナリオ。 */
    private fun fakeAgentArgs(bin: File): List<String> = listOf(
        "--fake-agent", bin.absolutePath,
        "--fake-orchestrator", ORCHESTRATOR_SCRIPT,
        "--fake-implementer", IMPLEMENTER_SCRIPT,
        "--fake-reviewer", REVIEWER_SCRIPT,
    )

    private companion object {
        const val DEFAULT_TEST_PORT = 14231
        const val STATE_TIMEOUT_MS = 30_000L
        const val RUN_TIMEOUT_MS = 120_000L
        const val POLL_MS = 100L

        val ORCHESTRATOR_SCRIPT = """
            {"turns": [
              {"match": "[yhtye:user_message]", "actions": [
                {"thought": "The user wants a greeting file. "},
                {"message": "I will create "},
                {"message": "a group for this."},
                {"mcp_call": {"tool": "create_group", "args": {"title": "greeting"}}},
                {"mcp_call": {"tool": "create_task", "args": {
                  "group_id": "${'$'}{group_id}", "title": "add hello.txt", "kind": "code",
                  "steps": [{"kind": "implement"}, {"kind": "review"}], "instruction": "write-hello"}}},
                {"message": "Task T-1 is running."}
              ]},
              {"match": "[yhtye:group_settled]", "actions": [
                {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${'$'}{group}", "summary": "hello.txt added"}}},
                {"message": "Done: hello.txt is on main."}
              ]}
            ]}
        """.trimIndent()
        val IMPLEMENTER_SCRIPT = """
            {"turns": [{"match": "write-hello", "actions": [
              {"message": "Writing the file."},
              {"write_file": {"path": "hello.txt", "text": "hello\n"}},
              {"mcp_call": {"tool": "report_step_done", "args": {"result": "wrote hello.txt"}}}
            ]}]}
        """.trimIndent()
        val REVIEWER_SCRIPT = """
            {"turns": [{"match": "[yhtye:step]", "actions": [
              {"message": "Looks good."},
              {"mcp_call": {"tool": "report_step_done", "args": {"result": "fine", "verdict": "approve"}}}
            ]}]}
        """.trimIndent()
    }
}
