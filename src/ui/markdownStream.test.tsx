// Markdown of text that is still streamed: which blocks are kept, and that the
// result is the page the whole text makes (a streamed reply and the stored one
// must look the same).

import { render } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { markdownOf } from "../test/benchChat";
import { Markdown, StreamingMarkdown } from "./Markdown";
import { advance, closedLength, EMPTY_STREAM, parseWithOutline, renderMarkdown, type StreamState } from "./markdownStream";

/** The text in pieces of `size` characters (the last one shorter). */
function pieces(text: string, size: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += size) out.push(text.slice(i, i + size));
  return out;
}

/** The state after streaming `text` in the given pieces; `each` sees every step. */
function stream(text: string, size: number | number[], each?: (state: StreamState, shown: string) => void): StreamState {
  let state = EMPTY_STREAM;
  let shown = "";
  const sizes = typeof size === "number" ? [size] : size;
  for (let at = 0, k = 0; at < text.length; k++) {
    const take = sizes[k % sizes.length];
    shown = text.slice(0, at + take);
    at += take;
    state = advance(state, shown);
    each?.(state, shown);
  }
  return state;
}

const markup = (node: React.ReactNode) => renderToStaticMarkup(<div className="md">{node}</div>);
const whole = (text: string) => markup(renderMarkdown(text));

describe("which blocks are closed", () => {
  const closedOf = (text: string) => stream(text, 1).closed;

  it("keeps the last two blocks open and closes the paragraphs before them", () => {
    expect(closedOf("a\n\nb\n\nc\n\nd")).toEqual(["a\n\n", "b\n\n"]);
    expect(closedOf("a\n\nb")).toEqual([]);
  });

  it("keeps a run of blank lines with the block before it", () => {
    expect(closedOf("a\n\n\n\n\nb\n\n\nc\n\nd")).toEqual(["a\n\n\n\n\n", "b\n\n\n"]);
  });

  it("splits at the line start after CRLF, and leaves the line ending's two characters together", () => {
    const closed = closedOf("a\r\n\r\nb\r\n\r\nc\r\n\r\nd");
    expect(closed).toEqual(["a\r\n\r\n", "b\r\n\r\n"]);
  });

  it("does not split a fenced code block, blank lines inside it included", () => {
    const text = "intro\n\n```ts\nlet a = 1;\n\nlet b = 2;\n```\n\nafter\n\nlast one\n\nend";
    const closed = closedOf(text);
    expect(closed.join("")).toBe("intro\n\n```ts\nlet a = 1;\n\nlet b = 2;\n```\n\nafter\n\n");
    expect(closed).toHaveLength(3);
    expect(closed[1]).toBe("```ts\nlet a = 1;\n\nlet b = 2;\n```\n\n");
  });

  it("keeps a fence that is not closed yet, and what comes after it, in the open end", () => {
    const text = "a\n\nb\n\n```ts\ncode\n\nmore code\n\nand more";
    const state = stream(text, 1);
    expect(state.closed).toEqual(["a\n\n"]);
    // The fence is still one piece: its text was not given to a closed block.
    expect(state.closed.join("")).not.toContain("```");
  });

  it("does not split a table", () => {
    const text = "a\n\n| x | y |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nb\n\nc\n\nd";
    const closed = closedOf(text);
    expect(closed).toEqual(["a\n\n", "| x | y |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n", "b\n\n"]);
  });

  it("does not split a list: blank lines between items, nested items, a numbered one that follows a blank line", () => {
    const text = "1. a\n\n2. b\n\n   - nested\n\n3. c\n\nafter\n\nz\n\nend";
    expect(closedOf(text)).toEqual(["1. a\n\n2. b\n\n   - nested\n\n3. c\n\n", "after\n\n"]);
  });

  it("does not split a block quote, nor a heading from its text", () => {
    const text = "> a\n> b\n>\n> c\n\n# h\n\ntext\n\n## h2\n\nz\n\nend";
    expect(closedOf(text)).toEqual(["> a\n> b\n>\n> c\n\n", "# h\n\n", "text\n\n", "## h2\n\n"]);
  });

  it("does not split an HTML comment that has a blank line in it", () => {
    const text = "a\n\n<!-- x\n\ny -->\n\nb\n\nc\n\nd";
    const closed = closedOf(text);
    expect(closed.join("")).toBe("a\n\n<!-- x\n\ny -->\n\nb\n\n");
    expect(closed[1]).toBe("<!-- x\n\ny -->\n\n");
  });

  it("closes the blocks all at once for a text that comes whole", () => {
    const state = advance(EMPTY_STREAM, "a\n\nb\n\nc\n\nd");
    expect(state.closed).toEqual(["a\n\nb\n\n"]);
  });

  it("closes nothing when the parser gives no positions or too few blocks", () => {
    expect(closedLength("a\n\nb", { blocks: null, definition: false })).toBe(0);
    expect(closedLength("a\n\nb", parseWithOutline("a\n\nb").outline)).toBe(0);
  });
});

