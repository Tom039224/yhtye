package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import io.github.tom039224.yhtye.MonoFont
import org.intellij.markdown.IElementType
import org.intellij.markdown.MarkdownElementTypes
import org.intellij.markdown.MarkdownTokenTypes
import org.intellij.markdown.ast.ASTNode
import org.intellij.markdown.ast.getTextInNode
import org.intellij.markdown.flavours.gfm.GFMElementTypes
import org.intellij.markdown.flavours.gfm.GFMFlavourDescriptor
import org.intellij.markdown.flavours.gfm.GFMTokenTypes
import org.intellij.markdown.parser.CancellationToken
import org.intellij.markdown.parser.MarkdownParser

// エージェントの文 (オーケストレータの返事) を Markdown として読む。JetBrains の構文解析器 (GFM) の木を、描画しやすい
// ブロックの並びにする。安全 (PC の Markdown.tsx と同じ。文は信用できないリポジトリを読む LLM が書く):
// - 生の HTML は描画しない (タグは捨てる)。
// - リンクは http(s): / mailto: だけ残す (外のブラウザで開く)。それ以外は文字だけ。
// - 画像は読み込まない (代わりの文字を出す)。
// ストリーミング中は届くたびに読み直す。閉じていないコードフェンスは閉じるまで開いたコードブロックになる。

/** 描画するブロック。 */
sealed interface MdBlock {
    data class Paragraph(val text: AnnotatedString) : MdBlock

    data class Heading(val level: Int, val text: AnnotatedString) : MdBlock

    /** [items] は項目ごとのブロック。[markers] は項目の印 (`•` か `1.`)。 */
    data class ListBlock(val markers: List<String>, val items: List<List<MdBlock>>) : MdBlock

    data class Code(val language: String?, val code: String) : MdBlock

    data class Table(val header: List<AnnotatedString>, val rows: List<List<AnnotatedString>>) : MdBlock

    data class Quote(val blocks: List<MdBlock>) : MdBlock

    data object Rule : MdBlock
}

/** 文中の装飾の色。 */
data class MdColors(val strong: Color, val code: Color, val codeBg: Color, val link: Color)

private val SAFE_URL = Regex("^(https?://|mailto:)", RegexOption.IGNORE_CASE)

/** 絶対の http(s) / mailto の URL だけ残す (それ以外は null)。 */
fun safeUrl(url: String): String? = url.trim().takeIf { SAFE_URL.containsMatchIn(it) }

private val ESCAPE = Regex("""\\([!-/:-@\[-`{-~])""")
private const val BULLET = "•"
private const val CODE_INDENT = 4

fun parseMarkdown(text: String, colors: MdColors): List<MdBlock> {
    val tree = MarkdownParser(GFMFlavourDescriptor(), assertionsEnabled = false, cancellationToken = CancellationToken.NonCancellable).buildMarkdownTreeFromString(text as CharSequence)
    return MdReader(text, colors).blocks(tree.children)
}

private class MdReader(private val src: String, private val colors: MdColors) {
    private fun ASTNode.text(): String = getTextInNode(src).toString()

    fun blocks(nodes: List<ASTNode>): List<MdBlock> = nodes.mapNotNull(::block)

    private fun block(node: ASTNode): MdBlock? = when (node.type) {
        MarkdownElementTypes.PARAGRAPH -> MdBlock.Paragraph(inline(node.children)).takeIf { it.text.isNotBlank() }
        MarkdownElementTypes.ATX_1, MarkdownElementTypes.SETEXT_1 -> heading(1, node)
        MarkdownElementTypes.ATX_2, MarkdownElementTypes.SETEXT_2 -> heading(2, node)
        MarkdownElementTypes.ATX_3, MarkdownElementTypes.ATX_4, MarkdownElementTypes.ATX_5, MarkdownElementTypes.ATX_6 ->
            heading(3, node)
        MarkdownElementTypes.UNORDERED_LIST, MarkdownElementTypes.ORDERED_LIST -> list(node)
        MarkdownElementTypes.CODE_FENCE -> fence(node)
        MarkdownElementTypes.CODE_BLOCK -> indentedCode(node)
        MarkdownElementTypes.BLOCK_QUOTE -> MdBlock.Quote(blocks(node.children))
        GFMElementTypes.TABLE -> table(node)
        MarkdownTokenTypes.HORIZONTAL_RULE -> MdBlock.Rule
        // 生の HTML・リンクの定義・空白は出さない
        MarkdownElementTypes.HTML_BLOCK, MarkdownElementTypes.LINK_DEFINITION -> null
        else -> if (node.children.isEmpty()) null else MdBlock.Paragraph(inline(node.children)).takeIf { it.text.isNotBlank() }
    }

