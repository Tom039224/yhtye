package io.github.tom039224.yhtye.ui.settings

import android.Manifest
import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import io.github.tom039224.yhtye.background.Channels

/** 通知が出せる状態か。 */
enum class NotificationState {
    Allowed,

    /** 許可されていない (Android 13 以降の権限が無い、アプリ全体か完了のチャネルを OS の設定で止めている)。 */
    Blocked,
}

/**
 * OS に関わる操作の窓口 (設定画面の「この端末」)。実物は [rememberSystemAccess]、テストは偽物。
 * 状態は呼ぶたびに OS に聞く (許可の画面や OS の設定から戻ったら画面が読み直す)。
 */
interface SystemAccess {
    fun notificationState(): NotificationState

    /** 通知の許可の画面を出せる (Android 13 以降で、まだ許可されていない)。最初のホームで 1 回尋ねるかの判断に使う。 */
    fun needsNotificationPermission(): Boolean

    /** Android 13 以降は通知の許可を求める (すでに断られていて画面が出ないときは [onResult] で分かる)。それ以前は OS の通知設定を開く。 */
    fun requestNotifications(onResult: () -> Unit = {})

    /** このアプリの OS の通知設定を開く。 */
    fun openNotificationSettings()

    /** 電池の最適化の対象外になっている (Doze の制限を受けにくい)。 */
    fun isIgnoringBatteryOptimizations(): Boolean

    /** 電池の最適化の対象から外す確認を出す (出せなければ OS の電池の最適化の一覧を開く)。 */
    fun requestIgnoreBatteryOptimizations()

    /** [url] (`https://`) をブラウザで開く (新しい版のリリースのページ)。それ以外の URL・開けるアプリが無いときは何もしない。 */
    fun openUrl(url: String)

    /** 何もしない (テストの既定)。 */
    object None : SystemAccess {
        override fun notificationState() = NotificationState.Allowed

        override fun needsNotificationPermission() = false

        override fun requestNotifications(onResult: () -> Unit) = onResult()

        override fun openNotificationSettings() = Unit

        override fun isIgnoringBatteryOptimizations() = true

        override fun requestIgnoreBatteryOptimizations() = Unit

        override fun openUrl(url: String) = Unit
    }
}

/** Android の実物。許可の画面の結果を受けるので Composable の中で作る。 */
@Composable
fun rememberSystemAccess(): SystemAccess {
    val context = LocalContext.current
    val pending = remember { arrayOfNulls<() -> Unit>(1) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        pending[0]?.invoke()
        pending[0] = null
    }
    val currentContext = rememberUpdatedState(context)
    return remember(launcher) {
        AndroidSystemAccess(
            context = currentContext,
            setPending = { callback -> pending[0] = callback },
            launchPermission = { launcher.launch(Manifest.permission.POST_NOTIFICATIONS) },
        )
    }
}

private class AndroidSystemAccess(
    private val context: State<Context>,
    private val setPending: (() -> Unit) -> Unit,
    private val launchPermission: () -> Unit,
) : SystemAccess {
    private val ctx get() = context.value

    private fun permissionGranted(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    override fun notificationState(): NotificationState {
        val manager = NotificationManagerCompat.from(ctx)
        val channel = manager.getNotificationChannel(Channels.WORK_DONE)
        val channelOff = channel != null && channel.importance == NotificationManagerCompat.IMPORTANCE_NONE
        return if (permissionGranted() && manager.areNotificationsEnabled() && !channelOff) NotificationState.Allowed else NotificationState.Blocked
    }

    override fun needsNotificationPermission(): Boolean = !permissionGranted()

    override fun requestNotifications(onResult: () -> Unit) {
        if (permissionGranted()) {
            // 権限はあるが OS の設定で止めている
            openNotificationSettings()
            return
        }
        setPending(onResult)
        launchPermission()
    }

    override fun openNotificationSettings() {
        val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, ctx.packageName)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        start(intent)
    }

    override fun isIgnoringBatteryOptimizations(): Boolean =
        ctx.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(ctx.packageName)

    // Play の方針は直接の除外の依頼を制限するが、このアプリの中心の機能 (裏での完了の通知) が Doze で止まるための依頼で、
    // 端末の持ち主が確認の画面で決める
    @SuppressLint("BatteryLife")
    override fun requestIgnoreBatteryOptimizations() {
        val direct = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${ctx.packageName}".toUri())
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (!start(direct)) start(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    override fun openUrl(url: String) {
        if (!url.startsWith("https://")) return
        start(Intent(Intent.ACTION_VIEW, url.toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun start(intent: Intent): Boolean = try {
        ctx.startActivity(intent)
        true
    } catch (e: ActivityNotFoundException) {
        false
    }
}
