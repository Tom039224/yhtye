plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.kotlin.multiplatform.library) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}

// テストが止まったまま CI の 30 分枠いっぱいまで待ち続けないよう、テストの実行に上限を置く。
// 普段は 1〜2 分で終わる (遅い 1 CPU でも約 1.5 分) ので、10 分あれば足りる。超えたらそのタスクが失敗する。
// 止まったときにどのテストだったか分かるよう、開始したテストをログに出す。
subprojects {
    tasks.withType<Test>().configureEach {
        timeout.set(java.time.Duration.ofMinutes(10))
        testLogging {
            events("started", "failed", "skipped")
            exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
        }
    }
}
