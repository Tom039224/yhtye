package io.github.tom039224.yhtye.credentials

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.Base64
import android.util.Log
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import io.github.tom039224.yhtye.shared.credentials.CredentialStore
import io.github.tom039224.yhtye.shared.credentials.Credentials
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private val Context.credentialsDataStore: DataStore<Preferences> by preferencesDataStore(name = "credentials")

/**
 * 認証情報を DataStore に保存する。端末トークンだけは Android Keystore の AES-GCM 鍵で暗号化する
 * (鍵は取り出せず、StrongBox があればそこに作る)。EncryptedSharedPreferences (非推奨) は使わない。
 *
 * 復号できない (鍵が消えた・バックアップから戻した等) ときは保存内容を捨てて「ペアリングしていない」に戻る。
 * トークンはログに出さない。
 */
class KeystoreCredentialStore(context: Context) : CredentialStore {
    private val dataStore = context.applicationContext.credentialsDataStore

    override suspend fun load(): Credentials? = withContext(Dispatchers.IO) {
        val prefs = dataStore.data.first()
        val serverId = prefs[SERVER_ID] ?: return@withContext null
        val hosts = prefs[HOSTS]?.split(HOST_SEPARATOR)?.filter { it.isNotEmpty() }.orEmpty()
        val port = prefs[PORT]
        val deviceId = prefs[DEVICE_ID]
        val deviceName = prefs[DEVICE_NAME]
        val iv = prefs[TOKEN_IV]
        val sealed = prefs[TOKEN]
        if (hosts.isEmpty() || port == null || deviceId == null || deviceName == null || iv == null || sealed == null) {
            Log.w(TAG, "incomplete stored credentials; dropping them")
            clear()
            return@withContext null
        }
        val token = try {
            decrypt(iv, sealed, aad(serverId, deviceId))
        } catch (e: GeneralSecurityException) {
            Log.w(TAG, "the stored device token cannot be decrypted (${e::class.java.simpleName}); dropping it")
            clear()
            return@withContext null
        } catch (e: IllegalArgumentException) { // 壊れた Base64
            Log.w(TAG, "the stored device token is corrupt; dropping it")
            clear()
            return@withContext null
        }
        Credentials(serverId, hosts, port, deviceId, deviceName, token)
    }

    override suspend fun save(credentials: Credentials) = withContext(Dispatchers.IO) {
        val (iv, sealed) = encrypt(credentials.token, aad(credentials.serverId, credentials.deviceId))
        dataStore.edit { prefs ->
            prefs.clear()
            prefs[SERVER_ID] = credentials.serverId
            prefs[HOSTS] = credentials.hosts.joinToString(HOST_SEPARATOR)
            prefs[PORT] = credentials.port
            prefs[DEVICE_ID] = credentials.deviceId
            prefs[DEVICE_NAME] = credentials.deviceName
            prefs[TOKEN_IV] = iv
            prefs[TOKEN] = sealed
        }
        Unit
    }

    override suspend fun clear() = withContext(Dispatchers.IO) {
        dataStore.edit { it.clear() }
        // 次のペアリングでは新しい鍵を作る
        runCatching { keyStore().deleteEntry(KEY_ALIAS) }
            .onFailure { Log.w(TAG, "could not delete the token key (${it::class.java.simpleName})") }
        Unit
    }

    /** 暗号文を別の PC・端末の記録に付け替えられないよう、サーバー ID と端末 ID を認証付きデータにする。 */
    private fun aad(serverId: String, deviceId: String): ByteArray = "$serverId\n$deviceId".encodeToByteArray()

    private fun encrypt(token: String, aad: ByteArray): Pair<String, String> {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key(create = true))
        cipher.updateAAD(aad)
        val sealed = cipher.doFinal(token.encodeToByteArray())
        return base64(cipher.iv) to base64(sealed)
    }

    private fun decrypt(iv: String, sealed: String, aad: ByteArray): String {
        val key = key(create = false) ?: throw GeneralSecurityException("no key")
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, unbase64(iv)))
        cipher.updateAAD(aad)
        return cipher.doFinal(unbase64(sealed)).decodeToString()
    }

    private fun key(create: Boolean): SecretKey? {
        (keyStore().getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        if (!create) return null
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            try {
                generateKey(strongBox = true)
            } catch (e: StrongBoxUnavailableException) {
                generateKey(strongBox = false)
            }
        } else {
            generateKey(strongBox = false)
        }
    }

    private fun generateKey(strongBox: Boolean): SecretKey {
        val spec = KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(KEY_BITS)
            .setRandomizedEncryptionRequired(true)
            .apply { if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setIsStrongBoxBacked(true) }
            .build()
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(spec)
        return generator.generateKey()
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    private fun base64(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.NO_WRAP)

    private fun unbase64(text: String): ByteArray = Base64.decode(text, Base64.NO_WRAP)

    private companion object {
        const val TAG = "YhtyeCredentials"
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "yhtye.device_token"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val KEY_BITS = 256
        const val GCM_TAG_BITS = 128
        const val HOST_SEPARATOR = "\n"

        val SERVER_ID = stringPreferencesKey("server_id")
        val HOSTS = stringPreferencesKey("hosts")
        val PORT = intPreferencesKey("port")
        val DEVICE_ID = stringPreferencesKey("device_id")
        val DEVICE_NAME = stringPreferencesKey("device_name")
        val TOKEN_IV = stringPreferencesKey("token_iv")
        val TOKEN = stringPreferencesKey("token_sealed")
    }
}
