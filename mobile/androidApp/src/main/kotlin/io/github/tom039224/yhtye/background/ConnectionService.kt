package io.github.tom039224.yhtye.background

import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import io.github.tom039224.yhtye.YhtyeApplication
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

/**
 * 裏でも PC とつなぎ続けるための Foreground Service。接続そのものは [io.github.tom039224.yhtye.session.Session] の同期層が
 * 持っている (アクティビティから独立している) ので、これは「プロセスを生かし、常設の通知を出す」だけ。
 * 動かすか ([io.github.tom039224.yhtye.session.Session.keepAlive]) は Session が決め、`false` になったら自分で止まる。
 *
 * 種類は `specialUse` (理由は docs/planned/android-client.md「実装で決めたこと (T-71)」): `dataSync` は Android 15 以降 24 時間に 6 時間
 * までで、再起動後の `BOOT_COMPLETED` からも始められない。
 */
class ConnectionService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var observing = false
    private var stopping = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val app = application as YhtyeApplication
        try {
            startForeground(app, connected = false)
        } catch (e: RuntimeException) {
            // 裏から始められない (Android 12 以降) など。常駐できないだけで、アプリが前面の間の接続には関係ない
            Log.w(TAG, "could not start the foreground service", e)
            stopSelf()
            return START_NOT_STICKY
        }
        if (!observing) {
            observing = true
            observe(app)
        }
        return START_STICKY
    }

    private fun observe(app: YhtyeApplication) {
        scope.launch {
            // Session が決めるまで (null) は動かしておく。止めるのは false になってから
            app.session.keepAlive.collect { keep -> if (keep == false) stop() }
        }
        scope.launch {
            combine(app.session.connected, app.device.mode) { connected, mode -> connected to mode }
                .distinctUntilChanged()
                .collect { (connected, _) -> if (!stopping) startForeground(app, connected) }
        }
    }

    /** 常設の通知を消して止まる (止まった後に通知を出し直さないよう、見張りも止める)。 */
    private fun stop() {
        if (stopping) return
        stopping = true
        scope.coroutineContext.cancelChildren()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /** 常設の通知を出して(か更新して)前面のサービスにする。 */
    private fun startForeground(app: YhtyeApplication, connected: Boolean) {
        val notification = app.notifications.serviceNotification(connected, app.device.mode.value)
        ServiceCompat.startForeground(this, SystemNotifications.SERVICE_ID, notification, foregroundType())
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        private const val TAG = "ConnectionService"

        /** Android 14 以降は `specialUse` を宣言する。それ以前は種類が無いので manifest に従う。 */
        private fun foregroundType(): Int =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            } else {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MANIFEST
            }

        /**
         * 常駐を始める (動いていれば通知を更新するだけ)。アプリが前面か、再起動・更新の直後の受信機から呼ぶ。裏から
         * 始められなければ (例外) 何もしない (接続はアプリが前面の間だけになる)。
         */
        fun start(context: Context) {
            try {
                ContextCompat.startForegroundService(context, Intent(context, ConnectionService::class.java))
            } catch (e: RuntimeException) {
                Log.w(TAG, "could not start the foreground service", e)
            }
        }
    }
}
