package io.github.tom039224.yhtye

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import io.github.tom039224.yhtye.credentials.KeystoreCredentialStore
import io.github.tom039224.yhtye.session.Session
import io.github.tom039224.yhtye.session.SharedPreferencesPrefs
import io.github.tom039224.yhtye.shared.client.createHttpClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.SupervisorJob

/** アプリ全体で 1 つのもの ([Session]。PC への接続と同期層) を持つ。前面/裏の切り替えで PC への接続を始め・止める。 */
class YhtyeApplication : Application() {
    lateinit var session: Session
        private set

    override fun onCreate() {
        super.onCreate()
        session = Session(
            store = KeystoreCredentialStore(this),
            http = createHttpClient(),
            scope = MainScope(),
            clientScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
            clientPrefs = SharedPreferencesPrefs(this),
        )
        // 画面の回転などでは発火しない (ProcessLifecycleOwner はアプリ全体の前面/裏)
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = session.onForeground()

                override fun onStop(owner: LifecycleOwner) = session.onBackground()
            },
        )
    }
}
