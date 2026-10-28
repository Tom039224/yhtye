package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.client.RpcException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.coroutines.CoroutineContext
import kotlin.io.encoding.Base64

/** 添付にするファイルの中身の読み方。[AttachmentComposer] が (ほかのアップロードの後で) 1 度に 1 つずつ呼ぶ。 */
fun interface AttachmentSource {
    /** 中身を全部読む。大きすぎると分かったら [AttachmentTooLargeException]。 */
    suspend fun read(): ByteArray
}

/**
 * 入力中の添付 (チャットごと): コアへのステージ (`stage_attachment`)・破棄 (`discard_attachment`) と、画面が購読する状態 ([state])。
 * 送るのは [AppStore.sendMessage] (ステージ済みの ID を付けて `send_user_message` を送り、受け付けられたら [markSent])。
 *
 * - ステージはファイルを 1 つずつ順に読んで送る: 読んだ中身・base64・JSON・フレームで同時にメモリを使うので、
 *   何枚も並行にはしない。順番待ちの間は [PendingAttachment.Upload.Uploading]。
 * - 失敗は [PendingAttachment.Upload.Failed] として残る (直して送るか、[remove] で外すまで送れない。[canSendMessage])。
 * - 状態は残らない (プロセスが落ちたら入力中の添付は消える。コアに残ったステージ済みのファイルはチャットと一緒に消える)。
 *
 * 状態の変更は [scope] (並列度 1。[AppStore] のもの) の上で 1 本ずつ行うので、どのスレッドから呼んでもよい。
 */
