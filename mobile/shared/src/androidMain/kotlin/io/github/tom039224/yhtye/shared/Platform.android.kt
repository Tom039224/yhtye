package io.github.tom039224.yhtye.shared

import android.os.Build

private object AndroidPlatform : Platform {
    override val name: String = "Android ${Build.VERSION.RELEASE}"
}

actual fun platform(): Platform = AndroidPlatform
