package io.github.tom039224.yhtye.shared

import java.util.TimeZone

actual fun deviceUtcOffsetMin(atEpochMs: Long): Int = TimeZone.getDefault().getOffset(atEpochMs) / MS_PER_MINUTE

private const val MS_PER_MINUTE = 60_000
