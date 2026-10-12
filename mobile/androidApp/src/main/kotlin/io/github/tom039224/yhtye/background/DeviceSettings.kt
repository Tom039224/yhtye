package io.github.tom039224.yhtye.background

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * この端末だけの設定: 裏での接続の保ち方、ペアリング済みか (再起動後に常駐を始めるかの手がかり)、通知の許可を一度尋ねたか。
 * PC のものではない (ペアリングを解除しても残す) ので同期層の [io.github.tom039224.yhtye.shared.sync.ClientPrefs] とは別のファイル。
 * 再起動後の受信機 ([BootReceiver]) が Session の読み込みを待たずに読めるよう、SharedPreferences に置く。
 */
class DeviceSettings(private val prefs: SharedPreferences) {
    private val _mode = MutableStateFlow(BackgroundMode.parse(prefs.getString(KEY_MODE, null)))

    /** 裏での接続の保ち方 (既定は「常に接続」)。 */
    val mode: StateFlow<BackgroundMode> = _mode.asStateFlow()

    fun setMode(mode: BackgroundMode) {
        prefs.edit { putString(KEY_MODE, mode.wire) }
        _mode.value = mode
    }

    /** ペアリングしている (Session が書く)。端末の再起動後に常駐を始めるかの手がかり。 */
    var paired: Boolean
        get() = prefs.getBoolean(KEY_PAIRED, false)
        set(value) = prefs.edit { putBoolean(KEY_PAIRED, value) }

    /** 通知の許可を一度尋ねた (尋ねるのは最初のホームで 1 回。あとは設定画面から)。 */
    var notificationsAsked: Boolean
        get() = prefs.getBoolean(KEY_ASKED, false)
        set(value) = prefs.edit { putBoolean(KEY_ASKED, value) }

    companion object {
        private const val FILE = "yhtye.device"
        private const val KEY_MODE = "backgroundMode"
        private const val KEY_PAIRED = "paired"
        private const val KEY_ASKED = "notificationsAsked"

        fun from(context: Context): DeviceSettings =
            DeviceSettings(context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE))
    }
}
