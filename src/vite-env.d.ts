/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** WebSocket URL of the development bridge (default ws://127.0.0.1:1421/ws). */
  readonly VITE_YHTYE_BRIDGE_URL?: string;
  /** Token printed by the bridge on start. */
  readonly VITE_YHTYE_BRIDGE_TOKEN?: string;
}
