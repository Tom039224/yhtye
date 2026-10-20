package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivered
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

private fun meta(
    id: String = "att-1",
    name: String = "shot.png",
    kind: AttachmentKind = AttachmentKind.Image,
    size: Long = 4,
    delivery: AttachmentDelivery = AttachmentDelivery.Native,
): AttachmentMeta = AttachmentMeta(id, name, "image/png", size, kind, delivery)

private fun pending(id: Long, upload: PendingAttachment.Upload): PendingAttachment = PendingAttachment(id, "f$id", null, null, upload)

private fun staged(id: Long) = pending(id, PendingAttachment.Upload.Staged(meta("att-$id")))

/** 添付の判定・表示と、会話 (`TranscriptItem.User`) への写し方。 */
class AttachmentsTest {
    // ---- 送れるか ----

    @Test
    fun canSendWithTextOrWithFilesOrBoth() {
        assertFalse(canSendMessage("", emptyList()))
        assertFalse(canSendMessage("  \n", emptyList()))
        assertTrue(canSendMessage("hi", emptyList()))
        assertTrue(canSendMessage("", listOf(staged(1))))
        assertTrue(canSendMessage("hi", listOf(staged(1), staged(2))))
    }

    @Test
    fun waitsForFilesThatAreUploadingAndStopsAtFailedOnes() {
        val uploading = pending(2, PendingAttachment.Upload.Uploading)
        val failed = pending(3, PendingAttachment.Upload.Failed("nope", retryable = true))
        assertFalse(canSendMessage("hi", listOf(staged(1), uploading)))
        assertFalse(canSendMessage("hi", listOf(staged(1), failed)))
        assertFalse(canSendMessage("", listOf(uploading)))
        assertEquals(listOf("att-1"), listOf(staged(1), uploading, failed).stagedIds())
    }

    @Test
    fun previewsImagesThatAreNotTooLarge() {
        assertTrue(meta(size = MAX_PREVIEW_BYTES).canPreview())
        assertFalse(meta(size = MAX_PREVIEW_BYTES + 1).canPreview())
        assertFalse(meta(kind = AttachmentKind.Text).canPreview())
        assertFalse(meta(kind = AttachmentKind.File).canPreview())
        assertTrue(meta(size = 100).canPreview(maxBytes = 100))
        assertFalse(meta(size = 101).canPreview(maxBytes = 100))
    }

    // ---- 大きさの表示 ----

    @Test
    fun formatsSizes() {
        assertEquals("0 B", formatSize(0))
        assertEquals("1023 B", formatSize(1023))
        assertEquals("1.0 KB", formatSize(1024))
        assertEquals("1.5 KB", formatSize(1536))
        assertEquals("1000.0 KB", formatSize(1000L * 1024))
        assertEquals("1.0 MB", formatSize(1024L * 1024))
        assertEquals("10.0 MB", formatSize(MAX_ATTACHMENT_BYTES))
        assertEquals("5.0 GB", formatSize(5L * 1024 * 1024 * 1024))
    }

    @Test
    fun saysHowLargeAFileIsAndWhatTheLimitIs() {
        assertEquals("the file is too large (12.0 MB; the limit is 10.0 MB)", tooLargeMessage(12L * 1024 * 1024, MAX_ATTACHMENT_BYTES))
    }

    // ---- 会話 ----

    private fun fold(vararg events: ApiEvent): List<TranscriptItem> =
        events.fold(emptyMap<String, List<TranscriptItem>>()) { t, e -> applyTranscriptEvent(t, e) }[ORCHESTRATOR].orEmpty()

    private fun promptedWith(seq: Long, vararg delivered: AttachmentDelivered) =
        ev(seq, ApiEventBody.Prompted(ORCHESTRATOR, "p", delivered.toList()))

    @Test
    fun aUserMessageCarriesItsAttachments() {
        val files = listOf(meta("att-1"), meta("att-2", "notes.txt", AttachmentKind.Text, delivery = AttachmentDelivery.Path))
        val user = assertIs<TranscriptItem.User>(fold(userMessage(1, 7, "look", attachments = files)).single())
        assertEquals(files, user.attachments)
        assertEquals(emptyList(), assertIs<TranscriptItem.User>(fold(userMessage(1, 7, "plain")).single()).attachments)
    }

    @Test
    fun theFinalDeliveryReplacesThePrediction() {
        val files = listOf(meta("att-1"), meta("att-2", "notes.txt", AttachmentKind.Text))
        val items = fold(
            userMessage(1, 7, "look", attachments = files),
            userMessage(2, 8, "again", attachments = listOf(meta("att-3"))),
            // 7 の att-1 は能力が無く path に落ちた。att-2 は予測のまま。8 はまだ送られていない
            promptedWith(3, AttachmentDelivered(7, "att-1", AttachmentDelivery.Path), AttachmentDelivered(7, "att-2", AttachmentDelivery.Native)),
        )
        val (first, second) = items.map { assertIs<TranscriptItem.User>(it) }
        assertEquals(listOf(AttachmentDelivery.Path, AttachmentDelivery.Native), first.attachments.map { it.delivery })
        assertEquals(listOf(AttachmentDelivery.Native), second.attachments.map { it.delivery })
    }

    @Test
    fun theFinalDeliveryOfADifferentMessageOrAnUnknownOneChangesNothing() {
        val user = userMessage(1, 7, "look", attachments = listOf(meta("att-1")))
        val items = fold(
            user,
            promptedWith(2, AttachmentDelivered(9, "att-1", AttachmentDelivery.Path)),
            promptedWith(3, AttachmentDelivered(7, "att-1", AttachmentDelivery.Unrecognized)),
        )
        assertEquals(listOf(AttachmentDelivery.Native), assertIs<TranscriptItem.User>(items.single()).attachments.map { it.delivery })
    }

    @Test
    fun aPromptWithoutAttachmentsLeavesTheConversationAsItWas() {
        val items = fold(userMessage(1, 7, "plain"), promptedWith(2))
        assertEquals(listOf<TranscriptItem>(TranscriptItem.User(1, TS_BASE + 1, 7, "plain")), items)
    }
}
