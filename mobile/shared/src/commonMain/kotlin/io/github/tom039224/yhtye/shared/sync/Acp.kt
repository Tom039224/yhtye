package io.github.tom039224.yhtye.shared.sync

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull

// ACP のペイロード (生成された型では JsonElement) から画面に出す部分だけを読む。PC の `src/api/acp.ts` の移植。
// どのフィールドも形を確かめてから読む (知らない形は null)。

private fun JsonElement?.field(key: String): JsonElement? = (this as? JsonObject)?.get(key)

private fun JsonElement?.str(): String? = (this as? JsonPrimitive)?.takeIf { it.isString }?.content

/** `ContentChunk` (`{content: {type: "text", text}}`) の本文。文字が無ければ null。 */
fun chunkText(update: JsonElement?): String? {
    val content = update.field("content")
    return if (content.field("type").str() == "text") content.field("text").str() else null
}

/** ツール呼び出し (`ToolCall` / `ToolCallUpdate`) の表示に使う部分。 */
data class ToolCallInfo(
    val id: String,
    val title: String?,
    val status: String?,
    val kind: String?,
    /** コードから呼んだ MCP のツール (`server.tool`)。無い・分からなければ空。 */
    val calls: List<String>,
)

/** `toolCallId` / `title` / `status` / `kind` / `rawInput.code`。`toolCallId` が無ければ null。 */
fun toolCallInfo(update: JsonElement?): ToolCallInfo? {
    val id = update.field("toolCallId").str() ?: return null
    return ToolCallInfo(
        id = id,
        title = update.field("title").str(),
        status = update.field("status").str(),
        kind = update.field("kind").str(),
        calls = codeToolCalls(update.field("rawInput").field("code").str()),
    )
}

private val CODE_CALL = Regex("""\btools\.([A-Za-z_$][\w$-]*)\.([A-Za-z_$][\w$]*)\s*\(""")

/**
 * コードモードのツール呼び出しが呼ぶ MCP のツール (OpenCode 2 は MCP のツールを `execute` の
 * `rawInput.code` (`await tools.yhtye.report_step_done({...})`) からしか呼ばない)。重複を除いて出てきた順。
 */
fun codeToolCalls(code: String?): List<String> {
    if (code.isNullOrEmpty()) return emptyList()
    return CODE_CALL.findAll(code).map { "${it.groupValues[1]}.${it.groupValues[2]}" }.distinct().toList()
}

/** セッションの文脈の使用量 (ACP の `usage_update`。トークン数)。 */
data class ContextUsage(
    val used: Double,
    val size: Double,
    /** エージェントが知らせていればセッションの累計の費用。 */
    val cost: Cost?,
) {
    data class Cost(val amount: Double, val currency: String)
}

private fun JsonElement?.count(): Double? =
    (this as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull?.takeIf { it.isFinite() && it >= 0 }

/** `UsageUpdate` (`used`, `size`, 任意の `cost: {amount, currency}`)。形が違えば null。 */
fun usageInfo(update: JsonElement?): ContextUsage? {
    val used = update.field("used").count() ?: return null
    val size = update.field("size").count() ?: return null
    val cost = update.field("cost")
    val amount = (cost.field("amount") as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull
    val currency = cost.field("currency").str()
    val known = if (amount != null && amount.isFinite() && !currency.isNullOrEmpty()) ContextUsage.Cost(amount, currency) else null
    return ContextUsage(used, size, known)
}
