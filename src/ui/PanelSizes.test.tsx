// The work view's draggable boundaries: the sizes are restored from the
// preferences, saved when a gesture ends, and reset to the CSS defaults.

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { AppStore } from "../store/app";
import { type Prefs, memoryPrefs } from "../store/prefs";
import { StoreContext } from "../store/useStore";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { LAYOUT_KEY } from "./layout";

const LOG = durable(FULL_RUN);

async function setup(prefs: Prefs = memoryPrefs()) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.log = LOG;
  core.attach(transport);
  const store = new AppStore(transport, { prefs });
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
  return { prefs, user: userEvent.setup() };
}

const stored = (prefs: Prefs) => JSON.parse(prefs.get(LAYOUT_KEY) ?? "{}");
const bodyVar = (name: string) => document.querySelector<HTMLElement>(".body")?.style.getPropertyValue(name);
const workspaceVar = (name: string) => document.querySelector<HTMLElement>(".workspace")?.style.getPropertyValue(name);

/** jsdom lays nothing out: report the sizes the handles measure (and the container's room). */
function layOut(sizes: { sidebar?: number; right?: number; bottom?: number }) {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const pick = (width: number, height: number) => ({ width, height, x: 0, y: 0, top: 0, left: 0, right: width, bottom: height, toJSON: () => ({}) });
    if (this.classList.contains("sidebar")) return pick(sizes.sidebar ?? 214, 800);
    if (this.classList.contains("right")) return pick(sizes.right ?? 600, 800);
    if (this.classList.contains("bottom")) return pick(600, sizes.bottom ?? 252);
    return pick(0, 0);
  });
  for (const [selector, width, height] of [
    [".body", 1440, 800],
    [".workspace", 1182, 800],
    [".right", 600, 800],
  ] as const) {
    const el = document.querySelector<HTMLElement>(selector);
    if (el) {
      Object.defineProperty(el, "clientWidth", { configurable: true, value: width });
      Object.defineProperty(el, "clientHeight", { configurable: true, value: height });
    }
  }
}

afterEach(() => vi.restoreAllMocks());

