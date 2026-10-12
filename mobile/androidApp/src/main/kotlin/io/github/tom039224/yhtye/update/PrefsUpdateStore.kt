package io.github.tom039224.yhtye.update

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import io.github.tom039224.yhtye.shared.update.StoredUpdate
import io.github.tom039224.yhtye.shared.update.UpdateStore

/**
 * 更新の確認の記録 (自動で確認するか・最後に確認した時刻・見つけた新しい版) を SharedPreferences に残す。
 * この端末のアプリのものなので、ペアリングを解除しても残す (同期層の [io.github.tom039224.yhtye.shared.sync.ClientPrefs] とは別のファイル)。
 */
class PrefsUpdateStore(private val prefs: SharedPreferences) : UpdateStore {
    override fun load(): StoredUpdate = StoredUpdate(
        enabled = prefs.getBoolean(KEY_ENABLED, true),
        checkedAt = if (prefs.contains(KEY_CHECKED_AT)) prefs.getLong(KEY_CHECKED_AT, 0L) else null,
        availableTag = prefs.getString(KEY_AVAILABLE_TAG, null),
    )

    override fun save(stored: StoredUpdate) {
        prefs.edit {
            putBoolean(KEY_ENABLED, stored.enabled)
            val checkedAt = stored.checkedAt
            if (checkedAt == null) remove(KEY_CHECKED_AT) else putLong(KEY_CHECKED_AT, checkedAt)
            val tag = stored.availableTag
            if (tag == null) remove(KEY_AVAILABLE_TAG) else putString(KEY_AVAILABLE_TAG, tag)
        }
    }

    companion object {
        private const val FILE = "yhtye.updates"
        private const val KEY_ENABLED = "enabled"
        private const val KEY_CHECKED_AT = "checkedAt"
        private const val KEY_AVAILABLE_TAG = "availableTag"

        fun from(context: Context): PrefsUpdateStore =
            PrefsUpdateStore(context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE))
    }
}
