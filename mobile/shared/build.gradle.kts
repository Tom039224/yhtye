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
            implementation(libs.kotlinx.coroutines.core)
            implementation(libs.kotlinx.serialization.json)
            implementation(libs.ktor.client.core)
            implementation(libs.ktor.client.websockets)
            implementation(libs.ktor.client.content.negotiation)
            implementation(libs.ktor.serialization.kotlinx.json)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.kotlinx.coroutines.test)
        }
        androidMain.dependencies {
            implementation(libs.kotlinx.coroutines.android)
            implementation(libs.ktor.client.okhttp)
        }
    }
}
