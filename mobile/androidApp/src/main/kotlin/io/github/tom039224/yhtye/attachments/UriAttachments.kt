package io.github.tom039224.yhtye.attachments

import android.content.ClipData
import android.content.ContentResolver
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import io.github.tom039224.yhtye.shared.sync.AttachmentComposer
import io.github.tom039224.yhtye.shared.sync.AttachmentTooLargeException
import io.github.tom039224.yhtye.shared.sync.MAX_ATTACHMENT_BYTES
import io.github.tom039224.yhtye.shared.sync.formatSize
import io.github.tom039224.yhtye.shared.sync.tooLargeMessage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.io.IOException

// 添付にするファイルを Uri (フォトピッカー・ファイルを選ぶ画面 (SAF)・クリップボード) から読む (ContentResolver)。
// 選んだ Uri は [UriAttachments.addTo] で入力中の添付 ([AttachmentComposer]) に足す。権限は選んだ画面・クリップボードが
// 一時的に与えるもので、読むのは選んだ直後 (アップロードの順番が来たとき) だけ。保存しておく必要は無い。

/** 名前の取れない Uri のファイル名 (種類から拡張子を付ける)。 */
private const val DEFAULT_NAME = "attachment"
private const val UNKNOWN_MIME = "application/octet-stream"
private const val READ_BUFFER_BYTES = 64 * 1024

/** Uri の指すファイルの、中身を読まずに分かること。 */
data class PickedFile(
    val uri: Uri,
    /** 表示名 (提供元の名前。無ければ Uri の末尾と種類から作る)。コアがさらに安全な名前にする。 */
    val name: String,
    /** 種類 (提供元の言うもの、無ければ拡張子から。分からなければ null。コアが中身で直す)。 */
    val mime: String?,
    /** バイト数 (提供元が教えなければ null)。 */
    val size: Long?,
)

class UriAttachments(
    private val resolver: ContentResolver,
    /** 読める大きさの上限 (バイト)。 */
    private val maxBytes: Long = MAX_ATTACHMENT_BYTES,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    /** [uri] の名前・種類・大きさ。提供元が答えなくても失敗せず、分かる範囲で埋める。 */
    suspend fun describe(uri: Uri): PickedFile = withContext(io) {
        val (queriedName, size) = queryNameAndSize(uri)
        val name = queriedName?.takeIf { it.isNotBlank() } ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
        val mime = queryMime(uri) ?: name?.let(::mimeFromExtension)
        PickedFile(uri, withExtension(name ?: DEFAULT_NAME, mime), mime, size)
    }

    /**
     * [file] の中身。[maxBytes] を超える (大きさが分かっていれば読む前に、分からなければ読みながら) と [AttachmentTooLargeException]。
     * 開けない・読めないときは [IOException] か [SecurityException] (権限が切れた)。
     */
    suspend fun read(file: PickedFile): ByteArray = withContext(io) {
        file.size?.takeIf { it > maxBytes }?.let { throw AttachmentTooLargeException(tooLargeMessage(it, maxBytes)) }
        val input = resolver.openInputStream(file.uri) ?: throw IOException("cannot open ${file.uri}")
        input.use { stream ->
            val out = ByteArrayOutputStream(file.size?.coerceIn(0, maxBytes)?.coerceAtMost(Int.MAX_VALUE.toLong())?.toInt() ?: READ_BUFFER_BYTES)
            val buffer = ByteArray(READ_BUFFER_BYTES)
            var total = 0L
            while (true) {
                ensureActive()
                val n = stream.read(buffer)
                if (n < 0) break
                total += n
                if (total > maxBytes) throw AttachmentTooLargeException("the file is too large (more than ${formatSize(maxBytes)})")
                out.write(buffer, 0, n)
            }
            out.toByteArray()
        }
    }

    /**
     * [uris] (選んだ順) を [chat] の入力中の添付に足す。名前と大きさだけ先に調べるので、チップはすぐ出る。中身は
     * アップロードの順番が来たとき読む (大きすぎるものは読まずに失敗として出る)。
     */
    suspend fun addTo(composer: AttachmentComposer, project: String, chat: String, uris: List<Uri>) {
        for (uri in uris) {
            val file = describe(uri)
            composer.stage(project, chat, file.name, file.mime, file.size) { read(file) }
        }
    }

    private fun queryNameAndSize(uri: Uri): Pair<String?, Long?> {
        val projection = arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        return try {
            resolver.query(uri, projection, null, null, null)?.use { cursor ->
                if (!cursor.moveToFirst()) return@use null to null
                val nameAt = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                val sizeAt = cursor.getColumnIndex(OpenableColumns.SIZE)
                val name = if (nameAt >= 0 && !cursor.isNull(nameAt)) cursor.getString(nameAt) else null
                val size = if (sizeAt >= 0 && !cursor.isNull(sizeAt)) cursor.getLong(sizeAt).takeIf { it >= 0 } else null
                name to size
            } ?: (null to null)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // 提供元が答えられない (落ちている・権限が無い): 読むときに同じ理由で失敗するので、ここでは名前も大きさも無しで続ける
            null to null
        }
    }

    private fun queryMime(uri: Uri): String? = try {
        resolver.getType(uri)?.takeIf { it.isNotBlank() && it != UNKNOWN_MIME }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        null
    }
}

private fun mimeFromExtension(name: String): String? {
    val extension = name.substringAfterLast('.', "").lowercase()
    return if (extension.isEmpty()) null else MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension)
}

/** 拡張子が無く種類が分かるなら付ける (名前の無い Uri のクリップボードの画像など)。 */
private fun withExtension(name: String, mime: String?): String {
    if ('.' in name) return name
    val extension = mime?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) } ?: return name
    return "$name.$extension"
}

/** クリップボードの項目のうち、ファイルを指す (`content:`) もの (貼り付けた画像など)。文字だけの項目・リンクは含まない。 */
fun ClipData.fileUris(): List<Uri> = (0 until itemCount).mapNotNull { index ->
    getItemAt(index).uri?.takeIf { it.scheme == ContentResolver.SCHEME_CONTENT }
}
