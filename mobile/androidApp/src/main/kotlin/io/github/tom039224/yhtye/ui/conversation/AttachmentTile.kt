package io.github.tom039224.yhtye.ui.conversation

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.sync.AttachmentPreview
import io.github.tom039224.yhtye.shared.sync.canPreview
import io.github.tom039224.yhtye.shared.sync.formatSize
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// 添付 1 つの正方形のタイル (PC の AttachmentTile): コンポーザでも会話でも同じ大きさ。画像はサムネイル、それ以外は
// 種類のアイコンと拡張子。左上に送り方 (native / path)、コンポーザでは右上に ×。画像はタップで大きく出す。

/** タイルの一辺 (PC の 84px より少し大きく。指で押す)。 */
val ATTACHMENT_TILE_SIZE = 88.dp
private val TILE_SHAPE = RoundedCornerShape(10.dp)
private val BADGE_SHAPE = RoundedCornerShape(6.dp)

/** タイルの裏の画像の大きさ (タイルの 2 倍の画素まで縮めて読む)。 */
private const val THUMB_SCALE = 2

/** 大きく出すときに読む長辺の画素数の上限 (メモリを食いすぎない)。 */
private const val VIEWER_MAX_PX = 2048

/** 拡張子をそのまま種類の印にする長さの上限 (それより長いものは種類の名前)。 */
private const val MAX_EXT = 5

/** 送り方の言葉 (左上の印の読み上げ)。PC の DELIVERY_LABELS と同じ。 */
val DELIVERY_LABELS: Map<AttachmentDelivery, String> = mapOf(
    AttachmentDelivery.Native to "モデルに直接渡す (native)",
    AttachmentDelivery.Path to "ファイルとして保存し、パスで渡す (path)",
)

/** 会話のチャットの添付の中身を取る (画像のサムネイル・拡大表示。キャッシュは同期層の AttachmentPreviews)。 */
fun interface LoadAttachment {
    suspend fun load(meta: AttachmentMeta): AttachmentPreview
}

/** タイルの状態 (コンポーザの添付はアップロード中・失敗もある。会話のものはいつも [Staged])。 */
enum class TileStatus { Uploading, Staged, Failed }

/** ファイルの種類の印: 拡張子 (短ければ)、無ければ種類の名前。 */
fun typeLabel(name: String, kind: AttachmentKind): String {
    val dot = name.lastIndexOf('.')
    val ext = if (dot > 0) name.substring(dot + 1) else ""
    if (ext.isNotEmpty() && ext.length <= MAX_EXT) return ext.uppercase()
    return when (kind) {
        AttachmentKind.Text -> "TEXT"
        AttachmentKind.Image -> "IMAGE"
        else -> "FILE"
    }
}

/** コアが種類を決める前 (アップロード中・失敗) の種類の見当 (選んだ側の言う MIME から)。 */
fun guessKind(mime: String?): AttachmentKind = when {
    mime == null -> AttachmentKind.File
    mime.startsWith("image/") -> AttachmentKind.Image
    mime.startsWith("text/") -> AttachmentKind.Text
    else -> AttachmentKind.File
}

private fun kindIcon(kind: AttachmentKind): ImageVector = when (kind) {
    AttachmentKind.Image -> YIcons.Image
    AttachmentKind.Text -> YIcons.FileText
    else -> YIcons.File
}

/** 画像の読み込みの結果 (タイルの面)。 */
@Immutable
sealed interface TileImage {
    /** 画像ではない・読み込み中・プレビューしない大きさ (種類のアイコンを出す)。 */
    data object None : TileImage

    class Shown(val bitmap: ImageBitmap) : TileImage

    data class Failed(val message: String) : TileImage
}

