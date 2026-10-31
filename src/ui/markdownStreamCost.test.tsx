// What streaming a long reply costs: the closed blocks are not parsed or
// rendered again for the next chunk, and the stored text takes what the stream made.

import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { markdownOf } from "../test/benchChat";
import { Markdown, StreamingMarkdown } from "./Markdown";

// Every parse of react-markdown goes through here: the length of the text it was given.
vi.mock("react-markdown", async (original) => {
  const real = await original<typeof import("react-markdown")>();
  return { ...real, default: vi.fn(real.default) };
});
// A link renders when its block is rendered.
const linkRenders = vi.hoisted(() => vi.fn());
vi.mock("./ExternalLink", () => ({
  ExternalLink: ({ href, children }: { href: string; children?: ReactNode }) => {
    linkRenders(href);
    return <a href={href}>{children}</a>;
  },
}));

const parse = vi.mocked(ReactMarkdown);
const parsedChars = () => parse.mock.calls.reduce((n, [options]) => n + (options.children?.length ?? 0), 0);

/** Streams `text` in pieces of `size` through the component; calls `each` after every piece. */
function streamThrough(text: string, size: number, each?: (shown: string) => void) {
  const view = render(<StreamingMarkdown text="" />);
  for (let at = size; ; at += size) {
    const shown = text.slice(0, Math.min(at, text.length));
    view.rerender(<StreamingMarkdown text={shown} />);
    each?.(shown);
    if (at >= text.length) break;
  }
  return view;
}

beforeEach(() => {
  parse.mockClear();
  linkRenders.mockClear();
});

describe("the work of a streamed reply", () => {
  it("parses the end of the text for a chunk, not the whole text", () => {
    const text = markdownOf(7, 9000);
    const size = 12;
    streamThrough(text, size);
    // The whole text for every chunk is about length²/(2·size) characters.
    const before = (text.length * text.length) / (2 * size);
    expect(parsedChars()).toBeLessThan(before / 5);
  });

  it("does not render or touch a closed block when a chunk arrives", () => {
    const text = "first [one](https://example.com/1)\n\nsecond [two](https://example.com/2)\n\nthird\n\nfourth\n\nfifth\n\nsixth\n\nseventh";
    const view = render(<StreamingMarkdown text="" />);
    const first = () => view.container.querySelector("p");
    let paragraph: Element | null = null;
    let mutations: MutationRecord[] = [];
    const observer = new MutationObserver((records) => void mutations.push(...records));
    for (let at = 1; at <= text.length; at++) {
      view.rerender(<StreamingMarkdown text={text.slice(0, at)} />);
      // Two blocks stay open: from "fourth" on, the first block is closed.
      if (text.slice(0, at).endsWith("fourth") && !paragraph) {
        paragraph = first();
        linkRenders.mockClear();
        observer.observe(paragraph!, { subtree: true, childList: true, characterData: true, attributes: true });
        mutations = [];
      }
    }
    expect(paragraph).not.toBeNull();
    expect(first()).toBe(paragraph);
    expect(mutations.map((m) => m.type)).toEqual([]);
    expect(linkRenders.mock.calls.map(([href]) => href)).not.toContain("https://example.com/1");
    observer.disconnect();
  });

  it("shows the stored text without parsing it again", () => {
    const text = markdownOf(3, 3000);
    const streamed = streamThrough(text, 25);
    parse.mockClear();
    const stored = render(<Markdown text={text} />);
    expect(parse).not.toHaveBeenCalled();
    expect(stored.container.innerHTML).toBe(streamed.container.innerHTML);
  });

  it("parses a text that was not streamed, once", () => {
    const text = markdownOf(4, 1200);
    render(<Markdown text={text} />);
    expect(parse).toHaveBeenCalledTimes(1);
    expect(parse.mock.calls[0][0].children).toBe(text);
  });
});
