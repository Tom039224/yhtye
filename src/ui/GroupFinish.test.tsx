// Stage 7a bug: a group whose work is finished must not look "in progress".
// Driven by a stream recorded from the real core (fake agents): the
// orchestrator reports to the user after `group_settled` without calling
// `finish_group`; Yhtye reminds it, then finishes the group itself.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { ApiEvent, Group, Snapshot, State } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { ev } from "../test/events";
import { durable, PROJECT, UNFINISHED_RUN } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

const LOG = durable(UNFINISHED_RUN);
const END = UNFINISHED_RUN.end;
const WAITING_FOR_OTHERS = "同グループの他タスク完了を待機 — 送信保留";

function isDomain(e: ApiEvent, type: string): boolean {
  return e.body.type === "domain" && e.body.event.type === type;
}

/** Events before the reminder: every task is done, the group is still open. */
const BEFORE_REMINDER = LOG.slice(0, LOG.findIndex((e) => isDomain(e, "group_finish_reminded")));

function setup(snapshot: Snapshot, log: ApiEvent[]) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, snapshot);
  core.log = log;
  core.attach(transport);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  return { transport, core, store };
}

async function open(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
}

function withGroup(patch: Partial<Group>): Snapshot {
  const state: State = { ...END.state, groups: END.state.groups.map((g) => ({ ...g, ...patch })) };
  return { ...END, state };
}

function expectFinishedLook() {
  const card = screen.getByRole("article", { name: "task T-1" });
  expect(within(card).queryByTitle(WAITING_FOR_OTHERS)).not.toBeInTheDocument();
  expect(within(card).queryByText(/待ち/)).not.toBeInTheDocument();
  expect(screen.queryByTestId("wait-banner")).not.toBeInTheDocument();
}

describe("a finished group", () => {
  it("the recording is the reported case: the orchestrator's settled turn did not finish the group", () => {
    expect(BEFORE_REMINDER.length).toBeGreaterThan(0);
    expect(BEFORE_REMINDER.some((e) => isDomain(e, "group_finishing"))).toBe(false);
    expect(LOG.some((e) => e.body.type === "tool_called" && e.body.record.tool === "finish_group")).toBe(false);
    expect(END.state.groups[0]).toMatchObject({ status: "done", finish_nudges: 1 });
  });

  it("says the tasks are done and the orchestrator must finish the group, not that other tasks are pending", async () => {
    const { store } = setup(UNFINISHED_RUN.start, BEFORE_REMINDER);
    await open(store);
    const card = await screen.findByRole("article", { name: "task T-1" });
    expect(within(card).queryByTitle(WAITING_FOR_OTHERS)).not.toBeInTheDocument();
    // Short text on the card, the sentence in its tooltip.
    expect(within(card).getByTitle("全タスク完了 — オーケストレータのグループ完了 (マージ) 待ち")).toHaveTextContent("グループの完了待ち");
    expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("完了待ち");
    expect(screen.getByTestId("wait-banner")).toHaveAccessibleName("グループ「greeting」の全タスクが完了 — オーケストレータがグループを完了 (マージ) するのを待っています");
  });

  it("shows the group as done once Yhtye finished it (live stream)", async () => {
    const { store, transport, core } = setup(UNFINISHED_RUN.start, BEFORE_REMINDER);
    await open(store);
    await screen.findByRole("article", { name: "task T-1" });
    const shown = BEFORE_REMINDER[BEFORE_REMINDER.length - 1].seq;
    core.log = LOG;
    act(() => {
      for (const e of UNFINISHED_RUN.events.filter((x) => x.seq > shown)) transport.emit(e);
    });
    await waitFor(() => expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("完了"));
    expect(screen.getByTestId("group-status-G-1")).not.toHaveTextContent("待ち");
    expectFinishedLook();
  });

  it("renders the same after a reload (snapshot at the end, nothing to catch up)", async () => {
    const { store } = setup(END, LOG);
    await open(store);
    await screen.findByRole("article", { name: "task T-1" });
    expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("完了");
    expectFinishedLook();
  });

  it("renders the same when catching up the whole log from the start", async () => {
    const { store } = setup(UNFINISHED_RUN.start, LOG);
    await open(store);
    await screen.findByRole("article", { name: "task T-1" });
    expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("完了");
    expectFinishedLook();
  });

  it("a cancelled group is shown as cancelled without wait lines", async () => {
    const { store } = setup(withGroup({ status: "cancelled", detail: "user" }), LOG);
    await open(store);
    await screen.findByRole("article", { name: "task T-1" });
    expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("中止");
    expectFinishedLook();
  });

  it("a merge_blocked group shows the alert, then done after the retry succeeds", async () => {
    const { store, transport, core } = setup(withGroup({ status: "merge_blocked", detail: "uncommitted changes in a.txt" }), LOG);
    await open(store);
    await screen.findByRole("article", { name: "task T-1" });
    expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("マージ保留");
    expect(screen.getByRole("alert")).toHaveTextContent("uncommitted changes in a.txt");
    expectFinishedLook();

    const seq = END.seq;
    const finishing = ev(seq + 1, { type: "domain", event: { type: "group_finishing", group: "G-1", summary: "s" } });
    const merged = ev(seq + 2, { type: "domain", event: { type: "group_merge_finished", group: "G-1", ok: true, detail: "merged" } });
    core.log = [...LOG, finishing, merged];
    act(() => transport.emit(finishing));
    await waitFor(() => expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("マージ中"));
    expect(screen.getByTestId("wait-banner")).toHaveAccessibleName("グループ「greeting」を base ブランチへマージ中");
    act(() => transport.emit(merged));
    await waitFor(() => expect(screen.getByTestId("group-status-G-1")).toHaveTextContent("完了"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expectFinishedLook();
  });
});
