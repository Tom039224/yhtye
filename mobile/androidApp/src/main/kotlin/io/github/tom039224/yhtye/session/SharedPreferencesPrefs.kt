package io.github.tom039224.yhtye.session

import android.content.Context
import androidx.core.content.edit
import io.github.tom039224.yhtye.shared.sync.ClientPrefs

/**
 * 同期層のこの端末だけの設定 (最後のプロジェクト・表示していたチャット・下書き) を SharedPreferences に残す。
 * 秘密は入らないので暗号化しない (端末トークンは KeystoreCredentialStore)。バックアップはアプリ全体で無効。
 */
class SharedPreferencesPrefs(context: Context) : ClientPrefs {
    private val prefs = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun set(key: String, value: String?) {
        prefs.edit { if (value == null) remove(key) else putString(key, value) }
    }

    override fun clear() {
        prefs.edit { clear() }
    }

    private companion object {
        const val FILE = "yhtye.client"
    }
}
