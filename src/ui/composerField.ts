// The composer's textarea fits its height to its text without being measured
// itself. Writing `style.height = "auto"` and reading `scrollHeight` on every key
// made the textarea's size depend on its content for a moment, and WebKit then
// re-laid out the whole conversation: it stops marking layout dirty at a text
// control only when the control's width is not `auto` and its height is a fixed
// length (~0.65 s per key in a 1000-exchange chat). Instead the height is a pixel
// value that is written only when it changes, and a hidden mirror of the text
// (App.css `.composer-field-mirror`: `height: 0` and out of flow, so measuring it
// lays out nothing else) says what it should be.

import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef } from "react";

/**
 * The text of the field's hidden mirror. The mirror renders it like the textarea
 * (`pre-wrap`, wrapped at the same width). A line break at the very end shows no
 * empty line in a block, but the textarea has the caret on one, so a trailing
 * space gives that line a box (a space at the end of a line hangs and never wraps).
 */
export function mirrorText(text: string): string {
  return `${text} `;
}

export interface FieldMetrics {
  /** The height the whole text needs (px; 0 when not laid out). */
  content: number;
  /** The tallest the textarea may grow (px; `Infinity` for none). */
  max: number;
}

/** The height (px) to give the textarea, or `null` to leave it: not laid out, or already that height. */
export function nextFieldHeight(metrics: FieldMetrics, current: number | null): number | null {
  if (!(metrics.content > 0)) return null;
  const next = Math.min(metrics.content, metrics.max);
  return next === current ? null : next;
}

/** `scrollHeight` of the mirror (it is `height: 0; overflow: hidden`) and the textarea's CSS `max-height`. */
export function readFieldMetrics(textarea: HTMLTextAreaElement, mirror: HTMLElement): FieldMetrics {
  return { content: mirror.scrollHeight, max: parseFloat(getComputedStyle(textarea).maxHeight) || Infinity };
}

/**
 * Fits `textarea`'s height to `text` (up to its CSS `max-height`, then it
 * scrolls): after every change of the text, and when the field's width changes
 * (the text wraps differently). The height is written only when it differs.
 */
export function useFieldHeight(
  textarea: RefObject<HTMLTextAreaElement | null>,
  mirror: RefObject<HTMLElement | null>,
  text: string,
  measure: (textarea: HTMLTextAreaElement, mirror: HTMLElement) => FieldMetrics = readFieldMetrics,
): void {
  const applied = useRef<number | null>(null);
  const fit = useCallback(() => {
    const field = textarea.current;
    const copy = mirror.current;
    if (!field || !copy) return;
    const next = nextFieldHeight(measure(field, copy), applied.current);
    if (next === null) return;
    applied.current = next;
    field.style.height = `${next}px`;
  }, [textarea, mirror, measure]);

  // `text` is what the mirror shows, so it is up to date by the time this runs.
  useLayoutEffect(fit, [fit, text]);

  useEffect(() => {
    const slot = textarea.current?.parentElement;
    if (!slot || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(fit);
    observer.observe(slot);
    return () => observer.disconnect();
  }, [textarea, fit]);
}
