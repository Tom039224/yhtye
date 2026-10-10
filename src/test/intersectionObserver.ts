// jsdom has no IntersectionObserver (and no layout): a stand-in that reports what the test says is in view.
// Like the real one, a new observation is reported at once (asynchronously) with the current state.

import { vi } from "vitest";

export class FakeObserver {
  readonly targets = new Set<Element>();
  /** Everything ever observed: a report queued before `disconnect` may still arrive. */
  private readonly seen = new Set<Element>();
  disconnected = false;

  constructor(
    private readonly owner: FakeIntersection,
    readonly callback: IntersectionObserverCallback,
    readonly options: IntersectionObserverInit | undefined,
  ) {}

  observe(target: Element): void {
    this.targets.add(target);
    this.seen.add(target);
    queueMicrotask(() => {
      if (!this.disconnected) this.deliver(this.owner.visible);
    });
  }

  unobserve(target: Element): void {
    this.targets.delete(target);
  }

  disconnect(): void {
    this.disconnected = true;
    this.targets.clear();
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  /** Calls the callback as the browser would, whatever the observer's state (a late entry). */
  deliver(isIntersecting: boolean): void {
    const entries = [...this.seen].map((target) => ({ target, isIntersecting }) as IntersectionObserverEntry);
    if (entries.length > 0) this.callback(entries, this as unknown as IntersectionObserver);
  }
}

export class FakeIntersection {
  /** Whether observed elements are in view (reported when observing starts, and by `notify`). */
  visible: boolean;
  /** Every observer that was created. */
  readonly created: FakeObserver[] = [];

  constructor(visible: boolean) {
    this.visible = visible;
  }

  /** The observers still watching something. */
  live(): FakeObserver[] {
    return this.created.filter((o) => !o.disconnected);
  }

  /** Reports the current visibility to the live observers (a scroll). */
  notify(): void {
    for (const o of this.live()) o.deliver(this.visible);
  }
}

/** Installs the stand-in as the global `IntersectionObserver` (undo with `vi.unstubAllGlobals()`). */
export function stubIntersectionObserver(visible = false): FakeIntersection {
  const owner = new FakeIntersection(visible);
  class Observer extends FakeObserver {
    constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
      super(owner, callback, options);
      owner.created.push(this);
    }
  }
  vi.stubGlobal("IntersectionObserver", Observer);
  return owner;
}
