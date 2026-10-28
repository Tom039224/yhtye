package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** ホストのフォルダの閲覧 (`FolderBrowser`)。 */
@OptIn(ExperimentalCoroutinesApi::class)
class FolderBrowserTest {
    private val core = FakeCore(PROJECT, FULL_RUN.start)
    private val opened = mutableListOf<String>()

    private fun TestScope.browser() = FolderBrowser(core, backgroundScope) { opened += it }

    private fun listCalls() = core.callsOf<ApiCommand.ListDirectory>()

    private fun FolderBrowserState.names() = entries.map { it.name }

    @Test
    fun startsEmptyAndOpensHomeWithNoPath() = runTest {
        val b = browser()
        assertEquals(FolderBrowserState(), b.state.value)
        assertFalse(b.state.value.canGoUp)

        b.open()
        val s = b.state.value
        assertEquals("/home/u", s.path)
        assertEquals("/home", s.parent)
        assertEquals("/home/u", s.home)
        assertEquals(listOf("Documents", "repo"), s.names())
        assertTrue(s.canGoUp)
        assertFalse(s.loading)
        assertNull(s.error)
        assertEquals(listOf(ApiCommand.ListDirectory(path = null, showHidden = null)), listCalls())
    }

    @Test
    fun opensAFolderAndKnowsWhichOnesAreRepositories() = runTest {
        val b = browser()
        b.open("/home/u")
        assertEquals(listOf(false, true), b.state.value.entries.map { it.isGitRepo })

        val repo = b.state.value.entries.single { it.name == "repo" }
        b.open(repo.path)
        assertEquals("/home/u/repo", b.state.value.path)
        assertEquals(listOf("src"), b.state.value.names())
        assertEquals("/home/u", b.state.value.parent)
    }

    @Test
    fun goesUpToTheParentAndStopsAtTheRoot() = runTest {
        val b = browser()
        b.open("/home/u/repo")
        b.up()
        assertEquals("/home/u", b.state.value.path)
        b.up()
        b.up()
        assertEquals("/", b.state.value.path)
        assertNull(b.state.value.parent)
        assertFalse(b.state.value.canGoUp)

        val before = listCalls().size
        b.up()
        assertEquals(before, listCalls().size) // ルートより上は無い: 読まない
        assertEquals("/", b.state.value.path)
    }

    @Test
    fun upWithNothingOpenDoesNothing() = runTest {
        val b = browser()
        b.up()
        assertTrue(listCalls().isEmpty())
    }

    @Test
    fun homeGoesBackToTheHomeDirectoryOfTheHost() = runTest {
        val b = browser()
        b.open("/tmp")
        b.home()
        assertEquals("/home/u", b.state.value.path)
        assertEquals(null, listCalls().last().path)
    }

    @Test
    fun aFailureKeepsTheFolderThatWasOpen() = runTest {
        val b = browser()
        b.open("/home/u")
        core.fail(ApiCommand.ListDirectory::class, ApiErrorCode.Forbidden, "/root cannot be read (permission denied)")
        b.open("/root")
        var s = b.state.value
        assertEquals("/root cannot be read (permission denied)", s.error)
        assertFalse(s.loading)
        assertEquals("/home/u", s.path)
        assertEquals(listOf("Documents", "repo"), s.names())

        core.failures.clear()
        b.open("/home/u/repo")
        s = b.state.value
        assertNull(s.error)
        assertEquals("/home/u/repo", s.path)
    }

    @Test
    fun aFolderThatDoesNotExistIsAnErrorNotACrash() = runTest {
        val b = browser()
        b.open("/nope")
        assertEquals("/nope does not exist", b.state.value.error)
        assertNull(b.state.value.path)
    }

    @Test
    fun hiddenFoldersAreShownOnRequestAndTheChoiceSticks() = runTest {
        val b = browser()
        b.open("/home/u")
        b.setShowHidden(true)
        assertEquals(listOf(".config", "Documents", "repo"), b.state.value.names())
        assertEquals(true, listCalls().last().showHidden)

        b.open("/home/u/repo")
        assertEquals(true, listCalls().last().showHidden)
        b.setShowHidden(false)
        assertEquals(null, listCalls().last().showHidden)
        assertFalse(b.state.value.showHidden)
    }

    @Test
    fun hiddenWithNothingOpenOnlyRemembersTheChoice() = runTest {
        val b = browser()
        b.setShowHidden(true)
        assertTrue(listCalls().isEmpty())
        b.open("/home/u")
        assertEquals(".config", b.state.value.names().first())
    }

    @Test
    fun reloadReadsTheOpenFolderAgain() = runTest {
        val b = browser()
        b.open("/home/u/Documents")
        core.fsTree["/home/u/Documents"] = listOf(io.github.tom039224.yhtye.shared.api.generated.DirectoryEntry("new", "/home/u/Documents/new", false))
        b.reload()
        assertEquals(listOf("new"), b.state.value.names())
    }

    @Test
    fun onlyTheLastOpenChangesTheState() = runTest {
        val b = browser()
        val gate = CompletableDeferred<Unit>()
        var calls = 0
        core.before = { if (it is ApiCommand.ListDirectory && calls++ == 0) gate.await() }

        val slow = backgroundScope.launch { b.open("/tmp") }
        runCurrent()
        assertTrue(b.state.value.loading)
        val fast = backgroundScope.launch { b.open("/home/u") }
        runCurrent()
        assertEquals("/home/u", b.state.value.path)

        gate.complete(Unit)
        runCurrent()
        slow.join()
        fast.join()
        assertEquals("/home/u", b.state.value.path)
        assertFalse(b.state.value.loading)
    }

    @Test
    fun aCancelledOpenDoesNotLeaveItLoading() = runTest {
        val b = browser()
        val never = CompletableDeferred<Unit>()
        core.before = { if (it is ApiCommand.ListDirectory) never.await() }
        val job = backgroundScope.launch { b.open("/tmp") }
        runCurrent()
        assertTrue(b.state.value.loading)
        job.cancel()
        runCurrent()
        assertFalse(b.state.value.loading)
        assertNull(b.state.value.error)
    }

    @Test
    fun opensTheChosenFolderAsAProject() = runTest {
        val b = browser()
        b.openAsProject() // 何も開いていない: 何もしない
        assertTrue(opened.isEmpty())

        b.open("/home/u/repo")
        b.openAsProject()
        b.openAsProject("/home/u/repo/src")
        assertEquals(listOf("/home/u/repo", "/home/u/repo/src"), opened)
    }

    @Test
    fun theStoreOpensTheChosenFolderThroughOpenProject() = runTest {
        val s = newStore(core)
        s.folders.open("/home/u")
        val repo = s.folders.state.value.entries.single { it.name == "repo" }
        s.folders.openAsProject(repo.path)
        runCurrent()

        assertEquals(listOf(ApiCommand.OpenProject(repo.path)), core.callsOf<ApiCommand.OpenProject>())
        assertEquals(PROJECT.id, s.state.value.project?.info?.id)
    }
}
