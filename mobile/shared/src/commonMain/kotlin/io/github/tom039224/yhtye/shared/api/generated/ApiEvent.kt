// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * One event of a project.
 *
 * Durable events (`live == false`) are stored in the event log and numbered
 * without gaps from 1; `seq` is their row in the log. Live events (streaming
 * text chunks and usage updates, see [`ApiEventBody::is_live`]) are not stored:
 * they carry the `seq` of the last durable event and never advance it. A text
 * block streamed as live chunks is stored once, coalesced, as
 * [`ApiEventBody::AgentText`] when the block ends.
 *
 * Sync rule for clients: take a [`Snapshot`], then apply only durable events with
 * `seq > snapshot.seq`; on a gap between durable events, fetch the missing ones
 * with [`ApiCommand::ListEvents`] (or take a new snapshot). Live events are for
 * display only and never folded into state.
 *
 * App-wide events ([`ApiEventBody::is_local_only`]: remote access) have an
 * empty `project`, `seq` 0 and are live.
 */
@Serializable
data class ApiEvent(
    val seq: Long,
    /**
     * Milliseconds since the Unix epoch.
     */
    @SerialName("ts_ms")
    val tsMs: Long,
    val project: String,
    val live: Boolean,
    val body: ApiEventBody,
)
