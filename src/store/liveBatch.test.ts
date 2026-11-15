import { afterEach, describe, expect, it, vi } from "vitest";

import { chunk } from "../test/events";
import { frameScheduler, LIVE_FLUSH_MAX_MS, LiveBatch, type LiveScheduler } from "./liveBatch";

/** A scheduler the test runs by hand. */
function manual() {
  const waiting: (() => void)[] = [];
  const cancelled: (() => void)[] = [];
  const schedule: LiveScheduler = (flush) => {
    waiting.push(flush);
    return () => void cancelled.push(flush);
  };
  return { schedule, waiting, cancelled };
}

const E = (text: string) => chunk(0, "s", text);

describe("LiveBatch", () => {
  it("asks for one flush however many events arrive, then shows them together, in order", () => {
    const sched = manual();
    const shown: string[][] = [];
    const batch = new LiveBatch(sched.schedule, (events) => shown.push(events.map((e) => JSON.stringify(e.body).match(/"text":"(.*?)"/)?.[1] ?? "")));
    batch.push(E("a"));
    batch.push(E("b"));
    batch.push(E("c"));
    expect(sched.waiting).toHaveLength(1);
    expect(shown).toEqual([]);
    sched.waiting[0]();
    expect(shown).toEqual([["a", "b", "c"]]);
    expect(batch.size).toBe(0);
  });

  it("asks again for the events after a flush", () => {
    const sched = manual();
    const batch = new LiveBatch(sched.schedule, () => undefined);
    batch.push(E("a"));
    batch.flush();
    batch.push(E("b"));
    expect(sched.waiting).toHaveLength(2);
  });

  it("flush by hand cancels the scheduled one and shows nothing twice", () => {
    const sched = manual();
    const show = vi.fn();
    const batch = new LiveBatch(sched.schedule, show);
    batch.push(E("a"));
    batch.flush();
    expect(sched.cancelled).toEqual(sched.waiting);
    expect(show).toHaveBeenCalledTimes(1);
    batch.flush();
    expect(show).toHaveBeenCalledTimes(1);
  });

  it("forgets what is waiting on clear", () => {
    const sched = manual();
    const show = vi.fn();
    const batch = new LiveBatch(sched.schedule, show);
    batch.push(E("a"));
    batch.clear();
    batch.flush();
    expect(show).not.toHaveBeenCalled();
    expect(sched.cancelled).toHaveLength(1);
  });
});

describe("frameScheduler", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("flushes at the next animation frame, and only once", () => {
    vi.useFakeTimers();
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (f: () => void) => frames.push(f));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const flush = vi.fn();
    frameScheduler(flush);
    expect(flush).not.toHaveBeenCalled();
    frames[0]();
    expect(flush).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(LIVE_FLUSH_MAX_MS * 2); // the timer was cancelled by the frame
    expect(flush).toHaveBeenCalledTimes(1);
  });

  it("flushes after LIVE_FLUSH_MAX_MS when no frame comes (a window that is not visible)", () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", () => 1);
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const flush = vi.fn();
    frameScheduler(flush);
    vi.advanceTimersByTime(LIVE_FLUSH_MAX_MS - 1);
    expect(flush).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(flush).toHaveBeenCalledTimes(1);
  });

  it("falls back to the timer where there are no animation frames", () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", undefined);
    const flush = vi.fn();
    frameScheduler(flush);
    vi.advanceTimersByTime(LIVE_FLUSH_MAX_MS);
    expect(flush).toHaveBeenCalledTimes(1);
  });

  it("flushes nothing once cancelled", () => {
    vi.useFakeTimers();
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (f: () => void) => frames.push(f));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const flush = vi.fn();
    const cancel = frameScheduler(flush);
    cancel();
    frames[0]();
    vi.advanceTimersByTime(LIVE_FLUSH_MAX_MS * 2);
    expect(flush).not.toHaveBeenCalled();
  });
});
