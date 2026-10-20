package io.github.tom039224.yhtye.ui.conversation

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.provider.MediaStore
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.sync.AttachmentPreview
import io.github.tom039224.yhtye.shared.sync.PendingAttachment
import io.github.tom039224.yhtye.shared.sync.TranscriptItem
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.ByteArrayOutputStream
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** 小さな本物の PNG (サムネイルとして読めるもの)。 */
fun pngBytes(): ByteArray {
    val bitmap = Bitmap.createBitmap(8, 8, Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.RED) }
    return ByteArrayOutputStream().also { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
}

private val IMAGE = AttachmentMeta("att-1", "shot.png", "image/png", 120, AttachmentKind.Image, AttachmentDelivery.Native)
private val FILE = AttachmentMeta("att-2", "report.pdf", "application/pdf", 2048, AttachmentKind.File, AttachmentDelivery.Path)

private const val NATIVE = "モデルに直接渡す (native)"
private const val PATH = "ファイルとして保存し、パスで渡す (path)"

/**
 * コンポーザと会話の添付 (PC の ComposerAttachments.test.tsx / transcriptAttachments.test.tsx と同じ事例): + から写真・ファイルを
 * 選ぶ、貼り付け、タイル (左上の送り方・右上の ×・アップロード中と失敗)、添付だけで送る、会話のタイルと拡大表示。
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w400dp-h800dp")
class ComposerAttachmentsTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val attachments = mutableStateOf<List<PendingAttachment>>(emptyList())
    private val added = mutableListOf<List<Uri>>()
    private val removed = mutableListOf<Long>()
    private val sent = mutableListOf<String>()
    private val png = pngBytes()
    private val load = LoadAttachment { meta -> if (meta.id == IMAGE.id) AttachmentPreview.Loaded(png) else AttachmentPreview.Failed("none") }

    private fun staged(localId: Long, meta: AttachmentMeta) =
        PendingAttachment(localId, meta.name, meta.mime, meta.size, PendingAttachment.Upload.Staged(meta))

    private fun showComposer(vararg pending: PendingAttachment) {
        attachments.value = pending.toList()
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                Composer(
                    initialText = "",
                    onTextChange = {},
                    disabledReason = null,
                    sending = false,
                    turnRunning = false,
                    starting = false,
                    mentions = emptyList(),
                    onRemoveMention = {},
                    onSend = { sent += it; true },
                    onCancel = {},
                    attachments = attachments.value,
                    loadAttachment = load,
                    onAddFiles = { added += it },
                    onRemoveAttachment = { removed += it },
                )
            }
        }
        rule.waitForIdle()
    }

    private fun count(tag: String) = rule.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().size

    @Test
    fun stagedTilesShowTheirDeliveryAtTheUpperLeftAndAnXThatRemovesThem() {
        showComposer(staged(1, IMAGE), staged(2, FILE))
        assertEquals(2, count("attachment-tile"))
        // 画像はサムネイル、それ以外は種類 (拡張子)
        rule.waitUntil { count("attachment-thumb") == 1 }
        rule.onNodeWithText("PDF").assertExists()
        // コンポーザの送り方は予測
        rule.onNodeWithContentDescription("$NATIVE · 送信時に確定").assertExists()
        rule.onNodeWithContentDescription("$PATH · 送信時に確定").assertExists()

        rule.onNodeWithContentDescription("report.pdf を外す").performClick()
        assertEquals(listOf(2L), removed)
    }

    @Test
    fun attachmentsAloneCanBeSentOnceAllAreStaged() {
        showComposer(staged(1, FILE))
        rule.onNodeWithTag("composer-send").performClick()
        rule.waitForIdle()
        assertEquals(listOf(""), sent)
    }

    @Test
    fun uploadingAndFailedAttachmentsHoldTheSendBackAndSayWhy() {
        showComposer(PendingAttachment(1, "big.zip", "application/zip", null, PendingAttachment.Upload.Uploading))
        rule.onNodeWithText("添付中… · 終わると送信できます").assertExists()
        rule.onNodeWithContentDescription("big.zip · 添付中").assertExists()
        // 送り方はステージできるまで分からない
        assertEquals(0, rule.onAllNodes(hasContentDescription("送信時に確定", substring = true)).fetchSemanticsNodes().size)
        rule.onNodeWithTag("composer-send").performClick()
        rule.waitForIdle()
        assertTrue(sent.isEmpty())

        attachments.value = listOf(
            PendingAttachment(1, "big.zip", "application/zip", null, PendingAttachment.Upload.Failed("the file is too large", retryable = false)),
        )
        rule.waitForIdle()
        rule.onNodeWithText("添付できなかったファイルを外すと送信できます").assertExists()
        rule.onNodeWithText("the file is too large").assertExists()
        rule.onNodeWithTag("composer-send").performClick()
        rule.waitForIdle()
        assertTrue(sent.isEmpty())
    }

    @Test
    fun thePlusOpensTheFilePickerAndAddsWhatWasPicked() {
        showComposer()
        rule.onNodeWithTag("composer-attach").performClick()
        rule.onNodeWithTag("attach-files").performClick()
        rule.waitForIdle()
        val started = shadowOf(rule.activity).nextStartedActivityForResult
        assertEquals(Intent.ACTION_OPEN_DOCUMENT, started.intent.action)
        assertTrue(started.intent.getBooleanExtra(Intent.EXTRA_ALLOW_MULTIPLE, false))

        val picked = Uri.parse("content://test.files/notes.txt")
        shadowOf(rule.activity).receiveResult(started.intent, Activity.RESULT_OK, Intent().setData(picked))
        rule.waitForIdle()
        assertEquals(listOf(listOf(picked)), added)
    }

    @Test
    fun photosOpenThePhotoPickerForImagesOnly() {
        showComposer()
        rule.onNodeWithTag("composer-attach").performClick()
        rule.onNodeWithTag("attach-photos").performClick()
        rule.waitForIdle()
        val started = shadowOf(rule.activity).nextStartedActivityForResult
        assertEquals(MediaStore.ACTION_PICK_IMAGES, started.intent.action)
        assertEquals("image/*", started.intent.type)
    }

    @Test
    fun pastingAnImageFromTheClipboardAddsItAsAnAttachment() {
        showComposer()
        val image = Uri.parse("content://test.files/pasted.png")
        val clipboard = rule.activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newRawUri("image", image))
        rule.onNodeWithTag("composer-input").performSemanticsAction(SemanticsActions.PasteText)
        rule.waitForIdle()
        assertEquals(listOf(listOf(image)), added)
    }

    @Test
    fun pastingTextStaysText() {
        showComposer()
        val clipboard = rule.activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("text", "hello"))
        rule.onNodeWithTag("composer-input").performSemanticsAction(SemanticsActions.PasteText)
        rule.waitForIdle()
        assertTrue(added.isEmpty())
        rule.onNodeWithTag("composer-input").assertTextEquals("hello")
    }

    @Test
    fun tappingAnImageOpensItLarge() {
        showComposer(staged(1, IMAGE))
        rule.waitUntil { count("attachment-thumb") == 1 }
        rule.onNodeWithTag("attachment-thumb").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("attachment-viewer").assertExists()
        rule.onNodeWithTag("attachment-viewer-close").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("attachment-viewer").assertDoesNotExist()
    }

    private fun showRows(vararg items: TranscriptItem) {
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                Column { items.forEach { TranscriptRow(it, emptySet(), null, load) } }
            }
        }
        rule.waitForIdle()
    }

    @Test
    fun aSentMessageShowsItsAttachmentsBelowTheTextWithTheFinalDeliveryAndNoX() {
        showRows(TranscriptItem.User(1, 1, 7, "see these", listOf(IMAGE, FILE)))
        rule.onNodeWithTag("user-bubble").assertExists()
        assertEquals(2, count("attachment-tile"))
        rule.onNodeWithContentDescription(NATIVE).assertExists()
        rule.onNodeWithContentDescription(PATH).assertExists()
        assertEquals(0, count("attachment-remove"))
        rule.waitUntil { count("attachment-thumb") == 1 }
        val bubble = rule.onNodeWithTag("user-bubble").fetchSemanticsNode().boundsInRoot
        val tiles = rule.onNodeWithTag("sent-attachments").fetchSemanticsNode().boundsInRoot
        assertTrue(tiles.top >= bubble.bottom, "the attachments are below the text")
    }

    @Test
    fun aMessageWithOnlyAttachmentsHasNoEmptyBubble() {
        showRows(TranscriptItem.User(1, 1, 7, "", listOf(FILE)))
        rule.onNodeWithTag("user-bubble").assertDoesNotExist()
        assertEquals(1, count("attachment-tile"))
    }
}
