import { memo, type MouseEvent, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

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
 * Streaming: the text is re-parsed as chunks arrive; an unfinished code fence
 * simply renders as an open code block until it is closed.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={PLUGINS} skipHtml urlTransform={safeUrl} components={COMPONENTS}>
        {text}
      </ReactMarkdown>
    </div>
  );
});

const PLUGINS = [remarkGfm];

const SAFE_URL = /^(https?:\/\/|mailto:)/i;

/** Keeps only absolute http(s) / mailto URLs; everything else becomes "". */
export function safeUrl(url: string): string {
  const trimmed = url.trim();
  return SAFE_URL.test(trimmed) ? trimmed : "";
}

function openExternally(event: MouseEvent<HTMLAnchorElement>, href: string) {
  event.preventDefault();
  if ("__TAURI_INTERNALS__" in window) {
    void import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(href)).catch(() => undefined);
  } else {
    window.open(href, "_blank", "noopener,noreferrer");
  }
}

function Link({ href, children }: { href?: string; children?: ReactNode }) {
  if (!href) return <span className="md-link-dead">{children}</span>;
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" onClick={(e) => openExternally(e, href)}>
      {children}
    </a>
  );
}

const COMPONENTS: Components = {
  a: ({ href, children }) => <Link href={href}>{children}</Link>,
  img: ({ alt }) => <span className="md-image">[{alt || "image"}]</span>,
};