/** [meta] が画像なら [load] で取り、長辺 [maxPx] まで縮めて読む (取れるまでと画像でないものは [TileImage.None])。 */
@Composable
fun rememberAttachmentImage(meta: AttachmentMeta?, load: LoadAttachment?, maxPx: Int): TileImage {
    val image by produceState<TileImage>(TileImage.None, meta?.id, load, maxPx) {
        if (meta == null || load == null || !meta.canPreview()) return@produceState
        value = when (val got = load.load(meta)) {
            is AttachmentPreview.Loaded ->
                withContext(Dispatchers.Default) { decodeImage(got.bytes, maxPx) }?.let { TileImage.Shown(it) }
                    ?: TileImage.Failed("the image could not be decoded")
            AttachmentPreview.TooLarge -> TileImage.None
            is AttachmentPreview.Failed -> TileImage.Failed(got.message)
        }
    }
    return image
}

/** 画像を長辺 [maxPx] 程度まで (2 の冪で) 縮めて読む。画像でなければ null。 */
fun decodeImage(bytes: ByteArray, maxPx: Int): ImageBitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxPx) sample *= 2
    val options = BitmapFactory.Options().apply { inSampleSize = sample }
    return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.asImageBitmap()
}

/** タイルの裏で読む画像の長辺の画素数。 */
@Composable
fun thumbnailPx(): Int = with(LocalDensity.current) { ATTACHMENT_TILE_SIZE.roundToPx() * THUMB_SCALE }

/**
 * 添付 1 つの正方形のタイル。
 *
 * @param delivery 左上の送り方 (まだ分からなければ null)。[predicted] はコンポーザの予測 (送るときにコアが決める)。
 * @param error 失敗の理由 ([TileStatus.Failed])。
 * @param onRemove 右上の × (コンポーザのタイル)。
 * @param onOpen 画像を大きく出す (面がボタンになる)。
 */
@Composable
fun AttachmentTile(
    name: String,
    kind: AttachmentKind,
    image: TileImage,
    delivery: AttachmentDelivery?,
    modifier: Modifier = Modifier,
    predicted: Boolean = false,
    status: TileStatus = TileStatus.Staged,
    error: String? = null,
    size: Long? = null,
    onRemove: (() -> Unit)? = null,
    onOpen: (() -> Unit)? = null,
) {
    val c = Yhtye.colors
    val details = listOfNotNull(
        name,
        size?.let(::formatSize),
        when (status) {
            TileStatus.Uploading -> "添付中"
            TileStatus.Failed -> "添付できませんでした: ${error.orEmpty()}"
            TileStatus.Staged -> null
        },
        (image as? TileImage.Failed)?.let { "プレビューを読めませんでした: ${it.message}" },
    ).joinToString(" · ")
    Box(
        modifier
            .size(ATTACHMENT_TILE_SIZE)
            .clip(TILE_SHAPE)
            .background(c.bgCard)
            .border(1.dp, if (status == TileStatus.Failed) c.handCardLine else c.lineCard, TILE_SHAPE)
            .testTag("attachment-tile"),
    ) {
        val face = Modifier
            .fillMaxSize()
            .then(if (onOpen != null) Modifier.clickable(role = Role.Image, onClickLabel = "$name を拡大表示", onClick = onOpen) else Modifier)
            .semantics { contentDescription = details }
        if (image is TileImage.Shown) {
            Image(image.bitmap, contentDescription = null, contentScale = ContentScale.Crop, modifier = face.testTag("attachment-thumb"))
        } else {
            FileFace(name, kind, face)
        }
        when (status) {
            TileStatus.Uploading -> Overlay(Color.Transparent) { YIcon(YIcons.Loader, c.tx2, 20.dp, spin = true) }
            TileStatus.Failed -> Overlay(c.handCardBg.copy(alpha = 0.85f)) {
                YIcon(YIcons.Alert, c.handBar, 18.dp)
                if (!error.isNullOrEmpty()) {
                    Text(
                        error,
                        color = c.handFg,
                        fontSize = 9.sp,
                        lineHeight = 11.sp,
                        textAlign = TextAlign.Center,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                }
            }
            TileStatus.Staged -> Unit
        }
        if (delivery != null && delivery != AttachmentDelivery.Unrecognized && status == TileStatus.Staged) {
            DeliveryBadge(delivery, predicted, Modifier.align(Alignment.TopStart).padding(4.dp))
        }
        if (onRemove != null) {
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .size(32.dp)
                    .clickable(role = Role.Button, onClick = onRemove)
                    .semantics { contentDescription = "$name を外す" }
                    .testTag("attachment-remove"),
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(22.dp).background(c.bgCenter.copy(alpha = 0.8f), CircleShape), contentAlignment = Alignment.Center) {
                    YIcon(YIcons.X, c.tx, 12.dp)
                }
            }
        }
    }
}

