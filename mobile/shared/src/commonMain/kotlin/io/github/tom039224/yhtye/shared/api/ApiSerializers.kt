package io.github.tom039224.yhtye.shared.api

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.SerializationStrategy
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonEncoder
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

// 生成された API 型 (api/generated) が使う直列化の部品。手書き。
// どれも JSON 専用 (ApiJson で使う)。

/**
 * このクライアントが知らない variant (PC 側の Yhtye が新しい)。受け取った JSON をそのまま持つ。
 * 知らないものを PC に送り返さないよう、エンコードは失敗する。
 */
interface UnrecognizedVariant {
    val raw: JsonElement
}

/** 文字列 enum の値と JSON 上の名前。 */
interface WireEnum {
    val wire: String
}

/** タグ付き enum の 1 variant: タグの値、その本体 (タグ以外のキー) の serializer、判定。 */
class TaggedVariant<T : Any>(
    val tag: String,
    val serializer: KSerializer<out T>,
    val matches: (T) -> Boolean,
)

/**
 * serde の内部タグ・隣接タグ付き enum (`{"type": "ping", ...}`)。
 * 知らないタグは [unrecognized] で受ける (クラッシュしない)。
 */
abstract class TaggedSerializer<T : Any>(
    private val serialName: String,
    private val tag: String,
    private val variants: List<TaggedVariant<T>>,
    private val unrecognized: (String, JsonObject) -> T,
) : KSerializer<T> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor(serialName)
    private val byTag = variants.associateBy { it.tag }

    override fun serialize(encoder: Encoder, value: T) {
        val out = encoder.asJson(serialName)
        refuseUnrecognized(serialName, value)
        val variant = variants.firstOrNull { it.matches(value) }
            ?: throw SerializationException("$serialName: no variant for $value")
        val body = out.json.encodeToJsonElement(variant.serializer.forValue(), value)
        val fields = body as? JsonObject
            ?: throw SerializationException("$serialName.${variant.tag}: the body is not an object")
        out.encodeJsonElement(JsonObject(mapOf(tag to JsonPrimitive(variant.tag)) + fields))
    }

    override fun deserialize(decoder: Decoder): T {
        val input = decoder.asJson(serialName)
        val obj = input.decodeJsonElement() as? JsonObject
            ?: throw SerializationException("$serialName: expected an object")
        val value = obj[tag] as? JsonPrimitive
        if (value == null || !value.isString) {
            throw SerializationException("$serialName: no string `$tag`")
        }
        val variant = byTag[value.content] ?: return unrecognized(value.content, obj)
        return input.json.decodeFromJsonElement(variant.serializer, JsonObject(obj - tag))
    }
}

/** タグ無し enum の 1 variant: これらのキーの組のどれかがそろっていればこの variant。 */
class UntaggedVariant<T : Any>(
    val keySets: List<Set<String>>,
    val serializer: KSerializer<out T>,
    val matches: (T) -> Boolean,
)

/**
 * serde のタグ無し・外部タグ付き enum (`{"id": 1, "ok": ...}` / `{"Ok": ...}`)。
 * あるキーで variant を選ぶ (先に並んだものが優先、serde と同じ)。どれでもなければ [unrecognized]。
 */
abstract class UntaggedSerializer<T : Any>(
    private val serialName: String,
    private val variants: List<UntaggedVariant<T>>,
    private val unrecognized: (JsonElement) -> T,
) : KSerializer<T> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor(serialName)

    override fun serialize(encoder: Encoder, value: T) {
        val out = encoder.asJson(serialName)
        refuseUnrecognized(serialName, value)
        val variant = variants.firstOrNull { it.matches(value) }
            ?: throw SerializationException("$serialName: no variant for $value")
        out.encodeSerializableValue(variant.serializer.forValue(), value)
    }

    override fun deserialize(decoder: Decoder): T {
        val input = decoder.asJson(serialName)
        val element = input.decodeJsonElement()
        val obj = element as? JsonObject ?: return unrecognized(element)
        val variant = variants.firstOrNull { v -> v.keySets.any { keys -> obj.keys.containsAll(keys) } }
            ?: return unrecognized(element)
        return input.json.decodeFromJsonElement(variant.serializer, obj)
    }
}

/** 文字列 enum。知らない値は [unrecognized] にする (クラッシュしない)。 */
abstract class WireEnumSerializer<E>(
    private val serialName: String,
    entries: List<E>,
    private val unrecognized: E,
) : KSerializer<E> where E : Enum<E>, E : WireEnum {
    override val descriptor: SerialDescriptor = PrimitiveSerialDescriptor(serialName, PrimitiveKind.STRING)
    private val byWire = entries.filter { it != unrecognized }.associateBy { it.wire }

    override fun serialize(encoder: Encoder, value: E) {
        if (value == unrecognized) {
            throw SerializationException("$serialName: an unrecognized value cannot be sent")
        }
        encoder.encodeString(value.wire)
    }

    override fun deserialize(decoder: Decoder): E = byWire[decoder.decodeString()] ?: unrecognized
}

private fun Encoder.asJson(serialName: String): JsonEncoder =
    this as? JsonEncoder ?: throw SerializationException("$serialName can only be written as JSON")

private fun Decoder.asJson(serialName: String): JsonDecoder =
    this as? JsonDecoder ?: throw SerializationException("$serialName can only be read from JSON")

private fun refuseUnrecognized(serialName: String, value: Any) {
    if (value is UnrecognizedVariant) {
        throw SerializationException("$serialName: an unrecognized variant cannot be sent")
    }
}

/** variant の serializer を、その親の型の値に使う (variant は [TaggedVariant.matches] で確かめ済み)。 */
@Suppress("UNCHECKED_CAST")
private fun <T> KSerializer<out T>.forValue(): SerializationStrategy<T> = this as SerializationStrategy<T>
