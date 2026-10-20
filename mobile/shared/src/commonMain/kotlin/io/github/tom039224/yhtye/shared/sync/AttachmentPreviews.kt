package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.client.RpcException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.coroutines.CoroutineContext
import kotlin.io.encoding.Base64

/** 添付の中身を取れたか ([AttachmentPreviews.load] の結果)。 */
sealed interface AttachmentPreview {
    /** [bytes] はファイルそのもの (画像ならエンコードされたままの PNG / JPEG / GIF / WebP)。書き換えない。 */
    class Loaded(val bytes: ByteArray) : AttachmentPreview

    /** プレビューにするには大きすぎる ([AttachmentPreviews.Options.maxItemBytes])。取りには行かない。 */
    data object TooLarge : AttachmentPreview

    data class Failed(val message: String) : AttachmentPreview
}

/**
 * 添付の中身 (`get_attachment`) をメモリにキャッシュする。会話の添付の画像を出すためのもの。
 *
 * - 同じ添付を同時に求められても、取りに行くのは 1 回 (求めた画面が離れても取得は続き、キャッシュに入る)。
 * - 置く大きさは合計 [Options.cacheBytes] まで。あふれたら使ってから最も長いものを捨てる。
 * - 自分で送った画像はステージしたときに入る ([put]) ので、会話に出るとき取り直さない。
 * - 失敗は覚えない (次に求めたとき取りに行く)。
 */
class AttachmentPreviews(
    private val core: CoreConnection,
    private val scope: CoroutineScope,
    private val options: Options = Options(),
) {
    data class Options(
        /** キャッシュに置く合計の大きさ (バイト)。 */
        val cacheBytes: Long = 32L * 1024 * 1024,
        /** これより大きいものは取りに行かず、キャッシュにも置かない。 */
        val maxItemBytes: Long = MAX_PREVIEW_BYTES,
        val timeoutMs: Long = 60_000,
        /** base64 の復号 (大きなデータの処理) の実行先。テストは呼び出し元のまま ([EmptyCoroutineContext])。 */
        val workContext: CoroutineContext = Dispatchers.Default,
    )

    private data class Key(val project: String, val chat: String, val id: String)

    /** 使った順 (後ろほど新しい) に並べた不変のキャッシュ。どのスレッドからでも [peek] できる。 */
    private class Cache(val entries: Map<Key, ByteArray> = emptyMap(), val bytes: Long = 0) {
        fun with(key: Key, data: ByteArray, limit: Long): Cache {
            val next = LinkedHashMap(entries)
            var total = bytes - (next.remove(key)?.size ?: 0)
            next[key] = data
            total += data.size
            val oldest = next.keys.iterator()
            while (total > limit && oldest.hasNext()) {
                val victim = oldest.next()
                if (victim == key) break
                total -= next.getValue(victim).size
                oldest.remove()
            }
            return Cache(next, total)
        }

        fun touched(key: Key): Cache {
            val data = entries[key] ?: return this
            return Cache(LinkedHashMap(entries).also { it.remove(key); it[key] = data }, bytes)
        }

        fun without(chat: ChatRef): Cache {
            val keep = entries.filterKeys { it.project != chat.project || it.chat != chat.chat }
            return Cache(keep, keep.values.sumOf { it.size.toLong() })
        }
    }

    private val cache = MutableStateFlow(Cache())
    private val lock = Mutex()
    private val fetching = HashMap<Key, Deferred<AttachmentPreview>>()

    /** キャッシュにあればその中身 (取りには行かない。画面を組むとき最初の 1 コマから出すため)。 */
    fun peek(project: String, chat: String, id: String): ByteArray? = cache.value.entries[Key(project, chat, id)]

    /** 添付 [meta] の中身。キャッシュに無ければコアから取る。 */
    suspend fun load(project: String, chat: String, meta: AttachmentMeta): AttachmentPreview {
        val key = Key(project, chat, meta.id)
        cache.value.entries[key]?.let { hit ->
            cache.update { it.touched(key) }
            return AttachmentPreview.Loaded(hit)
        }
        if (meta.size > options.maxItemBytes) return AttachmentPreview.TooLarge
        val fetch = lock.withLock {
            fetching.getOrPut(key) {
                scope.async {
                    try {
                        fetch(key)
                    } finally {
                        lock.withLock { fetching.remove(key) }
                    }
                }
            }
        }
        return fetch.await()
    }

    /** [bytes] を [id] の中身としてキャッシュに置く (大きすぎるものは置かない)。 */
    fun put(project: String, chat: String, id: String, bytes: ByteArray) {
        if (bytes.size > options.maxItemBytes) return
        cache.update { it.with(Key(project, chat, id), bytes, options.cacheBytes) }
    }

    /** 消えたチャットの分を捨てる。 */
    fun forgetChat(project: String, chat: String) {
        cache.update { it.without(ChatRef(project, chat)) }
    }

    private suspend fun fetch(key: Key): AttachmentPreview {
        val reply = try {
            core.expect<ApiResponse.Attachment>(ApiCommand.GetAttachment(key.project, key.chat, key.id), options.timeoutMs)
        } catch (e: RpcException) {
            return AttachmentPreview.Failed(describe(e))
        }
        val bytes = try {
            withContext(options.workContext) { Base64.decode(reply.dataBase64) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: IllegalArgumentException) {
            return AttachmentPreview.Failed("the attachment data is not valid base64")
        }
        put(key.project, key.chat, key.id, bytes)
        return AttachmentPreview.Loaded(bytes)
    }
}
