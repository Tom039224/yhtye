// Picks the transport for the environment (core-design.md §11): Tauri IPC inside
// the app, otherwise the WebSocket bridge (VITE_YHTYE_BRIDGE_URL / _TOKEN).

import { TauriTransport } from "./tauri";
import type { Transport } from "./transport";
import { WsTransport } from "./ws";

// 1420 is Vite, 1421 Vite's HMR under `TAURI_DEV_HOST`; the bridge takes 1422.
export const DEFAULT_BRIDGE_URL = "ws://127.0.0.1:1422/ws";

export interface BridgeEnv {
  VITE_YHTYE_BRIDGE_URL?: string;
  VITE_YHTYE_BRIDGE_TOKEN?: string;
}

export function bridgeUrl(env: BridgeEnv): string {
  const base = env.VITE_YHTYE_BRIDGE_URL || DEFAULT_BRIDGE_URL;
  const token = env.VITE_YHTYE_BRIDGE_TOKEN;
  if (!token) return base;
  const url = new URL(base);
  url.searchParams.set("token", token);
  return url.toString();
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function createTransport(env: BridgeEnv = import.meta.env): Transport {
  return isTauri() ? new TauriTransport() : new WsTransport(bridgeUrl(env));
}
