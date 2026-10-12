package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.font.FontWeight
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MarkdownModelTest {
    private val colors = MdColors(strong = Color.Red, code = Color.Green, codeBg = Color.Gray, link = Color.Blue)

    private fun parse(text: String) = parseMarkdown(text, colors)

    @Test
    fun `headings lists and paragraphs`() {
        val blocks = parse("## 現状\nバリデーションが **4 か所に重複** しています。\n\n1. 共通の検証モジュール\n2. `src/validation/` を作る\n\n- a\n- b")
        val heading = assertIs<MdBlock.Heading>(blocks[0])
        assertEquals(2, heading.level)
        assertEquals("現状", heading.text.text)
        val p = assertIs<MdBlock.Paragraph>(blocks[1])
        assertEquals("バリデーションが 4 か所に重複 しています。", p.text.text)
        val strong = p.text.spanStyles.single { it.item.fontWeight == FontWeight.SemiBold }
        assertEquals("4 か所に重複", p.text.text.substring(strong.start, strong.end))
        val ordered = assertIs<MdBlock.ListBlock>(blocks[2])
        assertEquals(listOf("1.", "2."), ordered.markers)
        assertEquals(" src/validation/  を作る", (ordered.items[1].single() as MdBlock.Paragraph).text.text)
        val bullets = assertIs<MdBlock.ListBlock>(blocks[3])
        assertEquals(listOf("•", "•"), bullets.markers)
    }

    @Test
    fun `a soft line break stays a line break like the PC's pre-wrap`() {
        val p = assertIs<MdBlock.Paragraph>(parse("one\ntwo").single())
        assertEquals("one\ntwo", p.text.text)
    }

    @Test
    fun `fenced code keeps its text and language, an unclosed fence is still code`() {
        val code = assertIs<MdBlock.Code>(parse("```ts\nconst a = 1\n  b()\n```").single())
        assertEquals("ts", code.language)
        assertEquals("const a = 1\n  b()", code.code)
        val open = assertIs<MdBlock.Code>(parse("~~~\npartial line").single())
        assertNull(open.language)
        assertEquals("partial line", open.code)
    }

    @Test
    fun `tables keep their header and rows`() {
        val table = assertIs<MdBlock.Table>(parse("| 場所 | 対象 |\n|---|---|\n| `a.ts` | 住所 |\n| b | カード |").single())
        assertEquals(listOf("場所", "対象"), table.header.map { it.text })
        assertEquals(listOf(listOf("a.ts", "住所"), listOf("b", "カード")), table.rows.map { r -> r.map { it.text } })
    }

    @Test
    fun `only http and mailto links are kept, html and images are not rendered`() {
        val p = assertIs<MdBlock.Paragraph>(parse("[ok](https://example.com) [bad](javascript:alert(1)) <b>x</b> ![pic](https://e/i.png)").single())
        assertTrue(p.text.text.startsWith("ok bad"))
        val links = p.text.getLinkAnnotations(0, p.text.length)
        assertEquals(listOf("https://example.com"), links.map { (it.item as LinkAnnotation.Url).url })
        assertTrue("<b>" !in p.text.text)
        assertTrue("[pic]" in p.text.text)
    }

    @Test
    fun `raw html blocks are dropped`() {
        assertEquals(emptyList(), parse("<div>\nhidden\n</div>"))
    }

    @Test
    fun `safeUrl accepts only absolute http(s) and mailto`() {
        assertEquals("https://a.b", safeUrl(" https://a.b "))
        assertEquals("mailto:x@y", safeUrl("mailto:x@y"))
        assertNull(safeUrl("file:///etc/passwd"))
        assertNull(safeUrl("/relative"))
    }
}
