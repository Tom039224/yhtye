package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.client.ConnectionLostException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.io.encoding.Base64
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.fail

/** 入力中の添付 (ステージ・破棄・送信) の同期層。コアは [FakeCore]。 */
@OptIn(ExperimentalCoroutinesApi::class)
class AttachmentComposerTest {
    private val core = FakeCore(PROJECT, FULL_RUN.start).also { it.log = durable(FULL_RUN) }
    private val image = byteArrayOf(1, 2, 3, 4)
    private val note = "hello".encodeToByteArray()

    private class Rig(val composer: AttachmentComposer, val previews: AttachmentPreviews)

    private fun TestScope.rig(maxBytes: Long = MAX_ATTACHMENT_BYTES): Rig {
        val previews = AttachmentPreviews(core, backgroundScope, AttachmentPreviews.Options(workContext = EmptyCoroutineContext))
        val options = AttachmentComposer.Options(maxBytes = maxBytes, workContext = EmptyCoroutineContext)
        return Rig(AttachmentComposer(core, backgroundScope, previews, options), previews)
    }

    private fun AttachmentComposer.of(chat: String = "C-1", project: String = "repo") = pending(project, chat)

    private fun PendingAttachment.failure(): PendingAttachment.Upload.Failed =
        upload as? PendingAttachment.Upload.Failed ?: fail("not failed: $upload")

    // ---- ステージ ----

    @Test
    fun showsTheFileAsUploadingAndThenStagedWithTheCoresPrediction() = runTest {
        val r = rig()
        val gate = CompletableDeferred<Unit>()
        core.before = { gate.await() }
        r.composer.stage("repo", "C-1", "shot.png", "image/png", image)
        runCurrent()
        val uploading = r.composer.of().single()
        assertEquals("shot.png", uploading.name)
        assertEquals(4L, uploading.size)
        assertEquals(PendingAttachment.Upload.Uploading, uploading.upload)
        assertFalse(r.composer.of().isSettled())

        gate.complete(Unit)
        runCurrent()
        val staged = r.composer.of().single()
        val meta = assertNotNull(staged.meta)
        assertEquals(listOf(meta.id), r.composer.of().stagedIds())
        assertEquals(AttachmentKind.Image, meta.kind)
        assertEquals(AttachmentDelivery.Native, meta.delivery)
        assertTrue(r.composer.of().isSettled())
        assertEquals(
            ApiCommand.StageAttachment("repo", "C-1", "shot.png", "image/png", Base64.encode(image)),
            core.callsOf<ApiCommand.StageAttachment>().single(),
        )
        assertContentEquals(image, core.stored.getValue(meta.id).second)
    }

    @Test
    fun takesTheCoresSafeNameAndSizeOnceStaged() = runTest {
        val r = rig()
        r.composer.stage("repo", "C-1", "a/b.txt", null, note)
        runCurrent()
        val staged = r.composer.of().single()
        assertEquals(staged.meta?.name, staged.name)
        assertEquals(5L, staged.size)
        // 種類が分からない (null) ままコアに送る: コアが中身で決める
        assertNull(core.callsOf<ApiCommand.StageAttachment>().single().mime)
    }

    @Test
    fun sendsTheFilesOneAtATimeInTheOrderTheyWereAdded() = runTest {
        val r = rig()
        val gate = CompletableDeferred<Unit>()
        var held = false
        core.before = { cmd ->
            if (cmd is ApiCommand.StageAttachment && !held) {
                held = true
                gate.await()
            }
        }
        r.composer.stage("repo", "C-1", "one.txt", "text/plain", note)
        r.composer.stage("repo", "C-1", "two.txt", "text/plain", note)
        runCurrent()
        // 1 つ目の応答が来るまで 2 つ目は送らない (大きなデータが同時にメモリに載らない)
        assertEquals(1, core.callsOf<ApiCommand.StageAttachment>().size)
        assertEquals(listOf("one.txt", "two.txt"), r.composer.of().map { it.name })
        assertEquals(PendingAttachment.Upload.Uploading, r.composer.of()[1].upload)

        gate.complete(Unit)
        runCurrent()
        assertEquals(listOf("one.txt", "two.txt"), core.callsOf<ApiCommand.StageAttachment>().map { it.name })
        assertEquals(listOf("att-1", "att-2"), r.composer.of().stagedIds())
    }

    @Test
    fun keepsEachChatsFilesApart() = runTest {
        val r = rig()
        r.composer.stage("repo", "C-1", "one.txt", "text/plain", note)
        r.composer.stage("repo", "C-2", "two.txt", "text/plain", note)
        r.composer.stage("other", "C-1", "three.txt", "text/plain", note)
        runCurrent()
        assertEquals(listOf("one.txt"), r.composer.of("C-1", "repo").map { it.name })
        assertEquals(listOf("two.txt"), r.composer.of("C-2", "repo").map { it.name })
        assertEquals(listOf("three.txt"), r.composer.of("C-1", "other").map { it.name })
        assertEquals(emptyList(), r.composer.of("C-3"))
        assertEquals(listOf("one.txt"), r.composer.state.value.of("repo", "C-1").map { it.name })
    }

