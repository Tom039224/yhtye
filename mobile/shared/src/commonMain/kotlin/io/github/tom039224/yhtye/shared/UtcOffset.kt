package io.github.tom039224.yhtye.shared

/**
 * この端末のタイムゾーンの UTC からのずれ (分。東が正: 日本は 540) を、エポックからのミリ秒 [atEpochMs] の時点で。
 * 夏時間があるので時刻を取る。利用統計の「1 日」の区切り (`get_usage_stats` の `tz_offset_min`) に使う。
 */
expect fun deviceUtcOffsetMin(atEpochMs: Long): Int
