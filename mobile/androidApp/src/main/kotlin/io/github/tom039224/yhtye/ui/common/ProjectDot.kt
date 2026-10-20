package io.github.tom039224.yhtye.ui.common

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.Ring

/** ドットの外側の呼吸する輪の明滅 (PC の `ypulse`: 0.25 → 1、1.4s)。 */
private const val PULSE_MS = 1_400
private const val PULSE_MIN = 0.25f

/**
 * プロジェクトの状態のドット (PC の ProjectDot。レールとダッシュボード): 作業が動いていれば (`running`) 緑に塗り、その外側に
 * 輪が呼吸する。それ以外 (待機中・停止) は中を抜いた灰色の円。[testTag] のノードの `stateDescription` に状態の名前。
 */
@Composable
fun ProjectDot(ring: Ring, testTag: String, modifier: Modifier = Modifier, diameter: Dp = 10.dp) {
    val c = Yhtye.colors
    val running = ring == Ring.Running
    val pulse = if (running) breatheAlpha(PULSE_MS, PULSE_MIN) else 1f
    // 輪の分だけ大きく取って、ドットはその真ん中
    Canvas(modifier.size(diameter + HALO * 2).testTag(testTag).semantics { stateDescription = RING_LABEL.getValue(ring) }) {
        val r = diameter.toPx() / 2
        if (running) {
            drawCircle(c.ringHead, radius = r)
            val halo = HALO_STROKE.toPx()
            drawCircle(c.ringTrack.copy(alpha = c.ringTrack.alpha * pulse), radius = r + HALO.toPx() - halo / 2, style = Stroke(halo))
        } else {
            val w = DOT_STROKE.toPx()
            drawCircle(c.ringStop, radius = r - w / 2, style = Stroke(w))
        }
    }
}

/** 輪の外側までの余白、輪の太さ、抜いた円の線の太さ (PC の inset -4.5px / 3px / 1.5px)。 */
private val HALO = 5.dp
private val HALO_STROKE = 3.5.dp
private val DOT_STROKE = 1.75.dp
