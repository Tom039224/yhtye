import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AppStore } from "../store/app";
import { newProjectView, type ProjectView } from "../store/project";
import { StoreContext } from "../store/useStore";
import { PROJECT } from "../test/fixtures";
import { stubIntersectionObserver } from "../test/intersectionObserver";
import { OlderHistory } from "./OlderHistory";

afterEach(() => {
  vi.unstubAllGlobals();
});

function viewWith(patch: Partial<ProjectView> = {}): ProjectView {
  return { ...newProjectView(PROJECT), phase: "ready", historyStart: 40, ...patch };
}

function setup(first: ProjectView = viewWith()) {
  const store = { loadOlderHistory: vi.fn(async () => {}) };
  const scroller = { current: document.createElement("div") };
  const ui = (view: ProjectView) => (
    <StoreContext.Provider value={store as unknown as AppStore}>
      <OlderHistory view={view} scroller={scroller} />
    </StoreContext.Provider>
  );
  const utils = render(ui(first));
  return { store, scroller, show: (view: ProjectView) => utils.rerender(ui(view)), ...utils };
}

/** Lets the observers' first reports (a microtask after observing) through. */
const settle = () => act(async () => {});

describe("OlderHistory", () => {
  it("loads the next page, once, when the row comes into view", async () => {
    const io = stubIntersectionObserver(false);
    const { store, scroller } = setup();
    await settle();
    expect(store.loadOlderHistory).not.toHaveBeenCalled();
    expect(io.live()).toHaveLength(1);
    expect(io.live()[0].options?.root).toBe(scroller.current);

    io.visible = true;
    await act(async () => io.notify());
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);
  });

  it("does not ask again while a page is loading", async () => {
    const io = stubIntersectionObserver(true);
    const { store, show } = setup();
    await settle();
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);

    show(viewWith({ loadingOlder: true }));
    await settle();
    expect(io.live()).toHaveLength(0);
    await act(async () => io.notify());
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("読み込み中…");
  });

  it("ignores a report that arrives after the row stopped waiting", async () => {
    const io = stubIntersectionObserver(false);
    const { store, show } = setup();
    await settle();
    const stale = io.created[0];
    show(viewWith({ loadingOlder: true }));
    await settle();
    await act(async () => stale.deliver(true));
    expect(store.loadOlderHistory).not.toHaveBeenCalled();
  });

  it("asks for the next page after one arrived while the row is still in view, until the log is exhausted", async () => {
    const io = stubIntersectionObserver(true);
    const { store, show, container } = setup(viewWith({ historyStart: 15 }));
    await settle();
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);

    show(viewWith({ historyStart: 15, loadingOlder: true }));
    show(viewWith({ historyStart: 8 }));
    await settle();
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(2);

    show(viewWith({ historyStart: 8, loadingOlder: true }));
    show(viewWith({ historyStart: 1 }));
    await settle();
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(2);
    expect(io.live()).toHaveLength(0);
    expect(container).toBeEmptyDOMElement();
  });

  it("does not load anything when the whole log is loaded", async () => {
    const io = stubIntersectionObserver(true);
    const { store, container } = setup(viewWith({ historyStart: 1 }));
    await settle();
    expect(container).toBeEmptyDOMElement();
    expect(io.created).toHaveLength(0);
    expect(store.loadOlderHistory).not.toHaveBeenCalled();
  });

  it("waits until the project is ready", async () => {
    const io = stubIntersectionObserver(true);
    const { store, show } = setup(viewWith({ phase: "loading" }));
    await settle();
    expect(store.loadOlderHistory).not.toHaveBeenCalled();
    show(viewWith({ phase: "ready" }));
    await settle();
    expect(io.live()).toHaveLength(1);
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);
  });

  it("stops after a failure, shows it, and retries only on request", async () => {
    const io = stubIntersectionObserver(true);
    const user = userEvent.setup();
    const { store, show } = setup(viewWith({ olderError: "bridge restarted" }));
    await settle();
    expect(io.live()).toHaveLength(0);
    expect(store.loadOlderHistory).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("履歴を読み込めませんでした: bridge restarted");

    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);

    // The store cleared the error and started over: the row watches again.
    show(viewWith({ loadingOlder: true }));
    show(viewWith());
    await settle();
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(2);
  });

  it("stops watching when it is removed", async () => {
    const io = stubIntersectionObserver(false);
    const { unmount } = setup();
    await settle();
    expect(io.live()).toHaveLength(1);
    unmount();
    expect(io.live()).toHaveLength(0);
  });

  it("is a button to press where there is no IntersectionObserver", async () => {
    vi.stubGlobal("IntersectionObserver", undefined);
    const user = userEvent.setup();
    const { store } = setup();
    await user.click(screen.getByRole("button", { name: "さらに前の履歴を読み込む (#39 まで)" }));
    expect(store.loadOlderHistory).toHaveBeenCalledTimes(1);
  });
});
