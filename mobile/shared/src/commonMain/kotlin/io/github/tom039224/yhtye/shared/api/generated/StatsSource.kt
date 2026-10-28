// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A harness's logs: whether they exist on this machine.
 */
@Serializable
data class StatsSource(
    val harness: String,
    /**
     * The logs (directory or database) exist.
     */
    val found: Boolean,
    /**
     * Log files read (1 for OpenCode's database); files older than the period are not read.
     */
    val files: Long,
)
