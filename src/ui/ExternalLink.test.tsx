// Links that leave the app: through the Tauri opener inside it, in a new tab elsewhere.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ExternalLink, openExternal } from "./ExternalLink";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  opener.openUrl.mockReset();
  vi.restoreAllMocks();
});

describe("ExternalLink", () => {
  it("is a link that opens in a new tab and keeps the page from the opened one", () => {
    render(<ExternalLink href="https://example.com/docs">docs</ExternalLink>);
    const link = screen.getByRole("link", { name: "docs" });
    expect(link).toHaveAttribute("href", "https://example.com/docs");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
  });

  it("opens a new window instead of following the link outside the app", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    render(<ExternalLink href="https://example.com/docs">docs</ExternalLink>);
    await userEvent.setup().click(screen.getByRole("link", { name: "docs" }));
    expect(open).toHaveBeenCalledWith("https://example.com/docs", "_blank", "noopener,noreferrer");
  });

  it("hands the address to the system browser inside the app, and never navigates the webview", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    render(<ExternalLink href="https://example.com/docs">docs</ExternalLink>);
    await userEvent.setup().click(screen.getByRole("link", { name: "docs" }));
    await vi.waitFor(() => expect(opener.openUrl).toHaveBeenCalledWith("https://example.com/docs"));
    expect(open).not.toHaveBeenCalled();
  });

  it("does not throw when the opener fails", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    opener.openUrl.mockRejectedValue(new Error("no opener"));
    expect(() => openExternal("https://example.com")).not.toThrow();
    await vi.waitFor(() => expect(opener.openUrl).toHaveBeenCalled());
  });
});
