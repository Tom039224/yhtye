package io.github.tom039224.yhtye.shared.pairing

/** PC のリモート接続サーバーの既定ポート (`yhtye_core::remote::DEFAULT_PORT`)。 */
const val DEFAULT_PORT: Int = 1423

/** ペアリングコードの長さと文字種 (PC の `remote/pairing.rs` と同じ。0/O・1/I/L を除く)。 */
const val PAIRING_CODE_LENGTH: Int = 8
const val PAIRING_CODE_ALPHABET: String = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"

/** 端末名の上限 (PC の `MAX_DEVICE_NAME_CHARS`)。 */
const val MAX_DEVICE_NAME_CHARS: Int = 64

/** このアプリが読める QR の版 (`v=`)。 */
private const val PAIRING_URI_VERSION = "1"
private const val PAIRING_URI_PREFIX = "yhtye://pair"

/**
 * ペアリングの相手 (QR / ディープリンク / 手入力から)。
 *
 * @property hosts 試す順の接続先 (MagicDNS 名 → IP)。どれも Tailscale のアドレス ([TailscaleHosts])。
 * @property code 正規化したコード (大文字、区切り無しの 8 文字)。
 * @property serverId QR の `sid`。手入力では無い。あれば応答の `server_id` と照合する (接続先の取り違えの検出)。
 */
data class PairingTarget(
    val hosts: List<String>,
    val port: Int,
    val code: String,
    val serverId: String?,
) {
    init {
        require(hosts.isNotEmpty()) { "no host" }
    }

    // コードは画面やログに出すものではない
    override fun toString(): String = "PairingTarget(hosts=$hosts, port=$port, serverId=$serverId)"
}

/** ペアリングの入力が使えない理由 (画面で文言にする)。 */
sealed interface InvalidPairingInput {
    /** `yhtye://pair?...` ではない (別の QR など)。 */
    data object NotPairingUri : InvalidPairingInput

    /** このアプリより新しい形式の QR。アプリの更新が要る。 */
    data class UnsupportedVersion(val version: String) : InvalidPairingInput

    data object MissingHost : InvalidPairingInput

    /** Tailscale のアドレス (100.64.0.0/10、fd7a:115c:a1e0::/48、`*.ts.net`) ではない。 */
    data class NotTailscaleHost(val host: String) : InvalidPairingInput

    data object InvalidPort : InvalidPairingInput

    data object MissingCode : InvalidPairingInput

    /** 8 文字でない、または使わない文字 (0/O・1/I/L など) を含む。 */
    data object InvalidCode : InvalidPairingInput
}

class InvalidPairingInputException(val reason: InvalidPairingInput) :
    IllegalArgumentException("invalid pairing input: $reason")

/**
 * 人が打ったコードを PC と同じ形にする (空白と `-` を除いて大文字に。`abcd-2345` → `ABCD2345`)。
 * 長さや文字種が合わなければ null。
 */
fun normalizePairingCode(input: String): String? {
    val code = input.toHalfWidth().filter { !it.isWhitespace() && it != '-' }.uppercase()
    return code.takeIf { it.length == PAIRING_CODE_LENGTH && it.all { c -> c in PAIRING_CODE_ALPHABET } }
}

/**
 * 全角の英数字・記号・空白を半角にする (日本語の IME が全角で入れた `１００．８９…` や `ＡＢＣＤ－２３４５` を受け付ける)。
 */
internal fun String.toHalfWidth(): String = buildString(length) {
    for (c in this@toHalfWidth) {
        append(
            when (c) {
                in FULLWIDTH_FIRST..FULLWIDTH_LAST -> c - FULLWIDTH_OFFSET
                IDEOGRAPHIC_SPACE -> ' '
                else -> c
            },
        )
    }
}

private const val FULLWIDTH_FIRST = '\uFF01'
private const val FULLWIDTH_LAST = '\uFF5E'
private const val FULLWIDTH_OFFSET = 0xFEE0
private const val IDEOGRAPHIC_SPACE = '\u3000'

/** 端末名を PC と同じ規則で整える (制御文字と前後の空白を除く。1〜64 文字)。合わなければ null。 */
fun normalizeDeviceName(input: String): String? {
    val name = input.filter { !it.isISOControl() }.trim()
    return name.takeIf { it.isNotEmpty() && it.codePointLength() <= MAX_DEVICE_NAME_CHARS }
}

private fun String.codePointLength(): Int = length - count { it.isLowSurrogate() }

/** 文字列が `yhtye://pair` の URI に見えるか (QR の読み取り中に、関係ない QR を素通りさせる判定)。 */
fun looksLikePairingUri(text: String): Boolean = text.trim().startsWith(PAIRING_URI_PREFIX, ignoreCase = true)

