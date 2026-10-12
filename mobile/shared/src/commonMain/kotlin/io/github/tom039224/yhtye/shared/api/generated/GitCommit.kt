// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class GitCommit(
    val sha: String,
    /**
     * First parent first (the branch the commit was made on).
     */
    val parents: List<String>,
    /**
     * Local branches pointing here.
     */
    val branches: List<String>,
    val subject: String,
    /**
     * Committer time.
     */
    @SerialName("ts_ms")
    val tsMs: Long,
)
