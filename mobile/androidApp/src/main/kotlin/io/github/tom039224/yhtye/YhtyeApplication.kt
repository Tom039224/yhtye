package io.github.tom039224.yhtye

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import io.github.tom039224.yhtye.background.ConnectionService
import io.github.tom039224.yhtye.background.DeviceSettings
import io.github.tom039224.yhtye.background.SystemNotifications
import io.github.tom039224.yhtye.credentials.KeystoreCredentialStore
import io.github.tom039224.yhtye.session.Session
import io.github.tom039224.yhtye.session.SharedPreferencesPrefs
import io.github.tom039224.yhtye.shared.client.createHttpClient
import io.github.tom039224.yhtye.shared.update.GithubReleaseSource
import io.github.tom039224.yhtye.shared.update.UpdateChecker
import io.github.tom039224.yhtye.update.PrefsUpdateStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * アプリ全体で 1 つのもの ([Session]。PC への接続と同期層、[UpdateChecker]。アプリの更新の確認) を持つ。前面/裏の切り替えを
 * Session に伝え、裏でも接続を保つ判断 ([Session.keepAlive]) に従って Foreground Service ([ConnectionService]) を始める
 * (止まるのはサービス自身)。更新の確認はアプリが前面に来たときに、前回から 24 時間以上たっていれば行う。
 */
class YhtyeApplication : Application() {
    lateinit var session: Session
        private set
    lateinit var updates: UpdateChecker
        private set
    lateinit var device: DeviceSettings
        private set
    lateinit var notifications: SystemNotifications
        private set

    override fun onCreate() {
        super.onCreate()
        device = DeviceSettings.from(this)
        notifications = SystemNotifications(this).also { it.ensureChannels() }
        val main = MainScope()
        val http = createHttpClient()
        updates = UpdateChecker(GithubReleaseSource(http), ThisApp.version, PrefsUpdateStore.from(this), main)
        session = Session(
            store = KeystoreCredentialStore(this),
            http = http,
            scope = main,
            clientScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
            clientPrefs = SharedPreferencesPrefs(this),
            device = device,
            notifications = notifications,
        )
        main.launch {
            session.keepAlive.collect { keep -> if (keep == true) ConnectionService.start(this@YhtyeApplication) }
        }
        // 画面の回転などでは発火しない (ProcessLifecycleOwner はアプリ全体の前面/裏)
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    session.onForeground()
                    updates.onForeground()
                }

                override fun onStop(owner: LifecycleOwner) = session.onBackground()
            },
        )
    }
}
