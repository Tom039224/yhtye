package io.github.tom039224.yhtye.shared.version

// Yhtye の版 (Semantic Versioning)。PC のデスクトップアプリも、この Android アプリも、同じ書き方の版を持つ。
// アプリは PC の版と自分の版の互換性を調べ ([compatibility])、GitHub の最新のリリースと自分の版を比べる
// ([io.github.tom039224.yhtye.shared.update.UpdateChecker])。

/**
 * `major.minor.patch` と、あれば `-pre.release` (プレリリースの印。ビルドのメタデータ `+…` は比べないので持たない)。
 * 並びは Semantic Versioning 2.0.0 の優先順位 ([compareTo]): 数の大きい方が新しく、プレリリースは同じ版の正式版より古い。
 */
data class AppVersion(
    val major: Int,
    val minor: Int,
    val patch: Int,
    /** `-` のあとの識別子 (`beta.1`)。正式版は null。 */
    val preRelease: String? = null,
) : Comparable<AppVersion> {
    init {
        require(major >= 0 && minor >= 0 && patch >= 0) { "version numbers cannot be negative" }
        require(preRelease == null || validPreRelease(preRelease)) { "bad pre-release: $preRelease" }
    }

    override fun compareTo(other: AppVersion): Int {
        compareValues(major, other.major).let { if (it != 0) return it }
        compareValues(minor, other.minor).let { if (it != 0) return it }
        compareValues(patch, other.patch).let { if (it != 0) return it }
        return comparePreRelease(preRelease, other.preRelease)
    }

    /** `0.1.0` / `0.2.0-beta.1`。 */
    override fun toString(): String = "$major.$minor.$patch" + (preRelease?.let { "-$it" } ?: "")

    companion object {
        private const val NUMBER = "0|[1-9][0-9]*"
        private const val IDENTIFIER = "[0-9A-Za-z-]+"
        private val IDENTIFIER_ONLY = Regex(IDENTIFIER)
        private val SEMVER = Regex("($NUMBER)\\.($NUMBER)\\.($NUMBER)(?:-($IDENTIFIER(?:\\.$IDENTIFIER)*))?(?:\\+$IDENTIFIER(?:\\.$IDENTIFIER)*)?")

        /** `0.1.0` や `1.2.3-beta.1+build` を読む (前後の空白は許す)。版として読めなければ null。先頭の `v` は付けない。 */
        fun parse(text: String): AppVersion? {
            val m = SEMVER.matchEntire(text.trim()) ?: return null
            val (major, minor, patch) = m.groupValues.drop(1).take(3).map { it.toIntOrNull() ?: return null }
            val pre = m.groupValues[4].ifEmpty { null }
            if (pre != null && !validPreRelease(pre)) return null
            return AppVersion(major, minor, patch, pre)
        }

        /** `.` で区切った識別子が文字・数・`-` だけで、数だけの識別子に先頭の 0 が無い (`01` は不可)。 */
        private fun validPreRelease(pre: String): Boolean = pre.split('.').all {
            IDENTIFIER_ONLY.matches(it) && (!it.all(Char::isDigit) || it == "0" || !it.startsWith('0'))
        }

        private fun comparePreRelease(a: String?, b: String?): Int {
            if (a == b) return 0
            // プレリリースは正式版より古い
            if (a == null) return 1
            if (b == null) return -1
            val left = a.split('.')
            val right = b.split('.')
            for (i in 0 until minOf(left.size, right.size)) {
                val c = compareIdentifier(left[i], right[i])
                if (c != 0) return c
            }
            // 共通の部分が同じなら、識別子の多い方が新しい
            return compareValues(left.size, right.size)
        }

        private fun compareIdentifier(a: String, b: String): Int {
            val an = a.all(Char::isDigit)
            val bn = b.all(Char::isDigit)
            return when {
                an && bn -> compareNumbers(a, b)
                // 数だけの識別子は文字を含む識別子より古い
                an -> -1
                bn -> 1
                else -> a.compareTo(b)
            }
        }

        /** 桁数が多くても溢れない数の比較 (先頭の 0 が無いので、桁数の次に文字列で比べればよい)。 */
        private fun compareNumbers(a: String, b: String): Int =
            if (a.length != b.length) a.length.compareTo(b.length) else a.compareTo(b)
    }
}
