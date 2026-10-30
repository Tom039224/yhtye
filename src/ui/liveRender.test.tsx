// What a live event renders. Chunks come many times a second, from every session
// of the open project; the screen must render again only where it shows their
// text, however long the conversation is.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import { Profiler } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { chunk, ORCHESTRATOR } from "../test/events";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

// `formatTime` runs in the render of a transcript item with a time: how often items render.
vi.mock("./labels", async (original) => {
  const labels = await original<typeof import("./labels")>();
  return { ...labels, formatTime: vi.fn(labels.formatTime) };
});
// `taskElapsedMs` runs in the render of a task card, `buildTree` in the render of the branch tree.
vi.mock("./taskInfo", async (original) => {
  const info = await original<typeof import("./taskInfo")>();
  return { ...info, taskElapsedMs: vi.fn(info.taskElapsedMs) };
});
vi.mock("./branchTree", async (original) => {
  const tree = await original<typeof import("./branchTree")>();
  return { ...tree, buildTree: vi.fn(tree.buildTree) };
});
import { buildTree } from "./branchTree";
import { formatTime } from "./labels";
import { taskElapsedMs } from "./taskInfo";

const LOG = durable(FULL_RUN);
const LAST = LOG[LOG.length - 1].seq;
const OTHER = "T-1/implementer";

let transport: MemoryTransport;
let store: AppStore;
const commits = vi.fn();
/** How many times `formatTime` ran while the project loaded: the items rendered, to know the counter counts. */
let itemRenders = 0;

async function setup() {
  transport = new MemoryTransport({ state: "open" });
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.log = LOG;
  core.attach(transport);
  // Frames are run by hand (`store.flushLive()`).
  store = new AppStore(transport, { liveScheduler: () => () => undefined });
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <Profiler id="app" onRender={commits}>
        <App initialView="work" />
      </Profiler>
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  await within(screen.getByRole("region", { name: "orchestrator" })).findByText(/^I will create a group for this\./);
  // Everything that was asked while loading (git, usage, ping) has been answered.
  await act(async () => void (await new Promise((r) => setTimeout(r, 100))));
  await waitFor(() => expect(store.getState().git?.loading).toBe(false));
}

/** A frame in which `pieces` of `session` were streamed. */
function stream(session: string, ...pieces: string[]) {
  act(() => {
    for (const piece of pieces) transport.emit(chunk(LAST, session, piece));
    store.flushLive();
  });
}

beforeEach(async () => {
  await setup();
  itemRenders = vi.mocked(formatTime).mock.calls.length;
  commits.mockClear();
  vi.mocked(formatTime).mockClear();
  vi.mocked(taskElapsedMs).mockClear();
  vi.mocked(buildTree).mockClear();
});

describe("a live chunk", () => {
  it("of a session that is not on screen renders nothing at all", () => {
    // The task's card shows the first line of the text: it is drawn once for the first chunk.
    stream(OTHER, "Working on ");
    commits.mockClear();
    vi.mocked(formatTime).mockClear();

    stream(OTHER, "the ", "change ", "now");
    stream(OTHER, ", and then on the rest of it");

    expect(commits).not.toHaveBeenCalled();
    expect(formatTime).not.toHaveBeenCalled(); // no transcript item rendered again
  });

  it("of a session that is not on screen is kept for the output panel that opens later", () => {
    stream(OTHER, "kept ", "text");
    expect(store.getState().live.streaming[OTHER]?.message).toBe("kept text");
  });

  it("of the chat on screen shows its text and renders none of the items again", () => {
    const conversation = screen.getByRole("region", { name: "orchestrator" });
    expect(itemRenders).toBeGreaterThan(0);

    stream(ORCHESTRATOR, "Streaming ", "now");
    expect(within(conversation).getByTestId("streaming")).toHaveTextContent("Streaming now");
    stream(ORCHESTRATOR, " and more");
    expect(within(conversation).getByTestId("streaming")).toHaveTextContent("Streaming now and more");
    // Only the streamed text was drawn: the stored items, each with a time, were not.
    expect(formatTime).not.toHaveBeenCalled();
  });

  it("of the chat on screen renders the conversation, not the task cards or the branch tree around it", () => {
    stream(ORCHESTRATOR, "first");
    stream(ORCHESTRATOR, " second");
    expect(commits).toHaveBeenCalled();
    expect(taskElapsedMs).not.toHaveBeenCalled();
    expect(buildTree).not.toHaveBeenCalled();
  });

  it("of a session whose card shows the first line renders the card again only when that line changes", () => {
    const card = () => vi.mocked(taskElapsedMs).mock.calls.length;
    // The newest session of T-1 is the one its card describes.
    const newest = store.getState().project?.sessions.map((x) => x.session_key).filter((k) => k.startsWith("T-1/")).at(-1) ?? OTHER;
    stream(newest, "First line");
    const afterFirst = card();
    stream(newest, " goes on");
    expect(card()).toBeGreaterThan(afterFirst); // the line grew
    const afterGrowing = card();
    stream(newest, "\nsecond line");
    stream(newest, " goes on\nand a third");
    expect(card()).toBe(afterGrowing); // the first line is the same, though the text is not
  });
});
