// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable

/**
 * One reason to wake the orchestrator.
 */
@Serializable
data class InboxItem(
    val kind: InboxKind,
    val attrs: List<List<String>>,
    val body: String,
    /**
     * Files the user attached to a `user_message` (their content is not part of
     * the event: it is read from disk when the prompt is sent). Their
     * `delivery` is the prediction made when they were staged; the final one is
     * in the `prompted` event.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val attachments: List<AttachmentMeta> = emptyList(),
)
