// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A conversation with the orchestrator, bound to one worktree
 * (`orchestration-model.md` §2.0, Stage 8e). The branch is not part of the
 * chat: it is read from the worktree when shown, and when a group is created
 * or merged. Times are not part of the state (the store derives them from
 * the event log).
 */
@Serializable
data class Chat(
    /**
     * `C-<n>`.
     */
    val id: String,
    /**
     * The worktree the chat works in, as `git worktree list` shows it: its
     * orchestrator's working directory and where its groups are merged. It
     * never changes.
     */
    val worktree: String,
    /**
     * The first user message, shortened (`None` until it is sent).
     */
    val title: String? = null,
)