describe("a text that depends on its other parts", () => {
  it("is rendered whole once a definition exists, and not before", () => {
    const text = "see [the docs][d]\n\nb\n\nc\n\nd\n\n[d]: https://example.com/docs";
    const states: boolean[] = [];
    stream(text, 3, (state, shown) => {
      if (shown.endsWith("docs")) states.push(state.whole);
    });
    expect(states.at(-1)).toBe(true);
    const final = stream(text, 3);
    expect(final.whole).toBe(true);
    expect(markup(final.output)).toBe(whole(text));
    expect(markup(final.output)).toContain('href="https://example.com/docs"');
  });

  it("is rendered whole for a footnote", () => {
    const text = "a[^1]\n\nb\n\nc\n\nd\n\n[^1]: the note";
    const final = stream(text, 2);
    expect(final.whole).toBe(true);
    expect(markup(final.output)).toBe(whole(text));
    expect(markup(final.output)).toContain("footnote");
  });

  it("starts again from nothing when the text is not the earlier one with more after it", () => {
    const first = stream("a\n\nb\n\nc\n\nd\n\ne", 4);
    const next = advance(first, "x\n\ny\n\nz\n\nw");
    expect(next.closed).toEqual(["x\n\ny\n\n"]);
    expect(markup(next.output)).toBe(whole("x\n\ny\n\nz\n\nw"));
  });
});

const SNIPPETS = [
  "# Heading",
  "## 調査結果",
  "Plain paragraph with **bold**, _emphasis_, `code`, ~~strike~~ and a [link](https://example.com/a?b=1).",
  "Two lines\nin one paragraph\\\nwith a break  \nhere",
  "- one\n- two\n  - nested\n- three",
  "- loose one\n\n- loose two\n\n  continued paragraph\n\n- loose three",
  "1. first\n2. second\n3. third",
  "1. step\n\n   ```bash\n   run it\n\n   again\n   ```\n\n2. next step\n\n    ```\n    indented fence\n    ```",
  "3. starts at three\n4. four",
  "- [x] done\n- [ ] todo",
  "> quote line\n> another\n>\n> - quoted item",
  "> quoted\n\n> second quote",
  "```ts\nconst a = 1;\n\nconst b = [1, 2, 3];\n```",
  "```\nplain fence\n```",
  "~~~py\nprint('tilde')\n\n~~~",
  "````md\n```\ninner\n```\n````",
  "    indented code\n\n    still code",
  "| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| `x` | **y** | z |",
  "---",
  "***",
  "<!-- a comment\n\nwith a blank line -->",
  "<div>html block</div>\n\ntext after",
  "<script>\n\nalert(1)\n\n</script>",
  "![image](https://example.com/p.png) and <b>inline</b> html",
  "Autolink https://example.com/x and www.example.org here",
  "Setext heading\n===",
  "Setext two\n---",
  "text with trailing definition-like [not a def] and [x](y)",
  "Escapes \\* \\_ and entities &amp; &copy;",
  "2024年に変更しました。\n\n- 項目 A\n- 項目 B",
  "1. a\n\n2",
  "- a\n\n-",
];
const SEPARATORS = ["\n\n", "\n\n", "\n\n", "\n", "\n\n\n", "\r\n\r\n", "\n \n", "\n\n  "];

