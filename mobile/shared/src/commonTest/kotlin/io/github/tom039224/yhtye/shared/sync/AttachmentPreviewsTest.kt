package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 添付の中身のメモリキャッシュ ([AttachmentPreviews])。 */
@OptIn(ExperimentalCoroutinesApi::class)
class AttachmentPreviewsTest {
    private val core = FakeCore(PROJECT, FULL_RUN.start)
    private val shot = byteArrayOf(9, 8, 7)

    private fun TestScope.previews(cacheBytes: Long = 1024, maxItemBytes: Long = 512): AttachmentPreviews =
        AttachmentPreviews(core, backgroundScope, AttachmentPreviews.Options(cacheBytes, maxItemBytes, workContext = EmptyCoroutineContext))

    /** コアが保存している画像。 */
    private fun stored(id: String = "att-1", bytes: ByteArray = shot): AttachmentMeta {
        val meta = AttachmentMeta(id, "$id.png", "image/png", bytes.size.toLong(), AttachmentKind.Image, AttachmentDelivery.Native)
        core.stored[id] = meta to bytes
        return meta
    }

    private fun AttachmentPreview.bytes(): ByteArray = assertIs<AttachmentPreview.Loaded>(this).bytes

    @Test
    fun fetchesTheFileFromTheCoreAndKeepsItInMemory() = runTest {
        val p = previews()
        val meta = stored()
        assertNull(p.peek("repo", "C-1", "att-1"))
        assertContentEquals(shot, p.load("repo", "C-1", meta).bytes())
        assertContentEquals(shot, p.peek("repo", "C-1", "att-1"))
        assertContentEquals(shot, p.load("repo", "C-1", meta).bytes())
        assertEquals(listOf(ApiCommand.GetAttachment("repo", "C-1", "att-1")), core.callsOf<ApiCommand.GetAttachment>())
    }

    @Test
    fun fetchesOnceWhenTheSameFileIsAskedForAtTheSameTime() = runTest {
        val p = previews()
        val meta = stored()
        val gate = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.GetAttachment) gate.await() }
        val a = async { p.load("repo", "C-1", meta) }
        val b = async { p.load("repo", "C-1", meta) }
        runCurrent()
        gate.complete(Unit)
        assertContentEquals(shot, a.await().bytes())
        assertContentEquals(shot, b.await().bytes())
        assertEquals(1, core.callsOf<ApiCommand.GetAttachment>().size)
    }

    @Test
    fun goesOnFetchingWhenTheCallerLeaves() = runTest {
        val p = previews()
        val meta = stored()
        val gate = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.GetAttachment) gate.await() }
        val leaving = async { p.load("repo", "C-1", meta) }
        runCurrent()
        leaving.cancel()
        gate.complete(Unit)
        runCurrent()
        // 画面が離れても取得は終わり、戻ってきたら取り直さない
        assertContentEquals(shot, p.peek("repo", "C-1", "att-1"))
        assertContentEquals(shot, p.load("repo", "C-1", meta).bytes())
        assertEquals(1, core.callsOf<ApiCommand.GetAttachment>().size)
    }

    @Test
    fun doesNotFetchWhatIsTooLarge() = runTest {
        val p = previews(maxItemBytes = 2)
        val meta = stored()
        assertEquals(AttachmentPreview.TooLarge, p.load("repo", "C-1", meta))
        assertEquals(0, core.callsOf<ApiCommand.GetAttachment>().size)
        p.put("repo", "C-1", "att-1", shot)
        assertNull(p.peek("repo", "C-1", "att-1"))
    }

    @Test
    fun reportsAFailureAndTriesAgainTheNextTime() = runTest {
        val p = previews()
        val meta = stored()
        core.fail(ApiCommand.GetAttachment::class, ApiErrorCode.NotFound, "no attachment att-1")
        val failed = assertIs<AttachmentPreview.Failed>(p.load("repo", "C-1", meta))
        assertTrue(failed.message.contains("no attachment att-1"))
        assertNull(p.peek("repo", "C-1", "att-1"))

        core.failures.clear()
        assertContentEquals(shot, p.load("repo", "C-1", meta).bytes())
        assertEquals(2, core.callsOf<ApiCommand.GetAttachment>().size)
    }

    @Test
    fun dropsTheOneUsedLeastRecentlyWhenFull() = runTest {
        val p = previews(cacheBytes = 10)
        val first = stored("a", ByteArray(4))
        p.put("repo", "C-1", "a", ByteArray(4))
        p.put("repo", "C-1", "b", ByteArray(4))
        // a を使うと b が一番長く使われていない
        p.load("repo", "C-1", first)
        p.put("repo", "C-1", "c", ByteArray(4))
        assertNotNull(p.peek("repo", "C-1", "a"))
        assertNull(p.peek("repo", "C-1", "b"))
        assertNotNull(p.peek("repo", "C-1", "c"))
        assertEquals(0, core.callsOf<ApiCommand.GetAttachment>().size)
    }

    @Test
    fun replacingAnEntryDoesNotCountItTwice() = runTest {
        val p = previews(cacheBytes = 10)
        p.put("repo", "C-1", "a", ByteArray(6))
        p.put("repo", "C-1", "a", ByteArray(6))
        p.put("repo", "C-1", "b", ByteArray(4))
        assertNotNull(p.peek("repo", "C-1", "a"))
        assertNotNull(p.peek("repo", "C-1", "b"))
    }

    @Test
    fun forgetsOnlyTheChatThatWasDeleted() = runTest {
        val p = previews()
        p.put("repo", "C-1", "a", shot)
        p.put("repo", "C-2", "a", shot)
        p.put("other", "C-1", "a", shot)
        p.forgetChat("repo", "C-1")
        assertNull(p.peek("repo", "C-1", "a"))
        assertNotNull(p.peek("repo", "C-2", "a"))
        assertNotNull(p.peek("other", "C-1", "a"))
    }
}
