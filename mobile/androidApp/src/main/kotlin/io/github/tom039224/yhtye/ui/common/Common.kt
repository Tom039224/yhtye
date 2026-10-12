package io.github.tom039224.yhtye.ui.common

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.Ring

/** 1 回転の時間 (PC の `yspin` .9s)。 */
private const val SPIN_MS = 900

/** 回り続ける角度 (読み込み中のアイコン・動作中のリング)。 */
@Composable
fun spinAngle(): Float {
    val transition = rememberInfiniteTransition(label = "spin")
    val angle by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(SPIN_MS, easing = LinearEasing), RepeatMode.Restart),
        label = "spin",
    )
    return angle
}

/** 呼吸するように明滅する不透明度 (PC の `ybreathe`)。 */
@Composable
fun breatheAlpha(periodMs: Int = 1_400, min: Float = 0.55f): Float {
    val transition = rememberInfiniteTransition(label = "breathe")
    val alpha by transition.animateFloat(
        initialValue = min,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(periodMs / 2), RepeatMode.Reverse),
        label = "breathe",
    )
    return alpha
}

/** 線のアイコン (装飾。意味は周りの文字が持つ)。[spin] で回る (読み込み中)。 */
@Composable
fun YIcon(icon: ImageVector, tint: Color, size: Dp, modifier: Modifier = Modifier, spin: Boolean = false) {
    val m = if (spin) modifier.rotate(spinAngle()) else modifier
    Icon(icon, contentDescription = null, tint = tint, modifier = m.size(size))
}

/**
 * プロジェクト・チャットのリング (PC の `.ring`、T-49): 回る緑 = 動作中、中を抜いた緑 = 待機、グレーの輪郭 = 停止。
 */
@Composable
fun RingMark(ring: Ring, modifier: Modifier = Modifier, size: Dp = 12.dp) {
    val c = Yhtye.colors
    val label = RING_LABEL.getValue(ring)
    val base = modifier.size(size).semantics { contentDescription = label }.testTag("ring-${ring.name}")
    when (ring) {
        Ring.Running -> {
            val angle = spinAngle()
            Canvas(base.rotate(angle)) {
                val w = 2.dp.toPx()
                val inset = w / 2
                val arcSize = androidx.compose.ui.geometry.Size(this.size.width - w, this.size.height - w)
                val topLeft = androidx.compose.ui.geometry.Offset(inset, inset)
                drawArc(c.ringTrack, 0f, 360f, false, topLeft, arcSize, style = Stroke(w))
                drawArc(c.ringHead, -135f, 90f, false, topLeft, arcSize, style = Stroke(w))
            }
        }
        Ring.Ready, Ring.Stopped -> Canvas(base) {
            val w = 2.dp.toPx()
            drawCircle(
                color = if (ring == Ring.Ready) c.doneBar else c.ringStop,
                radius = (this.size.minDimension - w) / 2,
                style = Stroke(w),
            )
        }
    }
}

/** 静かな空の状態: 薄いアイコンと短い 1 行 (PC の EmptyState)。 */
@Composable
fun EmptyState(icon: ImageVector, text: String, modifier: Modifier = Modifier, error: Boolean = false, testTag: String? = null) {
    val c = Yhtye.colors
    val tagged = if (testTag != null) modifier.testTag(testTag) else modifier
    Column(
        tagged.fillMaxWidth().padding(horizontal = 24.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        YIcon(icon, if (error) c.handBar else c.tx5, size = 26.dp)
        Text(
            text,
            color = if (error) c.handFg else c.tx3,
            fontSize = 14.sp,
            lineHeight = 24.sp,
            textAlign = TextAlign.Center,
        )
    }
}
