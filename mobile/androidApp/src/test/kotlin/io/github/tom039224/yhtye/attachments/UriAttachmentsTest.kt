package io.github.tom039224.yhtye.attachments

import android.app.Application
import android.content.ClipData
import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.test.core.app.ApplicationProvider
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.sync.AttachmentComposer
import io.github.tom039224.yhtye.shared.sync.AttachmentPreviews
import io.github.tom039224.yhtye.shared.sync.AttachmentTooLargeException
import io.github.tom039224.yhtye.shared.sync.PendingAttachment
import io.github.tom039224.yhtye.shared.sync.stagedIds
import io.github.tom039224.yhtye.ui.PROJECT
import io.github.tom039224.yhtye.ui.TestCore
import io.github.tom039224.yhtye.ui.readRecording
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertNull

private const val AUTHORITY = "test.files"

/** テストの提供元 (ContentProvider): Uri の末尾の名前で [FakeFiles.entries] を引く。 */
class FakeFiles : ContentProvider() {
    /** 提供元が教えること。[bytes] が null のファイルは開けない。 */
    class Entry(val name: String?, val size: Long?, val type: String?, val bytes: ByteArray?)

    companion object {
        val entries = HashMap<String, Entry>()
        var opens = 0
        var refuseQuery = false
        lateinit var dir: File
    }

    override fun onCreate(): Boolean = true

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, args: Array<String>?, order: String?): Cursor? {
        if (refuseQuery) throw SecurityException("no permission")
        val entry = entries[uri.lastPathSegment] ?: return null
        return MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)).apply { addRow(arrayOf<Any?>(entry.name, entry.size)) }
    }

    override fun getType(uri: Uri): String? = entries[uri.lastPathSegment]?.type

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        opens++
        val bytes = entries[uri.lastPathSegment]?.bytes ?: throw FileNotFoundException("$uri")
        val file = File.createTempFile("entry", null, dir).apply { writeBytes(bytes) }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, args: Array<String>?): Int = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, args: Array<String>?): Int = 0
}

