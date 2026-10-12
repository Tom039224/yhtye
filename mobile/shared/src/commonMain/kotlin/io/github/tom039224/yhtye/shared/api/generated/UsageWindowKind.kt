// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = UsageWindowKind.Serializer::class)
enum class UsageWindowKind(override val wire: String) : WireEnum {
    /**
     * The rolling 5-hour limit.
     */
    FiveHour("five_hour"),
    /**
     * The weekly limit across all models.
     */
    Week("week"),
    /**
     * Any other window (per-model weekly limits).
     */
    Other("other"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<UsageWindowKind>("UsageWindowKind", UsageWindowKind.entries, Unrecognized)
}
