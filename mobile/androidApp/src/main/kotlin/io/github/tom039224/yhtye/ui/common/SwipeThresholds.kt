package io.github.tom039224.yhtye.ui.common

import androidx.compose.ui.unit.dp

/**
 * スワイプで操作が確定するのに必要な指の移動量・速さ (T-72)。モックの値 (T-67 / T-69) の約 7 割。
 * 縦か横かを決める最初の判定 (`touchSlop`)・画面端の除外・アニメーションの速さはここに含めない。
 * 括弧内は T-72 の前の値。
 */
internal object SwipeThresholds {
    /** 離したとき、今の位置から隣の位置までの道のりのこの割合を超えて動いていれば、パネルを開く/閉じる (50%。最寄りのスナップ)。 */
    const val PANEL_COMMIT_SHARE = 0.35f

    /** 離したときにこの速さ (dp/s) を超えていればその向きへ。パネルとタスク詳細で共通 (350)。 */
    const val FLING_DP_PER_SECOND = 245f

    /** タスクの行: これを超えて離すとメンション (88dp)。 */
    val ROW_ARM = 62.dp

    /** タスクの行: この先は指に付いて行かず重くなる。[ROW_ARM] との比を保つ (132dp)。 */
    val ROW_MAX_PULL = 92.dp

    /** タスクの行: 短くてもこの速さ (dp/s) ではじけばメンション (450)。 */
    const val ROW_FLING_DP_PER_SECOND = 315f

    /** タスクの行: はじきで確定するのに最低限必要な距離 (40dp)。 */
    val ROW_FLING_MIN = 28.dp

    /**
     * タスク詳細: 幅のこの割合を超えて (指の移動量で) 離すと閉じる (35%)。T-74 で画面が指に 1:1 で付くようにしたとき、
     * それまで実機で必要だった指の移動量 (画面が指の半分しかずれなかったので 24.5% のずれ = 指で約 49%) を保った。
     */
    const val DETAIL_CLOSE_SHARE = 0.49f
}
