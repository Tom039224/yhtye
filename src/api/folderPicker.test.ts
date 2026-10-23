import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { canPickLocalFolder, pickLocalFolder } from "./folderPicker";

const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

beforeEach(() => {
  dialog.open.mockReset();
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("canPickLocalFolder", () => {
  it("is false outside the Tauri app", () => {
    expect(canPickLocalFolder(false)).toBe(false);
  });

  it("is true in the Tauri app on the local host", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(canPickLocalFolder(false)).toBe(true);
  });

  it("is false in the Tauri app on a remote host", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(canPickLocalFolder(true)).toBe(false);
  });
});

describe("pickLocalFolder", () => {
  it("asks for one directory with the given title and returns its path", async () => {
    dialog.open.mockResolvedValue("/home/me/work/app");

    await expect(pickLocalFolder("Choose a project folder")).resolves.toBe("/home/me/work/app");
    expect(dialog.open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Choose a project folder",
    });
  });

  it("returns null when the user cancels", async () => {
    dialog.open.mockResolvedValue(null);

    await expect(pickLocalFolder("Choose")).resolves.toBeNull();
  });

  it("passes a dialog failure on", async () => {
    dialog.open.mockRejectedValue(new Error("dialog unavailable"));

    await expect(pickLocalFolder("Choose")).rejects.toThrow("dialog unavailable");
  });
});
