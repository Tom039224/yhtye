// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What the user chose for a harness besides where its executable is: the
 * adapter version and, for Claude Code, whose `claude` runs. No stored row
 * means [`HarnessOptions::default`].
 */
@Serializable
data class HarnessOptions(
    val adapter: AdapterChannel,
    /**
     * The version of [`AdapterChannel::Pinned`] (semver, `1.2.3` or
     * `1.2.3-beta.1`); `None` for the other channels.
     */
    @SerialName("adapter_version")
    val adapterVersion: String? = null,
    /**
     * Claude Code only; `User` for the other harnesses.
     */
    @SerialName("claude_source")
    val claudeSource: ClaudeSource,
)
