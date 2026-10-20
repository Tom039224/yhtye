package io.github.tom039224.yhtye.ui.conversation

import android.content.ClipData
import android.content.ContentResolver
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.sync.PendingAttachment
import io.github.tom039224.yhtye.shared.sync.meta
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons

// コンポーザの添付 (PC の Composer の添付の部分): 左の + (写真はフォトピッカー、ファイルはファイルを選ぶ画面 (SAF)。
// どちらも複数選べる)、入力欄の上に横に並ぶタイル。会話のユーザーの発言の下のタイルもここ ([SentAttachments])。

/** 写真を選ぶ画面に出すもの (動画は大きすぎて送れないので出さない)。 */
private val PHOTOS_ONLY = PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)

/** ファイルを選ぶ画面で選べる種類 (何でも)。 */
private val ANY_FILE = arrayOf("*/*")

/**
 * 入力欄の左の +。押すと「写真」「ファイル」を選ぶメニュー。選んだもの (Uri、選んだ順) を [onAddFiles] に渡す。
 */
@Composable
fun AttachButton(enabled: Boolean, onAddFiles: (List<Uri>) -> Unit) {
    val c = Yhtye.colors
    var open by remember { mutableStateOf(false) }
    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia()) { uris ->
        if (uris.isNotEmpty()) onAddFiles(uris)
    }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) onAddFiles(uris)
    }
    Box {
        Box(
            Modifier
                .size(40.dp)
                .clip(CircleShape)
                .clickable(enabled = enabled, role = Role.Button, onClickLabel = "ファイルを添付") { open = true }
                .semantics { contentDescription = "ファイルを添付" }
                .testTag("composer-attach"),
            contentAlignment = Alignment.Center,
        ) { YIcon(YIcons.Plus, if (enabled) c.tx2 else c.tx5, 22.dp) }
        DropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("写真") },
                leadingIcon = { YIcon(YIcons.Image, c.tx2, 18.dp) },
                onClick = {
                    open = false
                    photos.launch(PHOTOS_ONLY)
                },
                modifier = Modifier.testTag("attach-photos"),
            )
            DropdownMenuItem(
                text = { Text("ファイル") },
                leadingIcon = { YIcon(YIcons.File, c.tx2, 18.dp) },
                onClick = {
                    open = false
                    files.launch(ANY_FILE)
                },
                modifier = Modifier.testTag("attach-files"),
            )
        }
    }
}

/**
 * 貼り付け・キーボードから入った項目のうち、添付にするファイル (`content:` の Uri で、文字を持たないもの)。
 * 文字も持つ項目 (表のセルのコピーなど) は文字として貼る (PC と同じ)。
 */
fun isPastedFile(item: ClipData.Item): Boolean =
    item.uri?.scheme == ContentResolver.SCHEME_CONTENT && item.text.isNullOrEmpty()

/** 入力欄の上の、入力中の添付のタイル (横にスクロール)。 */
@Composable
fun PendingAttachments(
    attachments: List<PendingAttachment>,
    load: LoadAttachment?,
    onRemove: ((Long) -> Unit)?,
    modifier: Modifier = Modifier,
) {
    var viewing by rememberSaveable { mutableStateOf<String?>(null) }
    val thumbPx = thumbnailPx()
    Row(
        modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).testTag("composer-attachments"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        attachments.forEach { a -> key(a.localId) {
            val meta = a.meta
            val image = rememberAttachmentImage(meta, load, thumbPx)
            AttachmentTile(
                name = a.name,
                kind = meta?.kind ?: guessKind(a.mime),
                image = image,
                delivery = meta?.delivery,
                predicted = true,
                status = when (a.upload) {
                    PendingAttachment.Upload.Uploading -> TileStatus.Uploading
                    is PendingAttachment.Upload.Staged -> TileStatus.Staged
                    is PendingAttachment.Upload.Failed -> TileStatus.Failed
                },
                error = (a.upload as? PendingAttachment.Upload.Failed)?.message,
                size = meta?.size ?: a.size,
                onRemove = onRemove?.let { remove -> { remove(a.localId) } },
                onOpen = if (image is TileImage.Shown && meta != null) ({ viewing = meta.id }) else null,
            )
        } }
    }
    val open = viewing?.let { id -> attachments.firstNotNullOfOrNull { a -> a.meta?.takeIf { it.id == id } } }
    if (open != null && load != null) AttachmentViewer(open, load, onDismiss = { viewing = null })
}

/** 会話のユーザーの発言の添付 (本文の下に右寄せ。× は無く、左上は最終の送り方)。画像はタップで大きく出す。 */
@Composable
fun SentAttachments(attachments: List<AttachmentMeta>, load: LoadAttachment?, modifier: Modifier = Modifier) {
    var viewing by rememberSaveable { mutableStateOf<String?>(null) }
    val thumbPx = thumbnailPx()
    FlowRow(
        modifier.testTag("sent-attachments"),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        attachments.forEach { meta -> key(meta.id) {
            val image = rememberAttachmentImage(meta, load, thumbPx)
            AttachmentTile(
                name = meta.name,
                kind = meta.kind,
                image = image,
                delivery = meta.delivery,
                size = meta.size,
                onOpen = if (image is TileImage.Shown) ({ viewing = meta.id }) else null,
            )
        } }
    }
    val open = viewing?.let { id -> attachments.find { it.id == id } }
    if (open != null && load != null) AttachmentViewer(open, load, onDismiss = { viewing = null })
}
