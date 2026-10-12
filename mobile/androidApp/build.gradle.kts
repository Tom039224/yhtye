plugins {
    // AGP 9 は Kotlin を内蔵しているので org.jetbrains.kotlin.android は適用しない
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// 版: versionName は -Pyhtye.versionName=<major>.<minor>.<patch> (CI はタグ android-v<版> から)、無ければ gradle.properties の
// 開発用の既定値。versionCode = major*10000 + minor*100 + patch (マイナーとパッチは 0〜99。外れたらビルドを失敗させる)。
val appVersionName: String = providers.gradleProperty("yhtye.versionName").orNull
    ?: error("yhtye.versionName is not set (see mobile/gradle.properties)")
val appVersionCode: Int = run {
    val m = Regex("""(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)""").matchEntire(appVersionName)
        ?: throw GradleException("yhtye.versionName must be <major>.<minor>.<patch> (digits only, no leading zeros): '$appVersionName'")
    val (major, minor, patch) = m.destructured
    val minorNumber = minor.toIntOrNull()?.takeIf { it in 0..99 }
        ?: throw GradleException("the minor version of yhtye.versionName must be 0..99 (versionCode has two digits for it): '$appVersionName'")
    val patchNumber = patch.toIntOrNull()?.takeIf { it in 0..99 }
        ?: throw GradleException("the patch version of yhtye.versionName must be 0..99 (versionCode has two digits for it): '$appVersionName'")
    // versionCode は Int。major が大きすぎれば溢れる前に失敗する
    val majorNumber = major.toIntOrNull()?.takeIf { it <= (Int.MAX_VALUE - 9999) / 10000 }
        ?: throw GradleException("the major version of yhtye.versionName is too large: '$appVersionName'")
    majorNumber * 10000 + minorNumber * 100 + patchNumber
}

// release の署名: 鍵は CI が Secrets から一時ファイルに復元して環境変数で渡す (リポジトリには入れない)。
// 4 つとも無ければ release は未署名のまま (ローカルの ./gradlew build を壊さない)。一部だけ、またはファイルが無いのは間違いなので失敗させる。
val signingVariables = listOf("YHTYE_KEYSTORE_FILE", "YHTYE_KEYSTORE_PASSWORD", "YHTYE_KEY_ALIAS", "YHTYE_KEY_PASSWORD")
val signing: Map<String, String> = signingVariables.mapNotNull { name ->
    providers.environmentVariable(name).orNull?.takeIf { it.isNotEmpty() }?.let { name to it }
}.toMap()
if (signing.isNotEmpty() && signing.size != signingVariables.size) {
    throw GradleException("Set all of $signingVariables to sign the release build (missing: ${signingVariables - signing.keys})")
}
val releaseKeystore: File? = signing["YHTYE_KEYSTORE_FILE"]?.let { path ->
    file(path).also { if (!it.isFile) throw GradleException("YHTYE_KEYSTORE_FILE is not a file: $path") }
}

android {
    namespace = "io.github.tom039224.yhtye"
    compileSdk = libs.versions.compileSdk.get().toInt()

    defaultConfig {
        // デスクトップアプリ (Tauri) の識別子と同じ。OS が違うので衝突しない
        applicationId = "io.github.tom039224.yhtye"
        minSdk = libs.versions.minSdk.get().toInt()
        targetSdk = libs.versions.targetSdk.get().toInt()
        versionCode = appVersionCode
        versionName = appVersionName
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = signing.getValue("YHTYE_KEYSTORE_PASSWORD")
                keyAlias = signing.getValue("YHTYE_KEY_ALIAS")
                keyPassword = signing.getValue("YHTYE_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    buildFeatures {
        compose = true
        // アプリから版 (BuildConfig.VERSION_NAME) を読む
        buildConfig = true
    }

    testOptions {
        // Compose の UI テストを Robolectric (JVM) で回す。リソース (文字列) を読む
        unitTests.isIncludeAndroidResources = true
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(project(":shared"))

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.androidx.camera.mlkit.vision)
    implementation(libs.mlkit.barcode.scanning)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.androidx.window.core)
    implementation(libs.markdown)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.kotlin.test)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.ktor.client.mock)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
}
