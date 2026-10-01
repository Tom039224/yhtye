// A chat's row in the BRANCHES tree: the "⋯" menu, renaming in place and
// deleting after a confirmation (keyboard and aria included).

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { CHAT_GIT, CHAT_LOG, chatSnapshot } from "../test/chats";
import { ev } from "../test/events";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

async function setup() {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, chatSnapshot());
  core.log = CHAT_LOG;
  core.git = CHAT_GIT;
  core.attach(transport);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
  await screen.findByRole("list", { name: "branches" });
  await waitFor(() => expect(store.getState().git?.overview).not.toBeNull());
  return { transport, core, store, user: userEvent.setup() };
}

const tree = () => screen.getByRole("list", { name: "branches" });
/** The "⋯" button of a chat. */
const more = (title: string) => within(tree()).getByRole("button", { name: `${title} の操作` });
/** A chat's row button (not its "⋯"). */
const row = (title: RegExp) =>
  within(tree()).getByRole("button", { name: (name) => !name.endsWith("の操作") && title.test(name) });
const menu = (title: string) => screen.getByRole("menu", { name: `${title} の操作` });
const chatIds = (store: AppStore) => store.getState().project?.chats.map((c) => c.id);

describe("the tree's shape", () => {
  it("hangs a branch's chats in their own indented list under it", async () => {
    await setup();
    const chats = within(tree()).getByRole("list", { name: "chats of main" });
    expect(chats).toHaveClass("chat-list");
    expect(within(chats).getByRole("button", { name: "Refactor checkout の操作" })).toBeInTheDocument();
    expect(within(chats).getAllByRole("button")).toHaveLength(2);
    // The branch row itself is not inside the chats list.
    expect(within(chats).queryByRole("button", { name: /^main/ })).not.toBeInTheDocument();
  });
});

