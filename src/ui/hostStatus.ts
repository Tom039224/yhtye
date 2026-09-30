// What the sidebar's host footer shows (design §3.3): the machine the core runs
// on, and how long ago the core last answered a ping.

import type { ConnectionStatus } from "../api/transport";

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
