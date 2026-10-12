// What the sidebar's host footer shows (design §3.3): the machine the core runs
// on, how long ago the core last answered a ping, and the paired devices.

import type { ClientDevice, ClientKind } from "../api/generated";
import type { ConnectionStatus } from "../api/transport";
import { formatDateTime } from "./labels";

/** A ping this recent, or newer, reads "now". */
export const PING_NOW_SECONDS = 5;
/** Without an answer for this long, an open connection is called unresponsive. */
export const PING_STALE_SECONDS = 12;

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Whole seconds since the last ping answer (`null`: none yet; never negative). */
export function pingAgeSeconds(nowMs: number, lastPingAt: number | null): number | null {
  return lastPingAt === null ? null : Math.max(0, Math.round((nowMs - lastPingAt) / 1000));
}

/** `now` up to 5 s, then `12s` / `3m` / `2h` / `4d`. */
export function formatPingAge(seconds: number): string {
  if (seconds <= PING_NOW_SECONDS) return "now";
  if (seconds < MINUTE) return `${seconds}s`;
  if (seconds < HOUR) return `${Math.floor(seconds / MINUTE)}m`;
  if (seconds < DAY) return `${Math.floor(seconds / HOUR)}h`;
  return `${Math.floor(seconds / DAY)}d`;
}

export type HostTone = "ok" | "busy" | "bad";

/**
 * The colour of the dot: green while the core answers, amber while connecting
 * or before the first answer, red when the connection is lost or an open one
 * has not been answered for a while.
 */
export function hostTone(connection: ConnectionStatus["state"], ageSeconds: number | null): HostTone {
  if (connection === "closed") return "bad";
  if (connection === "connecting" || ageSeconds === null) return "busy";
  return ageSeconds > PING_STALE_SECONDS ? "bad" : "ok";
}

/** The dot's state in words (its tooltip and accessible name). */
export function hostToneLabel(tone: HostTone, connection: ConnectionStatus["state"]): string {
  if (tone === "ok") return "接続中";
  if (connection === "closed") return "切断";
  if (connection === "open" && tone === "bad") return "応答なし";
  return "接続を確認中";
}

/** Devices shown under the host before the rest are folded into "他 N 台". */
export const CLIENTS_SHOWN = 3;

/** Connected devices first, then the most recently seen; the core's order otherwise. */
export function orderClients(devices: readonly ClientDevice[]): ClientDevice[] {
  const seen = (d: ClientDevice) => d.last_seen_ms ?? -1;
  return devices
    .map((d, i) => ({ d, i }))
    .sort((a, b) => Number(b.d.connected) - Number(a.d.connected) || seen(b.d) - seen(a.d) || a.i - b.i)
    .map(({ d }) => d);
}

const KIND_LABELS: Record<ClientKind, string> = { android: "Android" };

/** What kind of client a device is, in words. */
export function clientKindLabel(kind: ClientKind): string {
  return KIND_LABELS[kind] ?? kind;
}

/** A device's state in words (its accessible name and tooltip). */
export function clientState(d: ClientDevice): string {
  if (d.connected) return "接続中";
  return d.last_seen_ms === null ? "オフライン · 未接続" : `オフライン · 最終接続 ${formatDateTime(d.last_seen_ms)}`;
}
