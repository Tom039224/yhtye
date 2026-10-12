package io.github.tom039224.yhtye.shared.pairing

/**
 * アプリが通信してよい相手: Tailscale のアドレスだけ (PC もそこでしか待ち受けない)。
 *
 * - IPv4 の 100.64.0.0/10 (Tailscale の CGNAT 範囲)
 * - IPv6 の fd7a:115c:a1e0::/48 (Tailscale の ULA 範囲)
 * - MagicDNS 名 (`*.ts.net`)
 *
 * 通信は平文 (`http://` / `ws://`。暗号化は下の WireGuard がする) で、Android の network security config は
 * IP の範囲を書けない。ペアリングコードや端末トークンを他所へ送らないための歯止めはこの判定
 * (`androidApp` の `network_security_config.xml` を参照)。
 */
object TailscaleHosts {
    private const val MAGIC_DNS_SUFFIX = ".ts.net"
    private const val CGNAT_FIRST_OCTET = 100
    private val CGNAT_SECOND_OCTET = 64..127
    private val ULA_PREFIX = listOf(0xfd7a, 0x115c, 0xa1e0)

    /** 入力や QR のホストを比べられる形にする: 全角を半角に、前後の空白・角括弧・末尾の `.` を除き小文字に。 */
    fun normalize(host: String): String =
        host.toHalfWidth().trim().removePrefix("[").removeSuffix("]").removeSuffix(".").lowercase()

    /** [host] が Tailscale の IP アドレスか MagicDNS 名か。 */
    fun isTailscaleHost(host: String): Boolean {
        val h = normalize(host)
        return isTailscaleIpv4(h) || isTailscaleIpv6(h) || isMagicDnsName(h)
    }

    /** URL に書くときのホスト (IPv6 は角括弧で囲む)。 */
    fun urlHost(host: String): String {
        val h = normalize(host)
        return if (':' in h) "[$h]" else h
    }

    private fun isTailscaleIpv4(h: String): Boolean {
        val octets = parseIpv4(h) ?: return false
        return octets[0] == CGNAT_FIRST_OCTET && octets[1] in CGNAT_SECOND_OCTET
    }

    private fun isTailscaleIpv6(h: String): Boolean {
        val groups = parseIpv6(h) ?: return false
        return groups.take(ULA_PREFIX.size) == ULA_PREFIX
    }

    private fun isMagicDnsName(h: String): Boolean {
        if (!h.endsWith(MAGIC_DNS_SUFFIX)) return false
        val labels = h.split('.')
        // 少なくとも <マシン>.<tailnet>.ts.net
        return labels.size >= 4 && labels.all(::isDnsLabel)
    }

    private fun isDnsLabel(label: String): Boolean =
        label.isNotEmpty() && label.length <= 63 &&
            label.first() != '-' && label.last() != '-' &&
            label.all { it in 'a'..'z' || it in '0'..'9' || it == '-' }

    private fun parseIpv4(h: String): List<Int>? {
        val parts = h.split('.')
        if (parts.size != 4) return null
        return parts.map { part ->
            if (part.isEmpty() || part.length > 3 || !part.all { it in '0'..'9' }) return null
            part.toInt().takeIf { it <= 255 } ?: return null
        }
    }

    /** IPv6 アドレスの 8 つのグループ (`::` を展開)。読めなければ null。ゾーン ID や IPv4 埋め込みは扱わない。 */
    private fun parseIpv6(h: String): List<Int>? {
        if (':' !in h) return null
        val halves = h.split("::")
        if (halves.size > 2) return null
        fun groups(s: String): List<Int>? =
            if (s.isEmpty()) {
                emptyList()
            } else {
                s.split(':').map { g ->
                    if (g.isEmpty() || g.length > 4 || !g.all { it in '0'..'9' || it in 'a'..'f' }) return null
                    g.toInt(16)
                }
            }
        val head = groups(halves[0]) ?: return null
        if (halves.size == 1) return head.takeIf { it.size == 8 }
        val tail = groups(halves[1]) ?: return null
        val missing = 8 - head.size - tail.size
        if (missing < 1) return null
        return head + List(missing) { 0 } + tail
    }
}
