import { type RefObject, useEffect, useRef } from "react";

import type { ProjectView } from "../store/project";
import { useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";

/** How far above the visible top (px) the row counts as reached, so history arrives before the reader hits the top. */
const TOP_MARGIN_PX = 120;

interface Props {
  view: ProjectView;
  /** The scroll area the transcript is in (the sentinel is watched inside it). */
  scroller: RefObject<HTMLElement | null>;
}

/**
 * The first row of a transcript while the start of the log is not loaded (lazy
 * history). It is the sentinel: once it comes into view the next older page is
 * loaded, and again after each page while it is still in view (a short
 * transcript), until the log is exhausted. A failure stops the loading and is
 * shown with a retry. The row keeps its height in every state, so loading does
 * not move the transcript.
 */
export function OlderHistory({ view, scroller }: Props) {
  const store = useStore();
  const sentinel = useRef<HTMLDivElement>(null);
  const { historyStart, loadingOlder, olderError } = view;
  const auto = typeof IntersectionObserver !== "undefined";
  const waiting = historyStart > 1 && view.phase === "ready" && !loadingOlder && !olderError;

  useEffect(() => {
    const el = sentinel.current;
    if (!auto || !waiting || !el) return;
    let active = true;
    const observer = new IntersectionObserver(
      (entries) => {
        if (active && entries[entries.length - 1]?.isIntersecting) void store.loadOlderHistory();
      },
      { root: scroller.current, rootMargin: `${TOP_MARGIN_PX}px 0px 0px 0px` },
    );
    // A new observer reports the row's visibility at once: after a page was
    // added, a row that is still in view loads the next one. `historyStart`
    // shrinks with every page, so this ends.
    observer.observe(el);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [auto, waiting, historyStart, scroller, store]);

  if (historyStart <= 1) return null;
  const retry = () => void store.loadOlderHistory();
  return (
    <div className="older-history" ref={sentinel}>
      {olderError ? (
        <>
          <Icon name="alert" size={12} className="older-history-alert" />
          <span className="older-history-text" role="alert" title={olderError}>
            履歴を読み込めませんでした: {olderError}
          </span>
          <IconButton icon="refresh" size={13} className="load-older" label="再試行" onClick={retry} />
        </>
      ) : auto ? (
        <span className="older-history-text" role="status">
          <Icon name="loader" size={12} spin />
          読み込み中…
        </span>
      ) : (
        <IconButton
          icon={loadingOlder ? "loader" : "chevrons-up"}
          spin={loadingOlder}
          className="load-older"
          label={loadingOlder ? "読み込み中…" : `さらに前の履歴を読み込む (#${historyStart - 1} まで)`}
          disabled={loadingOlder || view.phase !== "ready"}
          onClick={retry}
        />
      )}
    </div>
  );
}
