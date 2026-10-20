// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * What an attachment is, as far as delivery is concerned.
 */
@Serializable(with = AttachmentKind.Serializer::class)
enum class AttachmentKind(override val wire: String) : WireEnum {
    /**
     * PNG, JPEG, GIF or WebP (by its content): an agent may be sent it as an image.
     */
    Image("image"),
    /**
     * UTF-8 text with a text MIME type or a known text extension.
     */
    Text("text"),
    /**
     * Anything else (PDF, archives, other images, binaries).
     */
    File("file"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<AttachmentKind>("AttachmentKind", AttachmentKind.entries, Unrecognized)
}
