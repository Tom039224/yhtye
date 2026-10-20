package io.github.tom039224.yhtye.ui.common

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

// 線だけのアイコン (PC の Icon・モックの ICONS と同じ lucide の形、線幅 1.75)。色は Icon の tint で付ける。

private const val VIEWPORT = 24f
private const val STROKE = 1.75f

/** 円 (cx, cy, r) を path にする。 */
private fun circle(cx: Float, cy: Float, r: Float): String =
    "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0"

/** 角の丸い四角 (x, y, w, h, rx) を path にする。 */
private fun rect(x: Float, y: Float, w: Float, h: Float, r: Float): String =
    "M${x + r} ${y}h${w - 2 * r}a$r $r 0 0 1 $r ${r}v${h - 2 * r}a$r $r 0 0 1 ${-r} ${r}h${-(w - 2 * r)}" +
        "a$r $r 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a$r $r 0 0 1 $r ${-r}z"

private fun lineIcon(name: String, vararg paths: String): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, VIEWPORT, VIEWPORT).apply {
        paths.forEach { d ->
            addPath(
                pathData = addPathNodes(d),
                stroke = SolidColor(Color.Black),
                strokeLineWidth = STROKE,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()

private fun filledIcon(name: String, vararg paths: String): ImageVector =
    ImageVector.Builder(name, 24.dp, 24.dp, VIEWPORT, VIEWPORT).apply {
        paths.forEach { d -> addPath(pathData = addPathNodes(d), fill = SolidColor(Color.Black)) }
    }.build()

object YIcons {
    val Menu = lineIcon("menu", "M4 6h16", "M4 12h16", "M4 18h16")
    val List = lineIcon("list", "M8 6h13", "M8 12h13", "M8 18h13", "M3 6h.01", "M3 12h.01", "M3 18h.01")
    val Layers = lineIcon("layers", "M12 3 21 8 12 13 3 8Z", "M3 12.5 12 17.5 21 12.5", "M3 17 12 22 21 17")
    val Send = lineIcon("send", "M12 19V5", "M5 12l7-7 7 7")
    val ChevronRight = lineIcon("chevR", "M9 18l6-6-6-6")
    val ChevronDown = lineIcon("chevD", "M6 9l6 6 6-6")
    val ChevronsUp = lineIcon("chevronsUp", "m17 11-5-5-5 5", "m17 18-5-5-5 5")
    val Plus = lineIcon("plus", "M12 5v14", "M5 12h14")
    val MessagePlus = lineIcon(
        "msgPlus",
        "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
        "M12 7v6",
        "M9 10h6",
    )
    val Message = lineIcon("message", "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z")
    val Branch = lineIcon("branch", "M6 3v12", "M18 9a9 9 0 0 1-9 9", circle(18f, 6f, 3f), circle(6f, 18f, 3f))
    val Merge = lineIcon("merge", circle(18f, 18f, 3f), circle(6f, 6f, 3f), "M6 21V9a9 9 0 0 0 9 9")
    val Check = lineIcon("check", "M20 6 9 17l-5-5")
    val X = lineIcon("x", "M18 6 6 18", "M6 6l12 12")
    val Loader = lineIcon("loader", "M21 12a9 9 0 1 1-6.2-8.56")
    val Wrench = lineIcon(
        "wrench",
        "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3" +
            "l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z",
    )
    val Bolt = lineIcon("bolt", "M13 2 3 14h9l-1 8 10-12h-9l1-8z")
    val Alert = lineIcon(
        "alert",
        "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z",
        "M12 9v4",
        "M12 17h.01",
    )
    val Hourglass = lineIcon(
        "hourglass",
        "M5 22h14",
        "M5 2h14",
        "M17 22v-4.17a2 2 0 0 0-.59-1.42L12 12l-4.41 4.41A2 2 0 0 0 7 17.83V22",
        "M7 2v4.17a2 2 0 0 0 .59 1.42L12 12l4.41-4.41A2 2 0 0 0 17 6.17V2",
    )
    val Code = lineIcon("code", "M16 18l6-6-6-6", "M8 6l-6 6 6 6")
    val Eye = lineIcon("eye", "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z", circle(12f, 12f, 3f))
    val Search = lineIcon("search", circle(11f, 11f, 7f), "M21 21l-4.3-4.3")
    val Ban = lineIcon("ban", circle(12f, 12f, 9f), "M5.6 5.6l12.8 12.8")
    val At = lineIcon("at", circle(12f, 12f, 4f), "M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8")
    val Thought = lineIcon("thought", "M9 18h6", "M10 22h4", "M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z")
    val Refresh = lineIcon("refresh", "M21 12a9 9 0 1 1-3-6.7L21 8", "M21 3v5h-5")
    val Download = lineIcon("download", "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "M7 10l5 5 5-5", "M12 15V3")
    val Bot = lineIcon("bot", rect(4f, 8f, 16f, 12f, 2f), "M12 8V4H8", "M2 14h2", "M20 14h2", "M15 13v2", "M9 13v2")
    val User = lineIcon("user", circle(12f, 8f, 4f), "M20 21a8 8 0 0 0-16 0")
    val Bell = lineIcon("bell", "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9", "M10.3 21a1.94 1.94 0 0 0 3.4 0")
    val Folder = lineIcon(
        "folder",
        "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
    )
    val Clock = lineIcon("clock", circle(12f, 12f, 10f), "M12 6v6l4 2")
    val Trash = lineIcon("trash", "M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2")
    val Info = lineIcon("info", circle(12f, 12f, 10f), "M12 16v-4", "M12 8h.01")
    val Edit = lineIcon("edit", "M12 20h9", "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z")
    val Minimize = lineIcon("minimize", "M4 14h6v6", "M20 10h-6V4", "M14 10l7-7", "M3 21l7-7")
    val More = lineIcon("more", circle(12f, 5f, 1f), circle(12f, 12f, 1f), circle(12f, 19f, 1f))
    val FileText = lineIcon(
        "fileText",
        "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z",
        "M14 2v6h6",
        "M16 13H8",
        "M16 17H8",
    )
    val File = lineIcon("file", "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z", "M14 2v6h6")
    val Image = lineIcon("image", rect(3f, 3f, 18f, 18f, 2f), circle(9f, 9f, 2f), "m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21")
    val Unplug = lineIcon(
        "unplug",
        "m19 5 3-3",
        "m2 22 3-3",
        "M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4Z",
        "M7.5 13.5 10 11",
        "M10.5 16.5 13 14",
        "m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0Z",
    )

    val Back = lineIcon("back", "M19 12H5", "M12 19l-7-7 7-7")
    val ArrowLeft = lineIcon("arrowL", "M19 12H5", "M11 18l-6-6 6-6")
    val ArrowRight = lineIcon("arrowR", "M5 12h14", "m12 5 7 7-7 7")
    val History = lineIcon("history", "M3 12a9 9 0 1 0 3-6.7L3 8", "M3 3v5h5", "M12 7v5l4 2")
    val Sliders = lineIcon(
        "sliders",
        "M4 21v-7",
        "M4 10V3",
        "M12 21v-9",
        "M12 8V3",
        "M20 21v-5",
        "M20 12V3",
        "M1 14h6",
        "M9 8h6",
        "M17 16h6",
    )
    val Globe = lineIcon(
        "globe",
        circle(12f, 12f, 8.5f),
        "M3.5 12h17",
        "M12 3.5c2.4 2.3 3.6 5.1 3.6 8.5s-1.2 6.2-3.6 8.5c-2.4-2.3-3.6-5.1-3.6-8.5S9.6 5.8 12 3.5",
    )
    val Terminal = lineIcon("terminal", "M4.5 6.5h15v11h-15z", "M8 10.5l2.5 2L8 14.5", "M12.5 15h3.5")
    val Key = lineIcon("key", circle(8f, 15.5f, 3.5f), "M10.5 13L19 4.5", "M16.5 7l2.5 2.5", "M13.5 10l2 2")
    val Star = lineIcon("star", "M12 4l2.4 5 5.4.7-4 3.8 1 5.4L12 16.2l-4.8 2.7 1-5.4-4-3.8 5.4-.7z")
    val Phone = lineIcon("phone", rect(7f, 3.5f, 10f, 17f, 2f), "M11 17.5h2")
    val Server = lineIcon(
        "server",
        rect(4.5f, 4.5f, 15f, 6.5f, 1.5f),
        rect(4.5f, 13f, 15f, 6.5f, 1.5f),
        "M8 7.75h.01M8 16.25h.01",
        "M12 7.75h4M12 16.25h4",
    )
    val Activity = lineIcon("activity", "M22 12h-4l-3 9L9 3l-3 9H2")
    val Flag = lineIcon("flag", "M4 22V4", "M4 4h13l-2 4 2 4H4")
    val Help = lineIcon("help", circle(12f, 12f, 10f), "M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3", "M12 17h.01")

    /** ターンの中止・タスクの停止 (塗りの角丸の四角)。 */
    val Stop = filledIcon("stop", rect(5f, 5f, 14f, 14f, 2.5f))
}
