package io.github.tom039224.yhtye.shared

/** 実行中のプラットフォーム。端末固有の実装は各ターゲットの source set に置く。 */
interface Platform {
    val name: String
}

expect fun platform(): Platform

/** 画面に出すアプリ名。共通モジュールが組み込まれていることの確認も兼ねる。 */
const val APP_NAME: String = "Yhtye"

fun greeting(): String = "$APP_NAME on ${platform().name}"