    @Test
    fun readsTheFileOnlyWhenItsTurnComes() = runTest {
        val r = rig()
        var reads = 0
        r.composer.stage("repo", "C-1", "lazy.txt", "text/plain", null) {
            reads++
            note
        }
        assertEquals(0, reads)
        runCurrent()
        assertEquals(1, reads)
        assertEquals(5L, r.composer.of().single().size)
    }

    // ---- 失敗 ----

    @Test
    fun refusesAFileOverTheLimitWithoutReadingIt() = runTest {
        val r = rig(maxBytes = 8)
        r.composer.stage("repo", "C-1", "big.bin", null, 9L) { fail("read a file that is over the limit") }
        runCurrent()
        val failed = r.composer.of().single().failure()
        assertFalse(failed.retryable)
        assertEquals(tooLargeMessage(9, 8), failed.message)
        assertEquals(0, core.callsOf<ApiCommand.StageAttachment>().size)
        r.composer.retry("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(0, core.callsOf<ApiCommand.StageAttachment>().size)
    }

    @Test
    fun refusesAFileThatTurnsOutToBeTooLargeWhenRead() = runTest {
        val r = rig(maxBytes = 8)
        r.composer.stage("repo", "C-1", "unsized.bin", null, null) { ByteArray(9) }
        r.composer.stage("repo", "C-1", "stopped.bin", null, null) { throw AttachmentTooLargeException("too big for the reader") }
        runCurrent()
        val (first, second) = r.composer.of()
        assertEquals(tooLargeMessage(9, 8), first.failure().message)
        assertFalse(first.failure().retryable)
        assertEquals("too big for the reader", second.failure().message)
        assertFalse(second.failure().retryable)
        assertEquals(0, core.callsOf<ApiCommand.StageAttachment>().size)
    }

    @Test
    fun letsYouRetryAFileThatCouldNotBeRead() = runTest {
        val r = rig()
        var reads = 0
        r.composer.stage("repo", "C-1", "flaky.txt", "text/plain", null) {
            if (reads++ == 0) error("the provider went away")
            note
        }
        runCurrent()
        val failed = r.composer.of().single().failure()
        assertTrue(failed.retryable)
        assertEquals("could not read the file: the provider went away", failed.message)

        r.composer.retry("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertTrue(r.composer.of().isSettled())
        assertEquals(2, reads)
    }

    @Test
    fun letsYouRetryAfterTheConnectionDropped() = runTest {
        val r = rig()
        core.before = { cmd -> if (cmd is ApiCommand.StageAttachment) throw ConnectionLostException() }
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        assertTrue(r.composer.of().single().failure().retryable)

        core.before = null
        r.composer.retry("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(listOf("att-1"), r.composer.of().stagedIds())
    }

    @Test
    fun doesNotOfferARetryForWhatTheCoreRefused() = runTest {
        val r = rig()
        core.fail(ApiCommand.StageAttachment::class, ApiErrorCode.NotFound, "no chat C-1")
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        val failed = r.composer.of().single().failure()
        assertFalse(failed.retryable)
        assertTrue(failed.message.contains("no chat C-1"))
        r.composer.retry("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(1, core.callsOf<ApiCommand.StageAttachment>().size)
    }

    // ---- 外す ----

    @Test
    fun removingAStagedFileDeletesItOnTheCore() = runTest {
        val r = rig()
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        r.composer.remove("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(emptyList(), r.composer.of())
        assertEquals(emptyMap(), core.stored)
        assertEquals(listOf(ApiCommand.DiscardAttachment("repo", "C-1", "att-1")), core.callsOf<ApiCommand.DiscardAttachment>())
        assertEquals(emptyMap(), r.composer.state.value, "a chat without files is not kept")
    }

    @Test
    fun removingAFileThatIsStillUploadingDeletesItWhenTheCoreHasIt() = runTest {
        val r = rig()
        val gate = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.StageAttachment) gate.await() }
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        r.composer.remove("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(emptyList(), r.composer.of())

        gate.complete(Unit)
        runCurrent()
        assertEquals(emptyList(), r.composer.of())
        assertEquals(emptyMap(), core.stored, "the file the core saved after it was removed is deleted")
    }

    @Test
    fun removingAFailedFileOnlyDropsIt() = runTest {
        val r = rig(maxBytes = 3)
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        r.composer.remove("repo", "C-1", r.composer.of().single().localId)
        runCurrent()
        assertEquals(emptyList(), r.composer.of())
        assertEquals(0, core.callsOf<ApiCommand.DiscardAttachment>().size)
    }

    // ---- プレビュー ----

    @Test
    fun keepsTheBytesOfAStagedImageForThePreview() = runTest {
        val r = rig()
        r.composer.stage("repo", "C-1", "shot.png", "image/png", image)
        r.composer.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        val (shot, text) = r.composer.of().mapNotNull { it.meta }
        assertContentEquals(image, r.previews.peek("repo", "C-1", shot.id))
        assertNull(r.previews.peek("repo", "C-1", text.id))
    }

    // ---- 送る ----

    private suspend fun TestScope.openedStore(): AppStore = newStore(core).also { openAndSettle(it) }

    @Test
    fun sendsTheStagedIdsWithTheMessageAndClearsThem() = runTest {
        val store = openedStore()
        store.attachments.stage("repo", "C-1", "shot.png", "image/png", image)
        store.attachments.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        assertTrue(store.sendMessage("look at these"))
        assertEquals(
            ApiCommand.SendUserMessage("repo", "C-1", "look at these", listOf("att-1", "att-2")),
            core.callsOf<ApiCommand.SendUserMessage>().single(),
        )
        assertEquals(emptyList(), store.attachments.pending("repo", "C-1"))
        // 送ったファイルはコアに残る (送信済み。discard しない)
        assertEquals(listOf("att-1", "att-2"), core.stored.keys.toList())
        assertEquals(0, core.callsOf<ApiCommand.DiscardAttachment>().size)
    }

    @Test
    fun sendsAFileWithoutText() = runTest {
        val store = openedStore()
        store.attachments.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        assertTrue(canSendMessage("", store.attachments.pending("repo", "C-1")))
        assertTrue(store.sendMessage(""))
        assertEquals(ApiCommand.SendUserMessage("repo", "C-1", "", listOf("att-1")), core.callsOf<ApiCommand.SendUserMessage>().single())
    }

    @Test
    fun keepsTheFilesWhenTheCoreRefusesTheMessage() = runTest {
        val store = openedStore()
        store.attachments.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        core.fail(ApiCommand.SendUserMessage::class, ApiErrorCode.Unavailable, "the project's orchestration has stopped")
        assertFalse(store.sendMessage("hi"))
        assertEquals(listOf("att-1"), store.attachments.pending("repo", "C-1").stagedIds())

        core.failures.clear()
        assertTrue(store.sendMessage("hi"))
        assertEquals(emptyList(), store.attachments.pending("repo", "C-1"))
    }

    @Test
    fun doesNotSendWhileAFileIsUploading() = runTest {
        val store = openedStore()
        val gate = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.StageAttachment) gate.await() }
        store.attachments.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        assertFalse(canSendMessage("hi", store.attachments.pending("repo", "C-1")))
        assertFalse(store.sendMessage("hi"))
        assertEquals(0, core.callsOf<ApiCommand.SendUserMessage>().size)
        assertTrue(store.state.value.errors.last().message.startsWith("send_user_message failed: "))

        gate.complete(Unit)
        runCurrent()
        assertTrue(store.sendMessage("hi"))
    }

    @Test
    fun doesNotSendAMessageWithoutAFileThatFailed() = runTest {
        val store = openedStore()
        core.before = { cmd -> if (cmd is ApiCommand.StageAttachment) throw ConnectionLostException() }
        store.attachments.stage("repo", "C-1", "a.txt", "text/plain", note)
        runCurrent()
        assertFalse(store.sendMessage("hi"))
        assertEquals(0, core.callsOf<ApiCommand.SendUserMessage>().size)

        store.attachments.remove("repo", "C-1", store.attachments.pending("repo", "C-1").single().localId)
        runCurrent()
        assertTrue(store.sendMessage("hi"))
    }

    @Test
    fun sendsOnlyTheSelectedChatsFiles() = runTest {
        val store = openedStore()
        store.attachments.stage("repo", "C-1", "mine.txt", "text/plain", note)
        store.attachments.stage("repo", "C-2", "theirs.txt", "text/plain", note)
        runCurrent()
        assertTrue(store.sendMessage("hi"))
        assertEquals(listOf("att-1"), core.callsOf<ApiCommand.SendUserMessage>().single().attachments)
        assertEquals(listOf("theirs.txt"), store.attachments.pending("repo", "C-2").map { it.name })
    }

    @Test
    fun forgetsTheFilesOfAChatThatWasDeleted() = runTest {
        val store = openedStore()
        store.attachments.stage("repo", "C-1", "shot.png", "image/png", image)
        runCurrent()
        assertNotNull(store.attachmentPreviews.peek("repo", "C-1", "att-1"))
        store.deleteChat("C-1")
        runCurrent()
        assertEquals(emptyList(), store.attachments.pending("repo", "C-1"))
        assertNull(store.attachmentPreviews.peek("repo", "C-1", "att-1"))
        // コアはチャットと一緒にファイルを消す: クライアントは discard を送らない
        assertEquals(0, core.callsOf<ApiCommand.DiscardAttachment>().size)
    }
}