/**
 * QR / ディープリンクの `yhtye://pair?v=1&host=<MagicDNS 名>&ip=<100.x>&port=<N>&code=<コード>&sid=<サーバー ID>`
 * を読む。`host` は無いことがある。値はパーセントエンコード。
 */
fun parsePairingUri(uri: String): Result<PairingTarget> = runCatching {
    val text = uri.trim()
    if (!looksLikePairingUri(text)) invalid(InvalidPairingInput.NotPairingUri)
    val rest = text.substring(PAIRING_URI_PREFIX.length)
    if (rest.isNotEmpty() && !rest.startsWith("?") && rest != "/" && !rest.startsWith("/?")) {
        invalid(InvalidPairingInput.NotPairingUri)
    }
    val params = parseQuery(rest.substringAfter('?', "").substringBefore('#'))
    val version = params["v"] ?: invalid(InvalidPairingInput.NotPairingUri)
    if (version != PAIRING_URI_VERSION) invalid(InvalidPairingInput.UnsupportedVersion(version))
    val hosts = listOfNotNull(params["host"], params["ip"]).filter { it.isNotBlank() }
    build(
        hosts = hosts,
        port = params["port"],
        code = params["code"],
        serverId = params["sid"]?.takeIf { it.isNotBlank() },
    )
}.mapInvalid()

/** 手入力 (ホスト名か IP・ポート・コード) を読む。ポートが空なら既定の 1423。 */
fun parseManualEntry(host: String, port: String, code: String): Result<PairingTarget> = runCatching {
    build(
        hosts = listOf(host).filter { it.isNotBlank() },
        port = port.trim().ifEmpty { DEFAULT_PORT.toString() },
        code = code,
        serverId = null,
    )
}.mapInvalid()

private fun build(hosts: List<String>, port: String?, code: String?, serverId: String?): PairingTarget {
    if (hosts.isEmpty()) invalid(InvalidPairingInput.MissingHost)
    val normalized = hosts.map(TailscaleHosts::normalize).distinct()
    normalized.firstOrNull { !TailscaleHosts.isTailscaleHost(it) }?.let {
        invalid(InvalidPairingInput.NotTailscaleHost(it))
    }
    val portNumber = port?.toHalfWidth()?.trim()
        ?.takeIf { it.isNotEmpty() && it.length <= 5 && it.all { c -> c in '0'..'9' } }
        ?.toInt()?.takeIf { it in 1..65535 }
        ?: invalid(InvalidPairingInput.InvalidPort)
    if (code.isNullOrBlank()) invalid(InvalidPairingInput.MissingCode)
    val normalizedCode = normalizePairingCode(code) ?: invalid(InvalidPairingInput.InvalidCode)
    return PairingTarget(normalized, portNumber, normalizedCode, serverId)
}

private fun invalid(reason: InvalidPairingInput): Nothing = throw InvalidPairingInputException(reason)

/** 想定外の例外 (バグ) はそのまま投げる。入力の誤りだけを失敗として返す。 */
private fun Result<PairingTarget>.mapInvalid(): Result<PairingTarget> =
    onFailure { if (it !is InvalidPairingInputException) throw it }

/** `a=1&b=x%20y` → 最初に現れた値 (後のものは無視)。 */
private fun parseQuery(query: String): Map<String, String> {
    val out = LinkedHashMap<String, String>()
    for (pair in query.split('&')) {
        if (pair.isEmpty()) continue
        val key = percentDecode(pair.substringBefore('='))
        val value = percentDecode(pair.substringAfter('=', ""))
        out.getOrPut(key) { value }
    }
    return out
}

/** RFC 3986 のパーセントデコード (UTF-8)。`+` は空白にしない。壊れたエスケープはそのまま残す。 */
private fun percentDecode(s: String): String {
    if ('%' !in s) return s
    val out = StringBuilder()
    val bytes = ArrayList<Byte>()
    fun flush() {
        if (bytes.isNotEmpty()) out.append(bytes.toByteArray().decodeToString())
        bytes.clear()
    }
    var i = 0
    while (i < s.length) {
        val hex = if (s[i] == '%' && i + 2 < s.length) hexByte(s[i + 1], s[i + 2]) else null
        if (hex != null) {
            bytes.add(hex)
            i += 3
        } else {
            flush()
            out.append(s[i])
            i += 1
        }
    }
    flush()
    return out.toString()
}

private fun hexByte(hi: Char, lo: Char): Byte? {
    val h = hi.digitToIntOrNull(16)?.takeIf { hi.code < 128 } ?: return null
    val l = lo.digitToIntOrNull(16)?.takeIf { lo.code < 128 } ?: return null
    return (h * 16 + l).toByte()
}
