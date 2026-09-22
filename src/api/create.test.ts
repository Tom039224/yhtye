import { afterEach, describe, expect, it } from "vitest";

import { bridgeUrl, DEFAULT_BRIDGE_URL, isTauri } from "./create";

describe("transport selection", () => {
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("uses the bridge URL from the environment, with the token", () => {
    expect(bridgeUrl({})).toBe(DEFAULT_BRIDGE_URL);
    expect(bridgeUrl({ VITE_YHTYE_BRIDGE_URL: "ws://127.0.0.1:9/ws", VITE_YHTYE_BRIDGE_TOKEN: "t k" })).toBe(
      "ws://127.0.0.1:9/ws?token=t+k",
    );
  });

  it("detects the Tauri webview", () => {
    expect(isTauri()).toBe(false);
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(isTauri()).toBe(true);
  });
});
