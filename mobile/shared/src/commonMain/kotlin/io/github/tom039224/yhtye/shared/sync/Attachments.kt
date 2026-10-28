package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta

// チャットの添付ファイル (core-design.md §20) のうち、画面に依らない部分: 入力中の添付の状態、送れるかの判定、
// 大きさの上限と表示。ステージ・破棄は [AttachmentComposer]、プレビューは [AttachmentPreviews]。

/**
 * 1 つのファイルとして送る大きさの上限 (バイト)。ファイルは base64 にして 1 つのコマンドで丸ごと送るが、Android の OkHttp は
 * 送信待ちのキューが 16 MiB までで、超える `send` は失敗する。base64 は 4/3 倍 + コマンドの枠なので、それに収まる 10 MiB にする
 * (コアは大きさに上限を持たない。上限を超えたものはステージせず、失敗として出す)。
 */
const val MAX_ATTACHMENT_BYTES: Long = 10L * 1024 * 1024

/** プレビュー (画像の取得・メモリへのキャッシュ) をする添付の大きさの上限 (バイト)。 */
const val MAX_PREVIEW_BYTES: Long = 10L * 1024 * 1024

/** 入力中の添付を置くチャット (添付はチャットごとのコンポーザに属する)。 */
data class ChatRef(val project: String, val chat: String)

/** チャットごとの入力中の添付 ([AttachmentComposer.state] の値)。 */
typealias AttachmentDrafts = Map<ChatRef, List<PendingAttachment>>

fun AttachmentDrafts.of(project: String, chat: String): List<PendingAttachment> = this[ChatRef(project, chat)].orEmpty()

/** 入力中 (まだ送っていない) の添付 1 件。 */
data class PendingAttachment(
    /** この端末だけで使う ID (コアが付ける [AttachmentMeta.id] はステージできてから。[Upload.Staged])。 */
    val localId: Long,
    /** 選んだファイルの名前 (ステージできたら、コアが安全にした名前が [Upload.Staged] の `meta` にある)。 */
    val name: String,
    /** 選んだ側が言う MIME (無ければ null。種類はコアが中身で決める)。 */
    val mime: String?,
    /** 分かっていればバイト数。 */
    val size: Long?,
    val upload: Upload,
) {
    sealed interface Upload {
        /** 読み込み・送信中 (他の添付のアップロードの順番待ちも含む)。 */
        data object Uploading : Upload

        /** コアに保存できた。`meta.delivery` は送り方の予測 (最終判定は送るとき)。 */
        data class Staged(val meta: AttachmentMeta) : Upload

        /** 失敗した。[retryable] なら [AttachmentComposer.retry] でやり直せる (大きすぎる・コアが断ったものは同じ結果になる)。 */
        data class Failed(val message: String, val retryable: Boolean) : Upload
    }
}

/** ステージできたときのコアのメタデータ。 */
val PendingAttachment.meta: AttachmentMeta? get() = (upload as? PendingAttachment.Upload.Staged)?.meta

/** 送るステージ済みの添付の ID (並べた順)。 */
fun List<PendingAttachment>.stagedIds(): List<String> = mapNotNull { it.meta?.id }

/** どれもステージできている (アップロード中も失敗も無い)。 */
fun List<PendingAttachment>.isSettled(): Boolean = all { it.upload is PendingAttachment.Upload.Staged }

/**
 * 送れるか: 添付がすべてステージできていて、本文が空でないか添付が 1 つ以上ある (コアは本文が空でも添付があれば受ける)。
 * アップロード中は待ち、失敗したものは直すか外すまで送らない (失敗した添付だけ抜いて送ってしまわないため)。
 */
fun canSendMessage(text: String, pending: List<PendingAttachment>): Boolean =
    pending.isSettled() && (text.isNotBlank() || pending.isNotEmpty())

/** プレビューできる添付か (画像で、取得してメモリに置いてよい大きさ)。 */
fun AttachmentMeta.canPreview(maxBytes: Long = MAX_PREVIEW_BYTES): Boolean = kind == AttachmentKind.Image && size <= maxBytes

/** ファイルを読んだ・読んでいる間に、大きさの上限を超えると分かった (読み出す側が投げる)。 */
class AttachmentTooLargeException(message: String) : Exception(message)

fun tooLargeMessage(size: Long, limit: Long): String = "the file is too large (${formatSize(size)}; the limit is ${formatSize(limit)})"

private val SIZE_UNITS = listOf("KB", "MB", "GB")

/** バイト数を `512 B` / `1.5 KB` / `10.0 MB` のように (1024 進、小数 1 桁、切り捨て)。 */
fun formatSize(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    var tenths = bytes * 10 / 1024
    var unit = 0
    while (tenths >= 10 * 1024 && unit < SIZE_UNITS.lastIndex) {
        tenths /= 1024
        unit++
    }
    return "${tenths / 10}.${tenths % 10} ${SIZE_UNITS[unit]}"
}