/** 画像でない (まだ読めていない) ときの面: 種類のアイコン、拡張子、名前。 */
@Composable
private fun FileFace(name: String, kind: AttachmentKind, modifier: Modifier) {
    val c = Yhtye.colors
    Column(
        modifier.padding(start = 6.dp, end = 6.dp, top = 12.dp, bottom = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
    ) {
        YIcon(kindIcon(kind), c.tx3, 26.dp)
        Text(typeLabel(name, kind), color = c.tx2, fontFamily = MonoFont, fontSize = 10.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        Text(
            name,
            color = c.tx4,
            fontFamily = MonoFont,
            fontSize = 9.5.sp,
            lineHeight = 12.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.fillMaxWidth(),
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun Overlay(tint: Color, content: @Composable () -> Unit) {
    val c = Yhtye.colors
    Column(
        Modifier.fillMaxSize().background(c.bgCenter.copy(alpha = 0.55f)).background(tint),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
    ) { content() }
}

/** 左上の送り方の印 (native: モデルが直接受け取る、path: 保存したファイルをエージェントが読む)。 */
@Composable
private fun DeliveryBadge(delivery: AttachmentDelivery, predicted: Boolean, modifier: Modifier) {
    val c = Yhtye.colors
    val base = DELIVERY_LABELS.getValue(delivery)
    val label = if (predicted) "$base · 送信時に確定" else base
    Box(
        modifier
            .size(20.dp)
            .background(c.bgCenter.copy(alpha = 0.8f), BADGE_SHAPE)
            .semantics { contentDescription = label }
            .testTag("attachment-delivery-${delivery.wire}"),
        contentAlignment = Alignment.Center,
    ) {
        YIcon(if (delivery == AttachmentDelivery.Native) YIcons.Bot else YIcons.Folder, if (delivery == AttachmentDelivery.Native) c.accent else c.tx2, 12.dp)
    }
}

/** 画像を画面いっぱいに出す (地か × のタップ・戻るで閉じる)。 */
@Composable
fun AttachmentViewer(meta: AttachmentMeta, load: LoadAttachment, onDismiss: () -> Unit) {
    val c = Yhtye.colors
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.92f))
                .clickable(onClickLabel = "閉じる", onClick = onDismiss)
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .testTag("attachment-viewer"),
        ) {
            Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                    when (val image = rememberAttachmentImage(meta, load, VIEWER_MAX_PX)) {
                        is TileImage.Shown ->
                            Image(image.bitmap, contentDescription = meta.name, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
                        is TileImage.Failed -> Text("プレビューを読めませんでした: ${image.message}", color = c.handFg, fontSize = 13.sp)
                        TileImage.None -> YIcon(YIcons.Loader, Color.White, 24.dp, spin = true)
                    }
                }
                Text(meta.name, color = Color.White.copy(alpha = 0.8f), fontFamily = MonoFont, fontSize = 11.sp, modifier = Modifier.padding(top = 8.dp))
            }
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .padding(8.dp)
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(c.bgChip)
                    .clickable(role = Role.Button, onClick = onDismiss)
                    .semantics { contentDescription = "閉じる" }
                    .testTag("attachment-viewer-close"),
                contentAlignment = Alignment.Center,
            ) { YIcon(YIcons.X, c.tx, 16.dp) }
        }
    }
}