    private fun heading(level: Int, node: ASTNode): MdBlock.Heading {
        val content = node.children.firstOrNull {
            it.type == MarkdownTokenTypes.ATX_CONTENT || it.type == MarkdownTokenTypes.SETEXT_CONTENT
        }
        val text = content?.let { inline(it.children.ifEmpty { listOf(it) }) } ?: AnnotatedString("")
        return MdBlock.Heading(level, trim(text))
    }

    private fun list(node: ASTNode): MdBlock.ListBlock {
        val ordered = node.type == MarkdownElementTypes.ORDERED_LIST
        val items = node.children.filter { it.type == MarkdownElementTypes.LIST_ITEM }
        val markers = items.map { item ->
            if (!ordered) {
                BULLET
            } else {
                item.children.firstOrNull { it.type == MarkdownTokenTypes.LIST_NUMBER }?.text()?.trim() ?: "${items.indexOf(item) + 1}."
            }
        }
        return MdBlock.ListBlock(markers, items.map { blocks(it.children) })
    }

    private fun fence(node: ASTNode): MdBlock.Code {
        val language = node.children.firstOrNull { it.type == MarkdownTokenTypes.FENCE_LANG }?.text()?.trim()?.ifEmpty { null }
        val body = node.children
            .dropWhile { it.type != MarkdownTokenTypes.EOL }
            .drop(1)
            .takeWhile { it.type != MarkdownTokenTypes.CODE_FENCE_END }
            .filter { it.type == MarkdownTokenTypes.CODE_FENCE_CONTENT || it.type == MarkdownTokenTypes.EOL }
            .joinToString("") { it.text() }
        return MdBlock.Code(language, body.trimEnd('\n'))
    }

    private fun indentedCode(node: ASTNode): MdBlock.Code {
        val lines = node.text().lines().map { line ->
            val indent = line.takeWhile { it == ' ' }.length.coerceAtMost(CODE_INDENT)
            line.drop(indent)
        }
        return MdBlock.Code(null, lines.joinToString("\n").trimEnd('\n'))
    }

    private fun table(node: ASTNode): MdBlock.Table {
        fun cells(row: ASTNode) = row.children.filter { it.type == GFMTokenTypes.CELL }.map { trim(inline(it.children)) }
        val header = node.children.firstOrNull { it.type == GFMElementTypes.HEADER }?.let(::cells).orEmpty()
        val rows = node.children.filter { it.type == GFMElementTypes.ROW }.map(::cells)
        return MdBlock.Table(header, rows)
    }

    // ---- 文中 -----------------------------------------------------------------------------

    fun inline(nodes: List<ASTNode>): AnnotatedString = buildAnnotatedString { nodes.forEach { appendInline(it) } }

