// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * What a paired device is: the Android client, or another Yhtye desktop app
 * that uses this PC as its host. The device says which it is when it pairs
 * (the `kind` of the pairing request; the Android client leaves it out).
 */
@Serializable(with = ClientKind.Serializer::class)
enum class ClientKind(override val wire: String) : WireEnum {
    Android("android"),
    Desktop("desktop"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<ClientKind>("ClientKind", ClientKind.entries, Unrecognized)
}
