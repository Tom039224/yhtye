import { memo, useEffect, useRef } from "react";
import ReactMarkdown from "react-markdown";

import { BASE_PLUGINS, MARKDOWN_OPTIONS, safeUrl } from "./markdownOptions";
import { advance, EMPTY_STREAM, recallOutput, rememberOutput } from "./markdownStream";

export { safeUrl };

/**
 * Agent text (orchestrator replies, sub-agent output) rendered as Markdown.
 *
 * Safety (the text comes from an LLM that reads untrusted repositories):
 * - raw HTML is never rendered as elements: react-markdown without `rehype-raw`,
 *   plus `skipHtml` so tags in the text are dropped instead of interpreted;
 * - links are kept only for `http(s):` / `mailto:` and open outside the app
 *   (the Tauri webview must never navigate away from the UI);
 * - images are not loaded (no requests to arbitrary hosts): their alt text is shown.
 *
 * The text of a reply that is still streamed is `StreamingMarkdown`'s: blocks
 * that are complete are kept, only the end is parsed again (`markdownStream.tsx`).
 * When the reply is stored, this component shows the same text; it takes the
 * elements the stream made instead of parsing the text once more.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      {recallOutput(text) ?? (
        <ReactMarkdown {...MARKDOWN_OPTIONS} remarkPlugins={BASE_PLUGINS}>
          {text}
        </ReactMarkdown>
      )}
    </div>
  );
});

/**
 * Text that grows with every chunk. The result is the same DOM as `Markdown`
 * of the whole text; what differs is the work: complete blocks are not parsed
 * again (a render of this component for one more chunk parses its last blocks only).
 */
export function StreamingMarkdown({ text }: { text: string }) {
  // A cache of the parse of the earlier text: `advance` gives the right result
  // whatever it holds, so writing it while rendering (twice, in strict mode) is harmless.
  const state = useRef(EMPTY_STREAM);
  const next = advance(state.current, text);
  state.current = next;
  const shown = useRef<string | null>(null);
  useEffect(() => {
    rememberOutput(shown.current, text, next.output);
    shown.current = text;
  });
  return <div className="md">{next.output}</div>;
}
