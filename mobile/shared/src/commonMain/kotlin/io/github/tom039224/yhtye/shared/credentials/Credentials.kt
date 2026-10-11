package io.github.tom039224.yhtye.shared.credentials

/**
 * ペアリングで得た、PC につなぐための情報。
 *
 * @property serverId PC の Yhtye の ID (QR の `sid`)。
 * @property hosts 接続先の候補 (試す順。ペアリングで届いたものが先頭)。どれも Tailscale のアドレス。
 * @property token 端末トークン (`Authorization: Bearer`)。画面やログに出さない ([toString] は伏せる)。
 */
data class Credentials(
    val serverId: String,
    val hosts: List<String>,
    val port: Int,
    val deviceId: String,
    val deviceName: String,
    val token: String,
) {
    init {
        require(hosts.isNotEmpty()) { "no host" }
    }

    override fun toString(): String =
        "Credentials(serverId=$serverId, hosts=$hosts, port=$port, deviceId=$deviceId, deviceName=$deviceName, token=***)"
}

/**
 * 認証情報の保存先。端末固有の実装 (Android は Keystore で暗号化) を差し込む境界。
 * 保存するのは 1 台の PC の分だけ (ペアリングし直すと置き換える)。
 */
interface CredentialStore {
    /** 保存されたもの。無い・読めない (鍵が失われた等) なら null。 */
    suspend fun load(): Credentials?

    suspend fun save(credentials: Credentials)

    /** 端末内の認証情報を消す (PC 側の端末登録は消えない。失効は PC からだけ)。 */
    suspend fun clear()
}

/** メモリだけに持つ実装 (テスト用)。 */
class InMemoryCredentialStore(initial: Credentials? = null) : CredentialStore {
    private var stored: Credentials? = initial

    override suspend fun load(): Credentials? = stored

    override suspend fun save(credentials: Credentials) {
        stored = credentials
    }

    override suspend fun clear() {
        stored = null
    }
}
