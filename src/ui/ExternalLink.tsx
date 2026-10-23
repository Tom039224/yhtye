import type { MouseEvent, ReactNode } from "react";

/**
 * Opens `href` outside the app: in the system browser through the Tauri opener
 * inside the app (the webview must never navigate away from the UI), in a new
 * tab elsewhere.
 */
export function openExternal(href: string): void {
  if ("__TAURI_INTERNALS__" in window) {
    void import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(href)).catch(() => undefined);
  } else {
    window.open(href, "_blank", "noopener,noreferrer");
  }
}

/** The click handler of an anchor that opens `href` with {@link openExternal} instead of following it. */
export function openExternally(event: MouseEvent<HTMLAnchorElement>, href: string): void {
  event.preventDefault();
  openExternal(href);
}

/** A link to a web page that opens outside the app. */
export function ExternalLink({ href, className, children }: { href: string; className?: string; children: ReactNode }) {
  return (
    <a href={href} className={className} target="_blank" rel="noopener noreferrer" onClick={(e) => openExternally(e, href)}>
      {children}
    </a>
  );
}
