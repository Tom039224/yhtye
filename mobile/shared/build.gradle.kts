plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.kotlin.multiplatform.library)
    alias(libs.plugins.kotlin.serialization)
}

kotlin {
    // AGP 9 では KMP の Android ターゲットは androidTarget() (com.android.library) ではなく
    // com.android.kotlin.multiplatform.library の android {} で宣言する。
    // commonMain には Android 依存を入れない (将来 iOS ターゲットを足せるように)。
    android {
        namespace = "io.github.tom039224.yhtye.shared"
        compileSdk = libs.versions.compileSdk.get().toInt()
        minSdk = libs.versions.minSdk.get().toInt()

        withHostTestBuilder {}
    }

    jvmToolchain(17)

    sourceSets {
        commonMain.dependencies {
            // 公開する型に現れる (StateFlow、JsonElement、HttpClient) ので api
            api(libs.kotlinx.coroutines.core)
            api(libs.kotlinx.serialization.json)
            api(libs.ktor.client.core)
            implementation(libs.ktor.client.websockets)
            implementation(libs.ktor.client.content.negotiation)
            implementation(libs.ktor.serialization.kotlinx.json)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.kotlinx.coroutines.test)
            implementation(libs.ktor.client.mock)
        }
        androidMain.dependencies {
            implementation(libs.kotlinx.coroutines.android)
            implementation(libs.ktor.client.okhttp)
        }
    }
}

// 本物のサーバーとの結合テスト (RealServerTest) は YHTYE_REAL_SERVER_TEST=1 のときだけ動く。
// そのときは毎回実行する (サーバー・Tailscale の状態はテストの入力に表れないので、結果を再利用しない)。
val realServerTest = providers.environmentVariable("YHTYE_REAL_SERVER_TEST").orNull
tasks.withType<Test>().configureEach {
    if (realServerTest != null) {
        outputs.upToDateWhen { false }
        outputs.cacheIf { false }
        environment("YHTYE_REAL_SERVER_TEST", realServerTest)
    }
}