    private fun AnnotatedString.Builder.appendInline(node: ASTNode) {
        when (node.type) {
            MarkdownTokenTypes.EOL, MarkdownTokenTypes.HARD_LINE_BREAK -> append('\n')
            MarkdownElementTypes.STRONG ->
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = colors.strong)) { inner(node, MarkdownTokenTypes.EMPH) }
            MarkdownElementTypes.EMPH -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { inner(node, MarkdownTokenTypes.EMPH) }
            GFMElementTypes.STRIKETHROUGH ->
                withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) { inner(node, GFMTokenTypes.TILDE) }
            MarkdownElementTypes.CODE_SPAN -> codeSpan(node)
            MarkdownElementTypes.INLINE_LINK -> link(node, node.children.firstOrNull { it.type == MarkdownElementTypes.LINK_DESTINATION }?.text())
            MarkdownElementTypes.FULL_REFERENCE_LINK, MarkdownElementTypes.SHORT_REFERENCE_LINK -> link(node, null)
            MarkdownElementTypes.AUTOLINK -> {
                val url = node.children.firstOrNull { it.type == MarkdownTokenTypes.AUTOLINK }?.text() ?: node.text().trim('<', '>')
                linkText(url, url)
            }
            GFMTokenTypes.GFM_AUTOLINK -> linkText(node.text(), node.text())
            MarkdownElementTypes.IMAGE -> {
                val alt = node.children.firstOrNull { it.type == MarkdownElementTypes.INLINE_LINK }
                    ?.children?.firstOrNull { it.type == MarkdownElementTypes.LINK_TEXT }
                    ?.let { linkLabel(it) }
                append("[${alt?.ifBlank { null } ?: "image"}]")
            }
            MarkdownTokenTypes.HTML_TAG -> Unit
            else -> if (node.children.isEmpty()) append(unescape(node.text())) else node.children.forEach { appendInline(it) }
        }
    }

    /** 区切り ([delimiter]) を除いた中身。 */
    private fun AnnotatedString.Builder.inner(node: ASTNode, delimiter: IElementType) {
        val children = node.children
        val first = children.indexOfFirst { it.type != delimiter }.takeIf { it >= 0 } ?: return
        val last = children.indexOfLast { it.type != delimiter }
        children.subList(first, last + 1).forEach { appendInline(it) }
    }

    private fun AnnotatedString.Builder.codeSpan(node: ASTNode) {
        val code = node.children.filter { it.type != MarkdownTokenTypes.BACKTICK }.joinToString("") { it.text() }
        withStyle(SpanStyle(fontFamily = MonoFont, color = colors.code, background = colors.codeBg)) {
            append(" ${code.trim()} ".replace('\n', ' '))
        }
    }

    private fun linkLabel(textNode: ASTNode): String {
        val inner = textNode.children.filter { it.type != MarkdownTokenTypes.LBRACKET && it.type != MarkdownTokenTypes.RBRACKET }
        return inline(inner).text
    }

    private fun AnnotatedString.Builder.link(node: ASTNode, destination: String?) {
        val label = node.children.firstOrNull { it.type == MarkdownElementTypes.LINK_TEXT }
        val inner = label?.children?.filter { it.type != MarkdownTokenTypes.LBRACKET && it.type != MarkdownTokenTypes.RBRACKET }
        val url = destination?.trim()?.removePrefix("<")?.removeSuffix(">")?.let(::safeUrl)
        if (url == null) {
            inner?.forEach { appendInline(it) }
            return
        }
        withLink(LinkAnnotation.Url(url, TextLinkStyles(SpanStyle(color = colors.link, textDecoration = TextDecoration.Underline)))) {
            inner.orEmpty().forEach { appendInline(it) }
        }
    }

    private fun AnnotatedString.Builder.linkText(label: String, url: String) {
        val safe = safeUrl(url)
        if (safe == null) {
            append(label)
            return
        }
        withLink(LinkAnnotation.Url(safe, TextLinkStyles(SpanStyle(color = colors.link, textDecoration = TextDecoration.Underline)))) {
            append(label)
        }
    }

    private fun unescape(text: String): String = ESCAPE.replace(text) { it.groupValues[1] }

    /** 前後の空白を除く (見出し・表のセルは区切りの後の空白から始まる)。 */
    private fun trim(text: AnnotatedString): AnnotatedString {
        val start = text.indexOfFirst { !it.isWhitespace() }
        if (start < 0) return AnnotatedString("")
        val end = text.indexOfLast { !it.isWhitespace() }
        return text.subSequence(start, end + 1)
    }
}
