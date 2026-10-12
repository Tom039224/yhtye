package io.github.tom039224.yhtye.shared.version

/** PC の Yhtye とこの端末のアプリの版の組み合わせの判定。 */
sealed interface Compatibility {
    /** 使える組み合わせ。 */
    data object Compatible : Compatibility

    /** 合わない組み合わせ。どちらかを更新する。 */
    data class Incompatible(val pc: AppVersion, val device: AppVersion) : Compatibility

    /** PC の版が分からない (この項目を返す前の古い PC、読めない版)。警告は出さない。 */
    data object Unknown : Compatibility
}

/**
 * [pc] (PC の `ping` が返した版。無い・読めないときは null か空) と [device] (この端末のアプリの版) の互換性。
 *
 * - 0.x の間は、`0.<minor>` が同じなら互換 (0.x ではマイナーで互換性が壊れ得る)。
 * - 1.0 以降は厳密な Semantic Versioning: メジャーが同じで、PC のマイナーがこの端末のマイナー以上なら互換
 *   (PC が新しい API を足していても、古いアプリは使える)。
 * - パッチは PC と Android で独立して上げてよいので見ない。プレリリースの印も見ない。
 * - メジャーが違えば互換でない (0 と 1 も違う)。
 */
fun compatibility(pc: String?, device: AppVersion): Compatibility {
    val pcVersion = pc?.let(AppVersion::parse) ?: return Compatibility.Unknown
    val compatible = when {
        pcVersion.major != device.major -> false
        device.major == 0 -> pcVersion.minor == device.minor
        else -> pcVersion.minor >= device.minor
    }
    return if (compatible) Compatibility.Compatible else Compatibility.Incompatible(pcVersion, device)
}
