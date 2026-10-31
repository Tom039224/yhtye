// Live events (streamed text, usage readings) come many times per frame, from
// every session of the open project. Showing each one would make the screen
// render again for each; the store collects them here and shows what arrived
// once per frame instead (core-design.md §2: live events only feed `streaming`).

import type { ApiEvent } from "../api/generated";

/**
 * The longest a live event waits before it is shown. A window that is not
 * visible gets no animation frames, so a timer makes sure it is shown anyway.
 */
export const LIVE_FLUSH_MAX_MS = 50;

/** Arranges one call of `flush` soon; returns what cancels it. */
export type LiveScheduler = (flush: () => void) => () => void;

/** The next animation frame, or `LIVE_FLUSH_MAX_MS` from now if none comes first. */
export const frameScheduler: LiveScheduler = (flush) => {
  let done = false;
  const frame = typeof requestAnimationFrame === "function" ? requestAnimationFrame(run) : null;
  const timer = setTimeout(run, LIVE_FLUSH_MAX_MS);
  function cancel(): void {
    done = true;
    if (frame !== null) cancelAnimationFrame(frame);
    clearTimeout(timer);
  }
  function run(): void {
    if (done) return;
    cancel();
    flush();
  }
  return cancel;
};

/**
 * Live events waiting to be shown. The first one asks the scheduler for a
 * flush; `flush` hands everything that arrived since to `show` at once, in
 * arrival order. The owner flushes before it changes the view in any other way,
 * so that live events never swap places with stored ones.
 */
export class LiveBatch {
  private events: ApiEvent[] = [];
  private cancel: (() => void) | null = null;

  constructor(
    private readonly schedule: LiveScheduler,
    private readonly show: (events: ApiEvent[]) => void,
  ) {}

  get size(): number {
    return this.events.length;
  }

  push(ev: ApiEvent): void {
    this.events.push(ev);
    this.cancel ??= this.schedule(() => this.flush());
  }

  /** Shows what is waiting (nothing happens if nothing is). */
  flush(): void {
    const events = this.take();
    if (events.length > 0) this.show(events);
  }

  /** Forgets what is waiting (the view it was for is gone). */
  clear(): void {
    this.take();
  }

  private take(): ApiEvent[] {
    this.cancel?.();
    this.cancel = null;
    const events = this.events;
    this.events = [];
    return events;
  }
}
