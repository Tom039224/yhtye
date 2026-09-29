// The settings modal's "秘密の環境変数" section (Stage 7e): names only, values
// go to the core once and are never shown afterwards.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { secretNameError } from "./SecretEnvSection";

const DUMMY = "dummy-secret-value-123";

async function setup(configure?: (core: FakeCore) => void) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.attach(transport);
  configure?.(core);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "設定" }));
  const dialog = await screen.findByRole("dialog", { name: "設定" });
  await user.click(within(dialog).getByRole("button", { name: "秘密の環境変数" }));
  return { user, dialog, core };
}

async function register(user: ReturnType<typeof userEvent.setup>, dialog: HTMLElement, name: string, value: string) {
  const nameBox = within(dialog).getByRole("textbox", { name: "変数名" });
  await user.clear(nameBox);
  await user.type(nameBox, name);
  await user.type(within(dialog).getByLabelText("値"), value);
  await user.click(within(dialog).getByRole("button", { name: "登録" }));
}

describe("secret environment variables", () => {
  it("validates names like environment variables", () => {
    expect(secretNameError("OPENROUTER_API_KEY_CODEX")).toBeNull();
    expect(secretNameError("_x1")).toBeNull();
    expect(secretNameError("")).not.toBeNull();
    expect(secretNameError("1A")).not.toBeNull();
    expect(secretNameError("A-B")).not.toBeNull();
    expect(secretNameError("A".repeat(129))).not.toBeNull();
  });

  it("lists registered names as 登録済み", async () => {
    const { dialog } = await setup((c) => (c.secretNames = ["A_KEY", "B_KEY"]));
    const list = await within(dialog).findByRole("list", { name: "登録済みの環境変数" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => within(r).getByText(/_KEY/).textContent)).toEqual(["A_KEY", "B_KEY"]);
    expect(within(rows[0]).getByText("登録済み")).toBeTruthy();
  });

  it("registers a variable, clears the form and never shows the value", async () => {
    const { user, dialog, core } = await setup();
    await register(user, dialog, "YHTYE_TEST_SECRET", DUMMY);
    const list = await within(dialog).findByRole("list", { name: "登録済みの環境変数" });
    expect(within(list).getByText("YHTYE_TEST_SECRET")).toBeTruthy();
    expect(core.secretValues.YHTYE_TEST_SECRET).toBe(DUMMY);
    expect((within(dialog).getByLabelText("値") as HTMLInputElement).value).toBe("");
    expect((within(dialog).getByRole("textbox", { name: "変数名" }) as HTMLInputElement).value).toBe("");
    expect(within(dialog).getByLabelText("値")).toHaveProperty("type", "password");
    expect(document.body.innerHTML).not.toContain(DUMMY);
  });

  it("rejects a bad name or an empty value without calling the core", async () => {
    const { user, dialog, core } = await setup();
    await register(user, dialog, "1bad", DUMMY);
    expect((await within(dialog).findByRole("alert")).textContent).toContain("数字から始められません");
    expect(core.secretNames).toEqual([]);
    const nameBox = within(dialog).getByRole("textbox", { name: "変数名" });
    await user.clear(nameBox);
    await user.type(nameBox, "GOOD_NAME");
    await user.clear(within(dialog).getByLabelText("値"));
    await user.click(within(dialog).getByRole("button", { name: "登録" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("値を入力");
    expect(core.secretNames).toEqual([]);
  });

  it("overwrites: 上書き prefills only the name", async () => {
    const { user, dialog, core } = await setup((c) => {
      c.secretNames = ["A_KEY"];
      c.secretValues = { A_KEY: "old" };
    });
    await user.click(await within(dialog).findByRole("button", { name: "A_KEY を上書き" }));
    expect((within(dialog).getByRole("textbox", { name: "変数名" }) as HTMLInputElement).value).toBe("A_KEY");
    expect((within(dialog).getByLabelText("値") as HTMLInputElement).value).toBe("");
    await user.type(within(dialog).getByLabelText("値"), DUMMY);
    await user.click(within(dialog).getByRole("button", { name: "登録" }));
    await waitFor(() => expect(core.secretValues.A_KEY).toBe(DUMMY));
    expect(core.secretNames).toEqual(["A_KEY"]);
    expect(document.body.innerHTML).not.toContain(DUMMY);
  });

  it("deletes after a confirmation", async () => {
    const { user, dialog, core } = await setup((c) => (c.secretNames = ["A_KEY"]));
    await user.click(await within(dialog).findByRole("button", { name: "A_KEY を削除" }));
    expect(core.secretNames).toEqual(["A_KEY"]);
    await user.click(within(dialog).getByRole("button", { name: "やめる" }));
    expect(core.secretNames).toEqual(["A_KEY"]);
    await user.click(within(dialog).getByRole("button", { name: "A_KEY を削除" }));
    await user.click(within(dialog).getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(core.secretNames).toEqual([]));
    expect(await within(dialog).findByText("登録されている変数はありません。")).toBeTruthy();
  });

  it("shows a keyring failure inline and keeps the form usable", async () => {
    const { user, dialog } = await setup((c) => (c.secretError = "the OS credential store is unavailable: no Secret Service"));
    await register(user, dialog, "K", DUMMY);
    const alert = await within(dialog).findByRole("alert");
    expect(alert.textContent).toContain("no Secret Service");
    expect(document.body.innerHTML).not.toContain(DUMMY);
    expect(within(dialog).getByRole("button", { name: "登録" })).toHaveProperty("disabled", false);
  });
});