class AttachmentComposer(
    private val core: CoreConnection,
    private val scope: CoroutineScope,
    private val previews: AttachmentPreviews,
    private val options: Options = Options(),
) {
    data class Options(
        /** 1 つのファイルの大きさの上限 (バイト)。 */
        val maxBytes: Long = MAX_ATTACHMENT_BYTES,
        /** `stage_attachment` の応答をこれ以上待たない (ファイルは 1 つのコマンドで丸ごと送る)。 */
        val stageTimeoutMs: Long = 180_000,
        /** base64 への変換 (大きなデータの処理) の実行先。テストは呼び出し元のまま ([EmptyCoroutineContext])。 */
        val workContext: CoroutineContext = Dispatchers.Default,
    )

    private val _state = MutableStateFlow<AttachmentDrafts>(emptyMap())

    /** チャットごとの入力中の添付 (追加した順)。 */
    val state: StateFlow<AttachmentDrafts> = _state.asStateFlow()

    // ここから下は [scope] でだけ触る
    private var nextLocalId = 1L
    private val sources = HashMap<Long, AttachmentSource>()
    private val uploads = Mutex()

    fun pending(project: String, chat: String): List<PendingAttachment> = _state.value.of(project, chat)

    /** [bytes] を [chat] の添付に足す (すぐ [state] に出て、アップロードが続く)。 */
    fun stage(project: String, chat: String, name: String, mime: String?, bytes: ByteArray) {
        stage(project, chat, name, mime, bytes.size.toLong()) { bytes }
    }

    /**
     * [source] から読むファイルを [chat] の添付に足す。[size] が分かっていて上限を超えるときは読まずに失敗として出す。
     * [mime] は選んだ側の言う種類 (無ければ null)。コアが中身で直す。
     */
    fun stage(project: String, chat: String, name: String, mime: String?, size: Long?, source: AttachmentSource) {
        scope.launch {
            val ref = ChatRef(project, chat)
            val localId = nextLocalId++
            if (size != null && size > options.maxBytes) {
                val failed = PendingAttachment.Upload.Failed(tooLargeMessage(size, options.maxBytes), retryable = false)
                edit(ref) { it + PendingAttachment(localId, name, mime, size, failed) }
                return@launch
            }
            sources[localId] = source
            edit(ref) { it + PendingAttachment(localId, name, mime, size, PendingAttachment.Upload.Uploading) }
            upload(ref, localId)
        }
    }

    /** 添付を外す (ステージ済みならコアの保存も消す。アップロード中なら、できたものをコアに残さない)。 */
    fun remove(project: String, chat: String, localId: Long) {
        scope.launch {
            val ref = ChatRef(project, chat)
            val entry = find(ref, localId) ?: return@launch
            sources.remove(localId)
            edit(ref) { list -> list.filter { it.localId != localId } }
            val staged = entry.meta ?: return@launch
            discardOnCore(ref, staged.id)
        }
    }

    /** 失敗した添付 ([PendingAttachment.Upload.Failed.retryable]) をもう一度読んで送る。 */
    fun retry(project: String, chat: String, localId: Long) {
        scope.launch {
            val ref = ChatRef(project, chat)
            val failed = find(ref, localId)?.upload as? PendingAttachment.Upload.Failed ?: return@launch
            if (!failed.retryable || localId !in sources) return@launch
            replace(ref, localId) { it.copy(upload = PendingAttachment.Upload.Uploading) }
            upload(ref, localId)
        }
    }

    /** 送れた添付を入力中から外す (コアでは送信済みになった。[scope] の上で呼ぶ)。 */
    internal fun markSent(project: String, chat: String, ids: Collection<String>) {
        edit(ChatRef(project, chat)) { list -> list.filter { entry -> entry.meta?.id?.let { it in ids } != true } }
    }

    /** 消えたチャットの入力中の添付を捨てる (コアのファイルはチャットと一緒に消える。[scope] の上で呼ぶ)。 */
    internal fun forgetChat(project: String, chat: String) {
        val ref = ChatRef(project, chat)
        _state.value[ref]?.forEach { sources.remove(it.localId) }
        _state.update { it - ref }
    }

    private suspend fun upload(ref: ChatRef, localId: Long) {
        uploads.withLock {
            val source = sources[localId] ?: return
            if (find(ref, localId) == null) return
            val bytes = try {
                source.read()
            } catch (e: CancellationException) {
                throw e
            } catch (e: AttachmentTooLargeException) {
                fail(ref, localId, e.message.orEmpty(), retryable = false)
                return
            } catch (e: Exception) {
                fail(ref, localId, "could not read the file: ${e.message ?: e::class.simpleName}", retryable = true)
                return
            }
            if (bytes.size > options.maxBytes) {
                fail(ref, localId, tooLargeMessage(bytes.size.toLong(), options.maxBytes), retryable = false)
                return
            }
            // 読んでいる間に外された
            val entry = find(ref, localId) ?: return
            val staged = try {
                val data = withContext(options.workContext) { Base64.encode(bytes) }
                val command = ApiCommand.StageAttachment(ref.project, ref.chat, entry.name, entry.mime, data)
                core.expect<ApiResponse.AttachmentStaged>(command, options.stageTimeoutMs).attachment
            } catch (e: RpcException) {
                // コアが断った (チャットが無い・base64 が不正) ものはやり直しても同じ。つながらない・時間切れはやり直せる
                fail(ref, localId, describe(e), retryable = e !is ApiException)
                return
            }
            settle(ref, localId, staged, bytes)
        }
    }

    private fun fail(ref: ChatRef, localId: Long, message: String, retryable: Boolean) {
        // やり直せないものは中身の読み方 (読んだバイト列を持つかもしれない) を手放す
        if (!retryable) sources.remove(localId)
        replace(ref, localId) { it.copy(upload = PendingAttachment.Upload.Failed(message, retryable)) }
    }

    private fun settle(ref: ChatRef, localId: Long, staged: AttachmentMeta, bytes: ByteArray) {
        sources.remove(localId)
        if (find(ref, localId) == null) {
            // 送っている間に外された: コアに残さない
            scope.launch { discardOnCore(ref, staged.id) }
            return
        }
        replace(ref, localId) {
            it.copy(name = staged.name, mime = staged.mime, size = staged.size, upload = PendingAttachment.Upload.Staged(staged))
        }
        // 送った画像はそのままプレビューになる (会話に出るとき取り直さない)
        if (staged.kind == AttachmentKind.Image) previews.put(ref.project, ref.chat, staged.id, bytes)
    }

    /** ステージ済みの添付をコアから消す。失敗しても取り残されるだけ (チャットを消すとき一緒に消える) なので黙っている。 */
    private suspend fun discardOnCore(ref: ChatRef, id: String) {
        try {
            core.expect<ApiResponse.Accepted>(ApiCommand.DiscardAttachment(ref.project, ref.chat, id))
        } catch (e: RpcException) {
            // 上の通り
        }
    }

    private fun find(ref: ChatRef, localId: Long): PendingAttachment? = pending(ref.project, ref.chat).find { it.localId == localId }

    private fun replace(ref: ChatRef, localId: Long, f: (PendingAttachment) -> PendingAttachment) {
        edit(ref) { list -> list.map { if (it.localId == localId) f(it) else it } }
    }

    /** チャットの入力中の添付を作り替える (空になったチャットは持たない)。 */
    private fun edit(ref: ChatRef, f: (List<PendingAttachment>) -> List<PendingAttachment>) {
        _state.update { drafts ->
            val next = f(drafts[ref].orEmpty())
            if (next.isEmpty()) drafts - ref else drafts + (ref to next)
        }
    }
}
