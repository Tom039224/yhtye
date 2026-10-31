import type { ReactNode } from "react";
import type { Components, Options } from "react-markdown";
import remarkGfm from "remark-gfm";

import { ExternalLink } from "./ExternalLink";

/**
 * How agent text is turned into elements (see `Markdown.tsx` for the safety rules).
 * One place, so that the whole text and the pieces of a streamed one are
 * rendered the same way.
 */

const SAFE_URL = /^(https?:\/\/|mailto:)/i;

/** Keeps only absolute http(s) / mailto URLs; everything else becomes "". */
export function safeUrl(url: string): string {
  const trimmed = url.trim();
  return SAFE_URL.test(trimmed) ? trimmed : "";
}

function Link({ href, children }: { href?: string; children?: ReactNode }) {
  if (!href) return <span className="md-link-dead">{children}</span>;
  return <ExternalLink href={href}>{children}</ExternalLink>;
}

const COMPONENTS: Components = {
  a: ({ href, children }) => <Link href={href}>{children}</Link>,
  img: ({ alt }) => <span className="md-image">[{alt || "image"}]</span>,
};

export type RemarkPlugins = NonNullable<Options["remarkPlugins"]>;

/** The plugins every render uses; a caller may add its own after them. */
export const BASE_PLUGINS: RemarkPlugins = [remarkGfm];

export const MARKDOWN_OPTIONS = {
  skipHtml: true,
  urlTransform: safeUrl,
  components: COMPONENTS,
} satisfies Options;
