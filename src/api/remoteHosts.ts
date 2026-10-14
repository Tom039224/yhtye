// The other Yhtye hosts this app is a client of (core-design.md §9, §18.9): the
// types the shell reports (written by hand after `yhtye_core::remote_client`;
// `src-tauri/src/remote_hosts/tests.rs` pins the JSON they are read from), how
// a host's connection reads as a store's {@link ConnectionStatus}, and the
// commands and events of the shell that manage the hosts.
//
// The WebSocket to a host is made by the shell, not here: a WebView's
// WebSocket cannot send `Authorization`, the CSP does not allow `ws://100.x`,
// and the device token stays out of the page this way.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { ConnectionStatus } from "./transport";
import { toCommandError } from "./transport";

/** What the connection to a host is doing (`ConnectionState`). */
export type HostConnection =
  | { state: "idle" }
  | { state: "connecting"; attempt: number }
  | { state: "connected"; session: number }
  | { state: "waiting_to_reconnect"; attempt: number; delay_ms: number; reason: string | null }
  /** 401: the host removed this device. Pair again. */
  | { state: "revoked" }
  /** 403: the host does not take this device. */
  | { state: "rejected"; message: string | null };

/** Whether a host's version and this app's fit (`Compatibility`). */
export type HostCompatibility =
  | { type: "compatible" }
  | { type: "incompatible"; host: string; app: string }
  | { type: "unknown" };

/** What the host said about itself after connecting (`ServerInfo`). */
export interface HostServerInfo {
  /** The name of its machine. */
  host: string;
  version: string | null;
  headless: boolean;
  compatibility: HostCompatibility;
}

export interface HostStatus {
  connection: HostConnection;
  /** Known once connected (kept while reconnecting). */
  server: HostServerInfo | null;
}

/** What is known of a host's device token (`TokenState`). */
export type HostToken =
  | { place: "keyring" }
  /** Only in memory: the host must be paired again after a restart. */
  | { place: "memory_only"; reason: string }
  /** Gone (it was memory only, and the app was restarted): pair again. */
  | { place: "missing" };

/** A host paired with, and its live state (`RemoteHostInfo`; nothing in it is secret). */
export interface RemoteHostInfo {
  /** Identifies the host's Yhtye. */
  server_id: string;
  /** The name of its machine once it has told, until then the address it was paired by. */
  name: string;
  hosts: string[];
  port: number;
  device_id: string;
  device_name: string;
  added_ms: number;
  token_in_keyring: boolean;
  token: HostToken;
  status: HostStatus;
}

/** What `yhtye://hosts` reports (`HostNotice`, without the events: those are `yhtye://event`). */
export type HostNotice =
  /** A host was added, removed or renamed: read the list again. */
  | { type: "hosts_changed" }
  | { type: "status"; server_id: string; status: HostStatus };

/** Whom to pair with: a `yhtye://pair?…` link, or what a person typed. */
export type PairingInput =
  | { kind: "uri"; uri: string }
  | { kind: "manual"; host: string; port: string; code: string };

/** What pairing would do, for the person to confirm (never the code). */
export interface PairingPreview {
  hosts: string[];
  port: number;
  /** Known when the input is a QR code / link. */
  server_id: string | null;
  /** The name this device is offered to be listed under on the host. */
  default_device_name: string;
}

export interface RemoveOutcome {
  /** The credential store would not delete the token: a copy may be left there. */
  token_left_in_keyring: string | null;
}

/** Event: a `yhtye://pair?…` link reached the app (its payload is the link). */
export const PAIR_LINK_EVENT = "yhtye://pair-link";
/** Event: the host list or a host's state changed ({@link HostNotice}). */
export const HOSTS_EVENT = "yhtye://hosts";

/**
 * How a host's connection reads as a store's: connected is open; making one is
 * connecting; waiting to reconnect, refused and removed are closed (the first
 * with its retry time). `null` retry means it will not try again by itself.
 */
export function linkStatus(connection: HostConnection): ConnectionStatus {
  switch (connection.state) {
    case "connected":
      return { state: "open" };
    case "connecting":
      return { state: "connecting" };
    case "waiting_to_reconnect":
      return {
        state: "closed",
        reason: connection.reason ?? "接続できません",
        retryInMs: connection.delay_ms,
      };
    case "revoked":
      return {
        state: "closed",
        reason: "この端末は接続先から外されました。ペアリングし直してください",
        retryInMs: null,
      };
    case "rejected":
      return {
        state: "closed",
        reason: connection.message ?? "接続先がこの端末を受け付けませんでした",
        retryInMs: null,
      };
    case "idle":
      return { state: "closed", reason: "接続していません", retryInMs: null };
  }
}

/** The shell's commands and events that manage the hosts. */
export interface HostsApi {
  /** The hosts, with their live state. */
  list(): Promise<RemoteHostInfo[]>;
  /** What pairing with `input` would do; rejects with the reason a link or typed input is no good. */
  previewPairing(input: PairingInput): Promise<PairingPreview>;
  /** Pairs as a device called `deviceName`, remembers the host and connects. */
  add(input: PairingInput, deviceName: string): Promise<RemoteHostInfo>;
  /** Forgets a host (it still lists this device until it is removed there). */
  remove(serverId: string): Promise<RemoveOutcome>;
  /** Connects now (again after being refused, or to skip the wait). */
  connect(serverId: string): Promise<void>;
  /** Resolves, with the way to stop, once the listener is in place. */
  onNotice(handler: (notice: HostNotice) => void): Promise<() => void>;
  /** Resolves once the listener is in place. */
  onPairLink(handler: (uri: string) => void): Promise<() => void>;
  /** Tells the shell the listener of {@link onPairLink} is in place; returns the links that came before. */
  takePairLinks(): Promise<string[]>;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw toCommandError(e);
  }
}

export class TauriHostsApi implements HostsApi {
  list(): Promise<RemoteHostInfo[]> {
    return call("yhtye_remote_hosts");
  }

  previewPairing(input: PairingInput): Promise<PairingPreview> {
    return call("yhtye_remote_preview_pairing", { input });
  }

  add(input: PairingInput, deviceName: string): Promise<RemoteHostInfo> {
    return call("yhtye_remote_add", { input, deviceName });
  }

  remove(serverId: string): Promise<RemoveOutcome> {
    return call("yhtye_remote_remove", { serverId });
  }

  async connect(serverId: string): Promise<void> {
    await call("yhtye_remote_connect", { serverId });
  }

  onNotice(handler: (notice: HostNotice) => void): Promise<() => void> {
    return listen<HostNotice>(HOSTS_EVENT, (e) => handler(e.payload));
  }

  onPairLink(handler: (uri: string) => void): Promise<() => void> {
    return listen<string>(PAIR_LINK_EVENT, (e) => handler(e.payload));
  }

  takePairLinks(): Promise<string[]> {
    return call("yhtye_pair_links_ready");
  }
}