/** Uri (フォトピッカー・SAF・クリップボード) から名前・種類・中身を読む。提供元は [FakeFiles] (Robolectric)。 */
@RunWith(RobolectricTestRunner::class)
@OptIn(ExperimentalCoroutinesApi::class)
class UriAttachmentsTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val png = byteArrayOf(1, 2, 3, 4)

    @Before
    fun setUp() {
        Robolectric.setupContentProvider(FakeFiles::class.java, AUTHORITY)
        FakeFiles.dir = app.cacheDir
        FakeFiles.entries.clear()
        FakeFiles.opens = 0
        FakeFiles.refuseQuery = false
    }

    @After
    fun tearDown() {
        FakeFiles.entries.clear()
    }

    private fun uri(key: String): Uri = Uri.parse("content://$AUTHORITY/$key")

    private fun reader(maxBytes: Long = 8) = UriAttachments(app.contentResolver, maxBytes, Dispatchers.Unconfined)

    @Test
    fun readsTheNameTypeSizeAndContentTheProviderGives() = runTest {
        FakeFiles.entries["shot"] = FakeFiles.Entry("shot.png", 4, "image/png", png)
        val file = reader().describe(uri("shot"))
        assertEquals(PickedFile(uri("shot"), "shot.png", "image/png", 4), file)
        assertContentEquals(png, reader().read(file))
    }

    @Test
    fun makesANameAndATypeWhenTheProviderGivesNone() = runTest {
        // クリップボードの画像: 名前の列が無く、種類だけ分かる
        FakeFiles.entries["clip1"] = FakeFiles.Entry(null, null, "image/png", png)
        assertEquals(PickedFile(uri("clip1"), "clip1.png", "image/png", null), reader().describe(uri("clip1")))
        // 種類が分からない: 拡張子から
        FakeFiles.entries["doc"] = FakeFiles.Entry("notes.txt", 5, null, null)
        assertEquals("text/plain", reader().describe(uri("doc")).mime)
        // 何も分からない: 種類は null (コアが中身で決める)
        FakeFiles.entries["blob"] = FakeFiles.Entry(null, null, "application/octet-stream", null)
        assertEquals(PickedFile(uri("blob"), "blob", null, null), reader().describe(uri("blob")))
    }

    @Test
    fun describesAUriTheProviderCannotAnswerFor() = runTest {
        FakeFiles.refuseQuery = true
        assertEquals(PickedFile(uri("gone.jpg"), "gone.jpg", "image/jpeg", null), reader().describe(uri("gone.jpg")))
    }

    @Test
    fun refusesAFileOverTheLimitWithoutOpeningIt() = runTest {
        FakeFiles.entries["big"] = FakeFiles.Entry("big.bin", 9, null, ByteArray(9))
        val file = reader().describe(uri("big"))
        assertFailsWith<AttachmentTooLargeException> { reader().read(file) }
        assertEquals(0, FakeFiles.opens)
    }

    @Test
    fun stopsReadingAFileOfUnknownSizeAtTheLimit() = runTest {
        FakeFiles.entries["exact"] = FakeFiles.Entry("exact.bin", null, null, ByteArray(8))
        FakeFiles.entries["over"] = FakeFiles.Entry("over.bin", null, null, ByteArray(9))
        assertEquals(8, reader().read(reader().describe(uri("exact"))).size)
        assertFailsWith<AttachmentTooLargeException> { reader().read(reader().describe(uri("over"))) }
        // 提供元が大きさを偽っていても (小さく言う) 読みながら止める
        FakeFiles.entries["liar"] = FakeFiles.Entry("liar.bin", 1, null, ByteArray(9))
        assertFailsWith<AttachmentTooLargeException> { reader().read(reader().describe(uri("liar"))) }
    }

    @Test
    fun failsToReadWhatCannotBeOpened() = runTest {
        FakeFiles.entries["locked"] = FakeFiles.Entry("locked.bin", 3, null, null)
        assertFailsWith<IOException> { reader().read(reader().describe(uri("locked"))) }
    }

    @Test
    fun addsTheFilesToTheComposerInTheOrderTheyWerePicked() = runTest {
        FakeFiles.entries["shot"] = FakeFiles.Entry("shot.png", 4, "image/png", png)
        FakeFiles.entries["huge"] = FakeFiles.Entry("huge.bin", 99, null, null)
        FakeFiles.entries["note"] = FakeFiles.Entry("note.txt", 5, "text/plain", "hello".encodeToByteArray())
        val recording = readRecording()
        val core = TestCore(recording.end, recording.events)
        val previews = AttachmentPreviews(core, backgroundScope, AttachmentPreviews.Options(workContext = EmptyCoroutineContext))
        val options = AttachmentComposer.Options(maxBytes = 8, workContext = EmptyCoroutineContext)
        val composer = AttachmentComposer(core, backgroundScope, previews, options)

        reader().addTo(composer, PROJECT.id, "C-1", listOf(uri("shot"), uri("huge"), uri("note")))
        runCurrent()

        val pending = composer.pending(PROJECT.id, "C-1")
        assertEquals(listOf("shot.png", "huge.bin", "note.txt"), pending.map { it.name })
        assertEquals(listOf("att-1", "att-2"), pending.stagedIds())
        assertIs<PendingAttachment.Upload.Failed>(pending[1].upload)
        assertEquals(listOf("shot.png", "note.txt"), core.callsOf<ApiCommand.StageAttachment>().map { it.name })
        assertContentEquals(png, core.attachments.getValue("att-1").second)
    }

    @Test
    fun picksOutTheFilesOfTheClipboard() {
        val clip = ClipData.newUri(app.contentResolver, "image", uri("a"))
        clip.addItem(ClipData.Item("just text"))
        clip.addItem(ClipData.Item(Uri.parse("https://example.com/page")))
        clip.addItem(ClipData.Item(uri("b")))
        assertEquals(listOf(uri("a"), uri("b")), clip.fileUris())
        assertNull(ClipData.newPlainText("t", "x").fileUris().firstOrNull())
    }
}
