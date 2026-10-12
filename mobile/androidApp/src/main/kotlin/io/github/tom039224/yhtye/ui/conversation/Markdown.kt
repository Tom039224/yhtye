package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye

private val BLOCK_GAP = 10.dp
private val CODE_SHAPE = RoundedCornerShape(10.dp)
private val LIST_MARKER_WIDTH = 22.dp

/** エージェントの文を Markdown として描く (見出し・リスト・表・コードブロック・強調・インラインコード・リンク)。 */
@Composable
fun Markdown(text: String, modifier: Modifier = Modifier) {
    val c = Yhtye.colors
    val colors = MdColors(strong = c.accentStrong, code = c.tx, codeBg = c.bgChip, link = c.accent)
    val blocks = remember(text, colors) { parseMarkdown(text, colors) }
    MdBlocks(blocks, modifier)
}

@Composable
private fun MdBlocks(blocks: List<MdBlock>, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(BLOCK_GAP)) {
        blocks.forEach { MdBlockView(it) }
    }
}

@Composable
private fun MdBlockView(block: MdBlock) {
    val c = Yhtye.colors
    when (block) {
        is MdBlock.Paragraph -> Body(block.text)
        is MdBlock.Heading -> Text(
            block.text,
            color = c.tx,
            fontWeight = FontWeight.SemiBold,
            fontSize = HEADING_SIZES[block.level - 1].sp,
            lineHeight = (HEADING_SIZES[block.level - 1] * HEADING_LINE).sp,
            modifier = Modifier.padding(top = 6.dp),
        )
        is MdBlock.ListBlock -> Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            block.items.forEachIndexed { i, item ->
                Row {
                    Text(
                        block.markers[i],
                        color = c.tx3,
                        fontSize = BODY_SIZE.sp,
                        lineHeight = (BODY_SIZE * BODY_LINE).sp,
                        modifier = Modifier.widthIn(min = LIST_MARKER_WIDTH).padding(end = 4.dp),
                    )
                    MdBlocks(item, Modifier.weight(1f))
                }
            }
        }
        is MdBlock.Code -> CodeBlock(block)
        is MdBlock.Table -> TableBlock(block)
        is MdBlock.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(3.dp).fillMaxHeight().background(c.lineCard))
            MdBlocks(block.blocks, Modifier.padding(start = 10.dp))
        }
        MdBlock.Rule -> HorizontalDivider(color = c.lineCard)
    }
}

@Composable
private fun Body(text: AnnotatedString) {
    Text(text, color = Yhtye.colors.txBody, fontSize = BODY_SIZE.sp, lineHeight = (BODY_SIZE * BODY_LINE).sp)
}

/**
 * コードブロック: 折り返さず横にスクロールする。はみ出していないときはスクロールを付けない (横のスワイプはパネルへ)。
 * はみ出していれば指はこのスクロールのもの (端まで行った余りもパネルに渡さない。モックと同じ)。
 */
@Composable
private fun CodeBlock(block: MdBlock.Code) {
    val c = Yhtye.colors
    val scroll = rememberScrollState()
    Column(
        Modifier
            .fillMaxWidth()
            .clip(CODE_SHAPE)
            .border(1.dp, c.lineCard, CODE_SHAPE)
            .background(c.codeBg)
            .testTag("code-block"),
    ) {
        block.language?.let {
            Text(
                it,
                color = c.tx4,
                fontFamily = MonoFont,
                fontSize = 10.5.sp,
                letterSpacing = 0.6.sp,
                modifier = Modifier.padding(start = 12.dp, top = 8.dp, end = 12.dp),
            )
        }
        Text(
            block.code,
            color = c.tx,
            fontFamily = MonoFont,
            fontSize = 12.5.sp,
            lineHeight = 20.sp,
            softWrap = false,
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(scroll, enabled = scroll.maxValue > 0)
                .padding(start = 12.dp, top = 8.dp, end = 12.dp, bottom = 12.dp)
                .testTag("code-scroll"),
        )
    }
}

/** 表: セルは折り返さず、表全体が横にスクロールする (コードブロックと同じ規則)。 */
@Composable
private fun TableBlock(block: MdBlock.Table) {
    val c = Yhtye.colors
    val scroll = rememberScrollState()
    val columns = maxOf(block.header.size, block.rows.maxOfOrNull { it.size } ?: 0)
    Box(
        Modifier
            .fillMaxWidth()
            .clip(CODE_SHAPE)
            .border(1.dp, c.lineCard, CODE_SHAPE)
            .horizontalScroll(scroll, enabled = scroll.maxValue > 0)
            .testTag("md-table"),
    ) {
        Row {
            for (col in 0 until columns) {
                Column(Modifier.width(IntrinsicSize.Max)) {
                    TableCell(block.header.getOrNull(col) ?: AnnotatedString(""), header = true)
                    block.rows.forEach { row ->
                        HorizontalDivider(color = c.line)
                        TableCell(row.getOrNull(col) ?: AnnotatedString(""), header = false)
                    }
                }
            }
        }
    }
}

@Composable
private fun TableCell(text: AnnotatedString, header: Boolean) {
    val c = Yhtye.colors
    Text(
        text,
        color = if (header) c.tx3 else c.txBody,
        fontWeight = if (header) FontWeight.SemiBold else FontWeight.Normal,
        fontSize = if (header) 12.5.sp else 13.5.sp,
        lineHeight = 20.sp,
        maxLines = 1,
        softWrap = false,
        modifier = Modifier
            .fillMaxWidth()
            .then(if (header) Modifier.background(c.bgInset) else Modifier)
            .padding(horizontal = 12.dp, vertical = 7.dp),
    )
}

private const val BODY_SIZE = 15f
private const val BODY_LINE = 1.65f
private val HEADING_SIZES = floatArrayOf(18f, 17f, 15.5f)
private const val HEADING_LINE = 1.4f
