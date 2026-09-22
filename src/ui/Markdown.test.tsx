// Markdown rendering of agent text: formatting, code blocks, streaming of an
// unfinished fence, and that nothing in the text can inject HTML or script.

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Markdown, safeUrl } from "./Markdown";

function html(text: string): HTMLElement {
  const { container } = render(<Markdown text={text} />);
  return container;
}

describe("Markdown", () => {
  it("renders emphasis, lists, inline code, tables and fenced code", () => {
    const c = html(
      "**方針**: `v3` へ寄せる\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```",
    );
    expect(c.querySelector("strong")?.textContent).toBe("方針");
    expect(c.querySelector("p code")?.textContent).toBe("v3");
    expect(c.querySelectorAll("li")).toHaveLength(2);
    expect(c.querySelector("table td")?.textContent).toBe("1");
    const block = c.querySelector("pre code");
    expect(block?.textContent).toBe("fn main() {}\n");
    expect(block?.className).toContain("language-rust");
  });

  it("renders an unfinished code fence while streaming, then the closed one", () => {
    const { container, rerender } = render(<Markdown text={"見てください:\n\n```ts\nconst a = 1;"} />);
    expect(container.querySelector("pre code")?.textContent).toContain("const a = 1;");
    rerender(<Markdown text={"見てください:\n\n```ts\nconst a = 1;\n```\n\n終わり"} />);
    expect(container.querySelector("pre code")?.textContent).toBe("const a = 1;\n");
    expect(container.textContent).toContain("終わり");
  });

  it("never turns raw HTML in the text into elements (XSS)", () => {
    const attack = [
      "<script>window.__pwned = 1</script>",
      '<img src="x" onerror="window.__pwned = 1">',
      '<iframe src="https://evil.example"></iframe>',
      '<a href="javascript:window.__pwned=1">click</a>',
      "<div onmouseover=\"alert(1)\">hover</div>",
    ].join("\n\n");
    const c = html(attack);
    for (const tag of ["script", "img", "iframe", "div div", "[onerror]", "[onmouseover]"]) {
      expect(c.querySelector(`.md ${tag}`), tag).toBeNull();
    }
    expect(c.querySelector('a[href^="javascript"]')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("drops dangerous link and image URLs in Markdown syntax", () => {
    const c = html(
      "[a](javascript:alert(1)) [b](data:text/html;base64,PHNjcmlwdD4=) [c](vbscript:x) [d](/etc/passwd)\n\n" +
        "![tracker](https://evil.example/pixel.png) [ok](https://example.com/docs)",
    );
    const anchors = [...c.querySelectorAll("a")];
    expect(anchors.map((a) => a.getAttribute("href"))).toEqual(["https://example.com/docs"]);
    expect(anchors[0].getAttribute("rel")).toContain("noopener");
    // Dead links keep their text, images are not loaded.
    expect(c.textContent).toContain("a b c d");
    expect(c.querySelector("img")).toBeNull();
    expect(c.textContent).toContain("[tracker]");
  });

  it("keeps only absolute http(s) and mailto URLs", () => {
    expect(safeUrl("https://example.com")).toBe("https://example.com");
    expect(safeUrl("mailto:a@b.c")).toBe("mailto:a@b.c");
    expect(safeUrl(" JavaScript:alert(1)")).toBe("");
    expect(safeUrl("java\nscript:alert(1)")).toBe("");
    expect(safeUrl("//evil.example")).toBe("");
    expect(safeUrl("file:///etc/passwd")).toBe("");
  });
});
