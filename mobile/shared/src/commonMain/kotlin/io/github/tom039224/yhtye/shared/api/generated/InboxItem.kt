// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One reason to wake the orchestrator.
 */
@Serializable
data class InboxItem(
    val kind: InboxKind,
    val attrs: List<List<String>>,
    val body: String,
)
