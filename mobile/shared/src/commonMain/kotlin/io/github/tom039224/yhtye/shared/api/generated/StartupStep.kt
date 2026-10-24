// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * How long one startup step took: the request the client sent to open the
 * session (`initialize`, `session/new` or `session/load`, `session/set_mode`,
 * `session/set_config_option`).
 */
@Serializable
data class StartupStep(
    val step: String,
    /**
     * Milliseconds from sending the request to its answer, its error or its
     * timeout (a step that timed out took the whole timeout).
     */
    val millis: Long,
)