describe("the ⋯ menu", () => {
  it("is a button named after the chat that opens a menu of rename and delete", async () => {
    const { user } = await setup();
    const button = more("Refactor checkout");
    expect(button).toHaveAttribute("aria-haspopup", "menu");
    expect(button).toHaveAttribute("aria-expanded", "false");
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    const items = within(menu("Refactor checkout")).getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(["名前を変更", "削除"]);
    expect(items[0]).toHaveFocus();
  });

  it("moves with the arrow keys, Home and End, and closes with Esc, returning focus to the button", async () => {
    const { user } = await setup();
    const button = more("Refactor checkout");
    button.focus();
    await user.keyboard("{ArrowDown}");
    const [rename, del] = within(menu("Refactor checkout")).getAllByRole("menuitem");
    expect(rename).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(del).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(rename).toHaveFocus();
    await user.keyboard("{End}");
    expect(del).toHaveFocus();
    await user.keyboard("{Home}");
    expect(rename).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
    expect(button).toHaveAttribute("aria-expanded", "false");
  });

  it("opens from the context menu of the row, and closes on a click elsewhere or on the button again", async () => {
    const { user } = await setup();
    fireEvent.contextMenu(row(/Refactor checkout/));
    expect(menu("Refactor checkout")).toBeInTheDocument();
    await user.click(document.body);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await user.click(more("Refactor checkout"));
    await user.click(more("Refactor checkout"));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("does not select the chat", async () => {
    const { store, user } = await setup();
    expect(store.getState().project?.selectedChat).toBe("C-2");
    await user.click(more("Refactor checkout"));
    expect(store.getState().project?.selectedChat).toBe("C-2");
  });
});

describe("renaming in place", () => {
  async function startRename(user: ReturnType<typeof userEvent.setup>, title = "Refactor checkout") {
    await user.click(more(title));
    await user.click(screen.getByRole("menuitem", { name: "名前を変更" }));
    return screen.getByRole("textbox", { name: "チャットの名前" });
  }

  it("shows the title in an input, all selected; Enter saves it and focus goes back to the row", async () => {
    const { transport, user } = await setup();
    const input = await startRename(user);
    expect(input).toHaveValue("Refactor checkout");
    expect(input).toHaveFocus();
    await user.clear(input);
    await user.type(input, "Checkout v2{Enter}");
    expect(transport.callsOf("rename_chat")).toEqual([
      { type: "rename_chat", project: "repo", chat: "C-1", title: "Checkout v2" },
    ]);
    expect(screen.queryByRole("textbox", { name: "チャットの名前" })).not.toBeInTheDocument();
    expect(row(/Checkout v2/)).toHaveFocus();
  });

  it("cancels with Esc without asking the core", async () => {
    const { transport, user } = await setup();
    const input = await startRename(user);
    await user.type(input, " more{Escape}");
    expect(transport.callsOf("rename_chat")).toHaveLength(0);
    expect(row(/Refactor checkout/)).toHaveFocus();
    expect(within(tree()).queryByText(/more/)).not.toBeInTheDocument();
  });

  it("saves a changed title when the input loses focus, and closes an unchanged one without a call", async () => {
    const { transport, user } = await setup();
    let input = await startRename(user);
    await user.click(document.body);
    expect(transport.callsOf("rename_chat")).toHaveLength(0);
    expect(screen.queryByRole("textbox", { name: "チャットの名前" })).not.toBeInTheDocument();
    input = await startRename(user);
    await user.type(input, "!");
    await user.click(document.body);
    await waitFor(() => expect(transport.callsOf("rename_chat")).toHaveLength(1));
    await waitFor(() => expect(row(/Refactor checkout!/)).toBeInTheDocument());
  });

  it("does not save on the Enter that confirms an IME composition", async () => {
    const { transport, user } = await setup();
    const input = await startRename(user);
    fireEvent.change(input, { target: { value: "にほんご" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(transport.callsOf("rename_chat")).toHaveLength(0);
    expect(input).toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(transport.callsOf("rename_chat")).toHaveLength(1));
  });

  it("asks for a name instead of saving an empty one", async () => {
    const { transport, user } = await setup();
    const input = await startRename(user);
    await user.clear(input);
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("名前を入力してください");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(transport.callsOf("rename_chat")).toHaveLength(0);
    // Esc still leaves.
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("textbox", { name: "チャットの名前" })).not.toBeInTheDocument();
  });

  it("keeps the input open with the core's refusal", async () => {
    const { core, user } = await setup();
    const input = await startRename(user);
    core.failures.set("rename_chat", new CommandError("invalid_argument", "the title is longer than 80 characters"));
    await user.type(input, "!{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("longer than 80");
    expect(screen.getByRole("textbox", { name: "チャットの名前" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "チャットの名前" })).toHaveFocus();
  });

  it("limits the title to what the core accepts", async () => {
    const { user } = await setup();
    const input = await startRename(user);
    expect(input).toHaveAttribute("maxlength", "80");
  });
});

describe("deleting with a confirmation", () => {
  async function startDelete(user: ReturnType<typeof userEvent.setup>, title: string) {
    await user.click(more(title));
    await user.click(screen.getByRole("menuitem", { name: "削除" }));
    return screen.getByRole("group", { name: "チャットの削除" });
  }

  it("asks first, focusing the safe button, and deletes only when confirmed", async () => {
    const { store, transport, user } = await setup();
    const confirm = await startDelete(user, "Refactor checkout");
    expect(confirm).toHaveTextContent("Refactor checkout");
    expect(confirm).toHaveTextContent("チャットを削除しますか?");
    expect(confirm).toHaveTextContent("作業ツリーとブランチは残ります");
    expect(within(confirm).getByRole("button", { name: "キャンセル" })).toHaveFocus();
    expect(transport.callsOf("delete_chat")).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "削除する" }));
    expect(transport.callsOf("delete_chat")).toEqual([{ type: "delete_chat", project: "repo", chat: "C-1" }]);
    await waitFor(() => expect(chatIds(store)).toEqual(["C-2"]));
    expect(within(tree()).queryByText("Refactor checkout")).not.toBeInTheDocument();
  });

  it("cancels with the button or Esc, and focus returns to the ⋯ button", async () => {
    const { transport, store, user } = await setup();
    let confirm = await startDelete(user, "Refactor checkout");
    await user.click(within(confirm).getByRole("button", { name: "キャンセル" }));
    expect(screen.queryByRole("group", { name: "チャットの削除" })).not.toBeInTheDocument();
    expect(more("Refactor checkout")).toHaveFocus();
    confirm = await startDelete(user, "Refactor checkout");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "チャットの削除" })).not.toBeInTheDocument();
    expect(more("Refactor checkout")).toHaveFocus();
    expect(transport.callsOf("delete_chat")).toHaveLength(0);
    expect(chatIds(store)).toEqual(["C-2", "C-1"]);
  });

  it("shows another chat when the chat on screen is deleted", async () => {
    const { store, transport, user } = await setup();
    expect(store.getState().project?.selectedChat).toBe("C-2");
    // C-2's group finishes, so nothing keeps the chat.
    act(() => transport.emit(ev(5, { type: "domain", event: { type: "group_merge_finished", group: "G-2", ok: true, detail: "merged" } })));
    const confirm = await startDelete(user, "Add feature x");
    await user.click(within(confirm).getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(chatIds(store)).toEqual(["C-1"]));
    expect(store.getState().project?.selectedChat).toBe("C-1");
    expect(within(screen.getByRole("region", { name: "orchestrator" })).getByText("Reply about checkout")).toBeInTheDocument();
  });

  it("says why a chat with an unfinished group cannot be deleted, and offers no delete", async () => {
    const { transport, user } = await setup();
    const confirm = await startDelete(user, "Add feature x");
    expect(within(confirm).getByRole("alert")).toHaveTextContent("G-2");
    expect(within(confirm).queryByRole("button", { name: "削除する" })).not.toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("group", { name: "チャットの削除" })).not.toBeInTheDocument();
    expect(transport.callsOf("delete_chat")).toHaveLength(0);
  });

  it("shows the core's refusal and keeps the chat", async () => {
    const { core, store, user } = await setup();
    const confirm = await startDelete(user, "Refactor checkout");
    core.failures.set("delete_chat", new CommandError("invalid_state", "the orchestrator of chat C-1 is working"));
    await user.click(within(confirm).getByRole("button", { name: "削除する" }));
    expect(await within(confirm).findByRole("alert")).toHaveTextContent("is working");
    expect(chatIds(store)).toEqual(["C-2", "C-1"]);
    expect(within(confirm).getByRole("button", { name: "削除する" })).toBeEnabled();
  });
});