describe("panel sizes", () => {
  it("puts a resize handle on each of the three boundaries", async () => {
    await setup();
    expect(screen.getByRole("separator", { name: "サイドバーの幅" })).toHaveAttribute("aria-orientation", "vertical");
    expect(screen.getByRole("separator", { name: "右パネルの幅" })).toHaveAttribute("aria-orientation", "vertical");
    expect(screen.getByRole("separator", { name: "git パネルの高さ" })).toHaveAttribute("aria-orientation", "horizontal");
  });

  it("keeps the CSS defaults while nothing is stored", async () => {
    await setup();
    expect(bodyVar("--sidebar-width")).toBe("");
    expect(workspaceVar("--right-width")).toBe("");
    expect(workspaceVar("--bottom-height")).toBe("");
  });

  it("restores the stored sizes", async () => {
    await setup(memoryPrefs({ [LAYOUT_KEY]: JSON.stringify({ sidebar: 260, right: 640, git: 300 }) }));
    expect(bodyVar("--sidebar-width")).toBe("260px");
    expect(workspaceVar("--right-width")).toBe("640px");
    expect(workspaceVar("--bottom-height")).toBe("300px");
  });

  it("ignores a damaged stored value", async () => {
    await setup(memoryPrefs({ [LAYOUT_KEY]: "{broken" }));
    expect(bodyVar("--sidebar-width")).toBe("");
    expect(screen.getByRole("separator", { name: "右パネルの幅" })).toBeInTheDocument();
  });

  it("saves the sidebar width when a key press or a drag ends, and forgets it on a double click", async () => {
    const { prefs } = await setup();
    layOut({ sidebar: 214 });
    const handle = screen.getByRole("separator", { name: "サイドバーの幅" });
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(stored(prefs)).toEqual({ sidebar: 230 });
    expect(bodyVar("--sidebar-width")).toBe("230px");

    // A drag moves the width live and saves it once, at the end.
    layOut({ sidebar: 230 });
    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: 230 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 280 });
    expect(bodyVar("--sidebar-width")).toBe("280px");
    expect(stored(prefs)).toEqual({ sidebar: 230 });
    fireEvent.pointerUp(handle, { pointerId: 1 });
    expect(stored(prefs)).toEqual({ sidebar: 280 });

    fireEvent.doubleClick(handle);
    expect(stored(prefs)).toEqual({});
    expect(bodyVar("--sidebar-width")).toBe("");
  });

  it("keeps the sidebar within its limits", async () => {
    const { prefs } = await setup();
    layOut({ sidebar: 214 });
    const handle = screen.getByRole("separator", { name: "サイドバーの幅" });
    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: 214 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 5000 });
    fireEvent.pointerUp(handle, { pointerId: 1 });
    expect(stored(prefs).sidebar).toBe(420);
    fireEvent.keyDown(handle, { key: "Home" });
    expect(stored(prefs).sidebar).toBe(160);
  });

  it("resizes the right column from its left edge", async () => {
    const { prefs } = await setup();
    layOut({ right: 600 });
    const handle = screen.getByRole("separator", { name: "右パネルの幅" });
    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: 580 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 500 });
    fireEvent.pointerUp(handle, { pointerId: 1 });
    // The pointer moved 80px left: the column on the right grows by as much.
    expect(stored(prefs)).toEqual({ right: 680 });
    expect(workspaceVar("--right-width")).toBe("680px");
  });

  it("keeps the conversation at least 380px and the right column at least 420px", async () => {
    const { prefs } = await setup();
    layOut({ right: 600 });
    const handle = screen.getByRole("separator", { name: "右パネルの幅" });
    fireEvent.keyDown(handle, { key: "End" });
    expect(stored(prefs).right).toBe(1182 - 380);
    fireEvent.keyDown(handle, { key: "Home" });
    expect(stored(prefs).right).toBe(420);
  });

  it("sizes the git panel and the agent output separately", async () => {
    const { prefs, user } = await setup(memoryPrefs({ [LAYOUT_KEY]: JSON.stringify({ git: 300, output: 420 }) }));
    expect(workspaceVar("--bottom-height")).toBe("300px");

    await user.click(screen.getByRole("button", { name: "add hello.txt" }));
    const output = screen.getByRole("region", { name: "agent output" });
    expect(workspaceVar("--bottom-height")).toBe("420px");
    layOut({ bottom: 420 });
    const handle = screen.getByRole("separator", { name: "出力パネルの高さ" });
    // The panel is below the handle: the pointer moving up makes it taller.
    fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientY: 300 });
    fireEvent.pointerMove(handle, { pointerId: 1, clientY: 250 });
    fireEvent.pointerUp(handle, { pointerId: 1 });
    expect(stored(prefs)).toEqual({ git: 300, output: 470 });

    await user.click(within(output).getByRole("button", { name: "出力を閉じる" }));
    expect(screen.getByRole("region", { name: "git" })).toBeInTheDocument();
    expect(workspaceVar("--bottom-height")).toBe("300px");
  });

  it("keeps the task list its room above the bottom panel", async () => {
    const { prefs } = await setup();
    layOut({ bottom: 252 });
    const handle = screen.getByRole("separator", { name: "git パネルの高さ" });
    fireEvent.keyDown(handle, { key: "End" });
    expect(stored(prefs).git).toBe(800 - 160);
    fireEvent.keyDown(handle, { key: "Home" });
    expect(stored(prefs).git).toBe(120);
  });

  it("keeps what each of them saved when the sidebar and the workspace both write", async () => {
    const { prefs } = await setup();
    layOut({ sidebar: 214, right: 600 });
    fireEvent.keyDown(screen.getByRole("separator", { name: "サイドバーの幅" }), { key: "ArrowRight" });
    fireEvent.keyDown(screen.getByRole("separator", { name: "右パネルの幅" }), { key: "ArrowLeft" });
    expect(stored(prefs)).toEqual({ sidebar: 230, right: 616 });
  });
});
