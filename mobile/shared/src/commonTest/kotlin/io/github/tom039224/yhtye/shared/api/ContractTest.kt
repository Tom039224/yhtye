package io.github.tom039224.yhtye.shared.api

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.ClientKind
import io.github.tom039224.yhtye.shared.api.generated.Clients
import io.github.tom039224.yhtye.shared.api.generated.ContractSample
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.PairRequest
import io.github.tom039224.yhtye.shared.api.generated.StepKind
import io.github.tom039224.yhtye.shared.api.generated.StepSpec
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.api.generated.WsReply
import io.github.tom039224.yhtye.shared.api.generated.WsRequest
import io.github.tom039224.yhtye.shared.api.generated.WsServerMessage
import io.github.tom039224.yhtye.shared.api.generated.contractSamples
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertTrue
import kotlin.test.fail

/**
 * Rust が書いた JSON (ContractSamples.kt、Rust 側の samples.rs から生成) を生成された型で読み、
 * 書き戻して同じ JSON になることを確かめる。キーの順と数値の書き方 (1 と 1.0) 以外は完全一致。
 * Rust が省くフィールドを Kotlin が null で書くこと (またはその逆) も違いとして検出する。
 */
class ContractTest {
    @Test
    fun everySampleFromRustReadsAndWritesBackUnchanged() {
        assertTrue(contractSamples.size > 100, "samples: ${contractSamples.size}")
        val failures = contractSamples.mapNotNull { sample ->
            try {
                roundTrip(sample)
                null
            } catch (e: Throwable) {
                "${sample.type}/${sample.name}: ${e.message}"
            }
        }
        if (failures.isNotEmpty()) fail(failures.joinToString("\n"))
    }

    private fun roundTrip(sample: ContractSample) {
        @Suppress("UNCHECKED_CAST")
        val serializer = sample.serializer as KSerializer<Any?>
        val expected = ApiJson.parseToJsonElement(sample.json)
        val value = ApiJson.decodeFromString(serializer, sample.json)
        // 知らない variant になっていればエンコードが失敗する (UnrecognizedVariant)。
        val written = ApiJson.encodeToJsonElement(serializer, value)
        assertJsonEquals(expected, written, "$")
        // 書いたものを読み直しても同じ値。
        assertEquals(value, ApiJson.decodeFromJsonElement(serializer, written))
    }

    @Test
    fun aPongFromAPcBeforeTheVersionWasAddedStillReads() {
        // 古い PC は version を返さない: 読めて、版は空 (= 不明) になる
        assertEquals(
            ApiResponse.Pong(host = "old-pc", version = ""),
            ApiJson.decodeFromString(ApiResponse.Serializer, """{"type":"pong","host":"old-pc"}"""),
        )
        assertEquals(
            ApiResponse.Pong(host = "pc", version = "0.2.1"),
            ApiJson.decodeFromString(ApiResponse.Serializer, """{"type":"pong","host":"pc","version":"0.2.1"}"""),
        )
    }

    @Test
    fun commandsBuiltInKotlinAreWrittenLikeRust() {
        val request = WsRequest(id = 7, cmd = ApiCommand.ListEvents(project = "repo", afterSeq = 3))
        assertJsonEquals(
            ApiJson.parseToJsonElement(
                """{"id":7,"cmd":{"type":"list_events","project":"repo","after_seq":3,"limit":null}}""",
            ),
            ApiJson.encodeToJsonElement(WsRequest.serializer(), request),
            "$",
        )
        assertEquals(
            """{"type":"ping"}""",
            ApiJson.encodeToString(ApiCommand.Serializer, ApiCommand.Ping),
        )
        // Option は Rust と同じく null を書く。Rust が省くもの (skip_serializing_if) は null なら書かない。
        assertEquals(
            """{"type":"create_chat","project":"repo","branch":"b","worktree":null}""",
            ApiJson.encodeToString(ApiCommand.Serializer, ApiCommand.CreateChat(project = "repo", branch = "b")),
        )
        assertEquals(
            """{"kind":"implement"}""",
            ApiJson.encodeToString(StepSpec.serializer(), StepSpec(kind = StepKind.Implement)),
        )
        assertEquals(
            """{"code":"ABCD2345","device_name":"Pixel"}""",
            ApiJson.encodeToString(PairRequest.serializer(), PairRequest(code = "ABCD2345", deviceName = "Pixel")),
        )
    }

