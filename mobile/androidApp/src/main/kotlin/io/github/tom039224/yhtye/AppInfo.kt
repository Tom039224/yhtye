package io.github.tom039224.yhtye

import io.github.tom039224.yhtye.shared.version.AppVersion

/** このアプリ自身。版は Gradle の `yhtye.versionName` ([BuildConfig.VERSION_NAME]。リリースはタグから渡す)。 */
object ThisApp {
    /** このアプリの版。ビルドが読めない版を渡したときは `0.0.0` (どんな更新よりも古い)。 */
    val version: AppVersion = AppVersion.parse(BuildConfig.VERSION_NAME) ?: AppVersion(0, 0, 0)
}
