package io.github.tom039224.yhtye.shared.version

import kotlin.test.Test
import kotlin.test.assertEquals

class CompatibilityTest {
    private fun device(text: String) = AppVersion.parse(text) ?: error(text)

    private fun check(pc: String?, device: String) = compatibility(pc, device(device))

    private fun incompatible(pc: String, device: String) =
        Compatibility.Incompatible(device(pc), device(device))

    // ---- 0.x: 0.<minor> が同じなら互換 ----

    @Test
    fun beforeOnePointZeroTheSameMinorIsCompatible() {
        assertEquals(Compatibility.Compatible, check("0.1.0", "0.1.0"))
        assertEquals(Compatibility.Compatible, check("0.2.5", "0.2.0"))
    }

    @Test
    fun beforeOnePointZeroADifferentMinorIsNotCompatibleInEitherDirection() {
        assertEquals(incompatible("0.2.0", "0.1.0"), check("0.2.0", "0.1.0"))
        assertEquals(incompatible("0.1.0", "0.2.0"), check("0.1.0", "0.2.0"))
    }

    // ---- 1.0 以降: メジャーが同じで、PC のマイナー ≥ スマホのマイナー ----

    @Test
    fun fromOnePointZeroAPcWithTheSameOrANewerMinorIsCompatible() {
        assertEquals(Compatibility.Compatible, check("1.2.0", "1.2.0"))
        assertEquals(Compatibility.Compatible, check("1.3.0", "1.2.7"))
        assertEquals(Compatibility.Compatible, check("2.9.1", "2.0.0"))
    }

    @Test
    fun fromOnePointZeroAPcWithAnOlderMinorIsNotCompatible() {
        assertEquals(incompatible("1.1.9", "1.2.0"), check("1.1.9", "1.2.0"))
    }

    @Test
    fun aDifferentMajorIsNeverCompatible() {
        assertEquals(incompatible("2.0.0", "1.9.0"), check("2.0.0", "1.9.0"))
        assertEquals(incompatible("1.0.0", "2.0.0"), check("1.0.0", "2.0.0"))
        // 0.x と 1.x も別
        assertEquals(incompatible("1.0.0", "0.1.0"), check("1.0.0", "0.1.0"))
        assertEquals(incompatible("0.1.0", "1.0.0"), check("0.1.0", "1.0.0"))
    }

    // ---- パッチ・プレリリース・不明 ----

    @Test
    fun thePatchIsIndependentOnEachSide() {
        assertEquals(Compatibility.Compatible, check("0.1.7", "0.1.2"))
        assertEquals(Compatibility.Compatible, check("0.1.0", "0.1.9"))
        assertEquals(Compatibility.Compatible, check("1.2.0", "1.2.9"))
    }

    @Test
    fun aPreReleaseMarkDoesNotChangeTheAnswer() {
        assertEquals(Compatibility.Compatible, check("0.1.0-beta.1", "0.1.0"))
        assertEquals(incompatible("0.2.0-rc.1", "0.1.0"), check("0.2.0-rc.1", "0.1.0"))
    }

    @Test
    fun aPcWithoutAVersionIsUnknownNotIncompatible() {
        assertEquals(Compatibility.Unknown, check(null, "0.1.0"))
        assertEquals(Compatibility.Unknown, check("", "0.1.0"))
        assertEquals(Compatibility.Unknown, check("  ", "0.1.0"))
        assertEquals(Compatibility.Unknown, check("not a version", "0.1.0"))
    }
}