    @Test
    fun anUnknownResponseOrEventVariantIsKeptNotACrash() {
        val reply = ApiJson.decodeFromString(
            WsServerMessage.Serializer,
            """{"id":3,"ok":{"type":"brand_new","x":1}}""",
        )
        val ok = assertIs<WsReply.Ok>(assertIs<WsServerMessage.Reply>(reply).value)
        val unknown = assertIs<ApiResponse.Unrecognized>(ok.ok)
        assertEquals("brand_new", unknown.tag)
        assertEquals(JsonPrimitive(1), unknown.raw["x"])
        assertFailsWith<SerializationException> {
            ApiJson.encodeToString(ApiResponse.Serializer, unknown)
        }

        val event = ApiJson.decodeFromString(
            ApiEvent.serializer(),
            """{"seq":1,"ts_ms":2,"project":"p","live":true,"body":{"type":"brand_new"},"new_field":[]}""",
        )
        assertIs<ApiEventBody.Unrecognized>(event.body)
    }

    @Test
    fun anUnknownEnumValueOrShapeIsKeptNotACrash() {
        val body = ApiJson.decodeFromString(
            ApiEventBody.Serializer,
            """{"type":"domain","event":{"type":"task_status_changed","task":"T-1","status":"paused"}}""",
        )
        val domain = assertIs<ApiEventBody.Domain>(body)
        val changed = assertIs<DomainEvent.TaskStatusChanged>(domain.event)
        assertEquals(TaskStatus.Unrecognized, changed.status)
        assertFailsWith<SerializationException> {
            ApiJson.encodeToString(TaskStatus.Serializer, TaskStatus.Unrecognized)
        }

        val message = ApiJson.decodeFromString(WsServerMessage.Serializer, """{"hello":1}""")
        assertIs<WsServerMessage.Unrecognized>(message)
    }

    @Test
    fun aDesktopClientIsKnownAndADeviceKindOfANewerPcIsKept() {
        val devices = """
            {"server":{"host":"pc"},"devices":[
              {"id":"a","name":"Workstation","kind":"desktop","connected":true,"last_seen_ms":1},
              {"id":"b","name":"Fridge","kind":"appliance","connected":false,"last_seen_ms":null}]}
        """.trimIndent()
        val clients = ApiJson.decodeFromString(Clients.serializer(), devices)
        // 一覧は壊れず、知らない種類だけが Unrecognized になる (画面では「不明な端末」)
        assertEquals(listOf(ClientKind.Desktop, ClientKind.Unrecognized), clients.devices.map { it.kind })
        assertEquals(listOf("Workstation", "Fridge"), clients.devices.map { it.name })
    }

    @Test
    fun aPairRequestNamesTheKindOnlyWhenGiven() {
        val android = PairRequest(code = "ABCD2345", deviceName = "Pixel 9")
        assertEquals("""{"code":"ABCD2345","device_name":"Pixel 9"}""", ApiJson.encodeToString(PairRequest.serializer(), android))
        val desktop = PairRequest(code = "ABCD2345", deviceName = "PC", kind = ClientKind.Desktop)
        assertEquals(
            """{"code":"ABCD2345","device_name":"PC","kind":"desktop"}""",
            ApiJson.encodeToString(PairRequest.serializer(), desktop),
        )
    }

    @Test
    fun aMissingTagIsAnError() {
        assertFailsWith<SerializationException> {
            ApiJson.decodeFromString(ApiResponse.Serializer, """{"host":"pc"}""")
        }
    }
}

/** JSON の意味的な一致: オブジェクトのキー順と数値の表記 (1 / 1.0) は問わない。 */
private fun assertJsonEquals(expected: JsonElement, actual: JsonElement, path: String) {
    when {
        expected is JsonObject && actual is JsonObject -> {
            assertEquals(expected.keys, actual.keys, "$path: keys")
            for ((key, value) in expected) assertJsonEquals(value, actual.getValue(key), "$path.$key")
        }
        expected is JsonArray && actual is JsonArray -> {
            assertEquals(expected.size, actual.size, "$path: length")
            expected.indices.forEach { assertJsonEquals(expected[it], actual[it], "$path[$it]") }
        }
        expected is JsonNull || actual is JsonNull -> assertEquals(expected, actual, path)
        expected is JsonPrimitive && actual is JsonPrimitive -> {
            assertEquals(expected.isString, actual.isString, "$path: string or not")
            val same = expected.content == actual.content ||
                (!expected.isString && expected.content.toDoubleOrNull() != null &&
                    expected.content.toDoubleOrNull() == actual.content.toDoubleOrNull())
            assertTrue(same, "$path: expected ${expected.content}, got ${actual.content}")
        }
        else -> fail("$path: expected $expected, got $actual")
    }
}
