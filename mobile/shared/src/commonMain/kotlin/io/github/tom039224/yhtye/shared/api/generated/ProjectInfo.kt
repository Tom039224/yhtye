// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A project known to Yhtye.
 */
@Serializable
data class ProjectInfo(
    val id: String,
    /**
     * Display name (the directory name).
     */
    val name: String,
    val path: String,
    /**
     * Its orchestration is running in this process.
     */
    val open: Boolean,
    /**
     * Work is on in it: a live agent session is in a turn, or a task is
     * running or merging (what the sidebar's turning ring shows). Always
     * `false` while it is not open.
     */
    val running: Boolean,
)