/** A small deterministic generator, so a failure repeats. */
function random(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function documentOf(seed: number): string {
  const next = random(seed);
  const count = 3 + Math.floor(next() * 14);
  let text = "";
  for (let i = 0; i < count; i++) {
    if (i > 0) text += SEPARATORS[Math.floor(next() * SEPARATORS.length)];
    text += SNIPPETS[Math.floor(next() * SNIPPETS.length)];
  }
  return next() < 0.5 ? text : `${text}\n`;
}

const DOCUMENTS: Record<string, string> = {
  "the benchmark's long answer": markdownOf(7, 7000),
  "the benchmark's other answers": [1, 2, 3, 4, 5].map((i) => markdownOf(i, 1500)).join("\n\n"),
  "every kind of block": SNIPPETS.join("\n\n"),
  "the same with CRLF": SNIPPETS.join("\r\n\r\n").replaceAll("\n\n", "\r\n\r\n").replace(/([^\r])\n/g, "$1\r\n"),
  "numbered steps with fences": "1. one\n\n   ```sh\n   a\n\n   b\n   ```\n\n2. two\n\n   ```sh\n   c\n   ```\n\n3. three\n\nDone.\n\nMore.\n\nLast.",
  "a list that continues after a blank line": "1. a\n\n2. b\n\nparagraph\n\n1. c\n\n2",
  // Read from the start, `3. x` after an indented code block is text, not a list: a cut there must not change it.
  "a numbered list after an indented code block": "para\n\n    indented code\n\n    still code\n\n3. starts at three\n4. four\n\nx\n\ny\n\nz",
  "an indented code block after an empty list item": "- a\n\n-\n\n    indented code\n\n    still code\n- a\n\n-\r\n\r\n---\r\n\r\n# Heading\n\nend",
};

describe("a streamed text is the page of the whole text", () => {
  for (const [name, text] of Object.entries(DOCUMENTS)) {
    it(`${name}: in pieces of 1, 7 and 40 characters, and in uneven ones`, () => {
      const expected = whole(text);
      // One character at a time parses the end of the text for every character: only for the short ones.
      const sizes = text.length <= 3000 ? [1, 7, 40, [3, 11, 1, 29, 2, 64]] : [7, 40, [3, 11, 1, 29, 2, 64]];
      for (const size of sizes) {
        const state = stream(text, size);
        expect(markup(state.output), `pieces of ${JSON.stringify(size)}`).toBe(expected);
        expect(text.startsWith(state.closed.join(""))).toBe(true);
      }
    }, 60_000);
  }

  it("holds for generated documents streamed in uneven pieces", { timeout: 60_000 }, () => {
    for (let seed = 1; seed <= 150; seed++) {
      const text = documentOf(seed);
      const next = random(seed * 7919);
      const sizes = Array.from({ length: 6 }, () => 1 + Math.floor(next() * 30));
      const state = stream(text, sizes);
      expect(markup(state.output), `seed ${seed}: ${JSON.stringify(text)} in ${sizes}`).toBe(whole(text));
    }
  });

  it("holds at every step for a document that has the hard cases", { timeout: 60_000 }, () => {
    const text = DOCUMENTS["every kind of block"].slice(0, 2600);
    stream(text, 1, (state, shown) => {
      // Cheap checks on every step; the whole page is compared every few steps.
      expect(shown.startsWith(state.closed.join(""))).toBe(true);
      if (shown.length % 37 === 0) expect(markup(state.output), `after ${shown.length} characters`).toBe(whole(shown));
    });
  });

  it("puts the same DOM in the page when streamed through the component", () => {
    const text = DOCUMENTS["every kind of block"];
    const { container, rerender } = render(<StreamingMarkdown text="" />);
    for (const shown of pieces(text, 25).map((_, i, all) => all.slice(0, i + 1).join(""))) rerender(<StreamingMarkdown text={shown} />);
    const whole = render(<Markdown text={text} />);
    expect(container.innerHTML).toBe(whole.container.innerHTML);
    expect(container.querySelectorAll("pre").length).toBeGreaterThan(3);
  });
});
