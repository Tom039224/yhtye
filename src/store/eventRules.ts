// Which core events change what: rules the app store's sync uses
// (core-design.md §2, §11, T-65).

import type { ApiEvent } from "../api/generated";
import { isOrchestratorKey } from "./chats";
import type { SettingsChange } from "./settingsCommands";

/** "Anything may have changed" for the settings screens. */
export const EVERYTHING: SettingsChange = { kind: null, project: null, revision: null };

/** Events of the whole app rather than of a project (`ApiEvent::app_wide` of the core). */
export function isAppWide(ev: ApiEvent): boolean {
  switch (ev.body.type) {
    case "projects_changed":
    case "settings_changed":
    case "resync":
    case "remote_status_changed":
    case "remote_devices_changed":
      return true;
    default:
      return false;
  }
}

/**
 * An orchestrator finished a turn: it may have renamed or switched a branch
 * itself (Stage 8e: the tree shows what the worktrees have checked out now).
 */
export function endsOrchestratorTurn(ev: ApiEvent): boolean {
  return ev.body.type === "agent" && ev.body.event.type === "turn_ended" && isOrchestratorKey(ev.body.session);
}

/** Durable domain events after which branches or commits may have changed. */
export function changesGit(ev: ApiEvent): boolean {
  if (ev.body.type !== "domain") return false;
  switch (ev.body.event.type) {
    case "chat_created":
    case "group_created":
    case "group_base_changed":
    case "group_merge_finished":
    case "group_cancelled":
    case "workspace_ready":
    case "task_status_changed":
    case "task_cancelled":
    case "step_completed":
      return true;
    default:
      return false;
  }
}

/** `promise`, or a rejection if it takes longer than `ms`. */
export function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`no answer within ${ms} ms`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}
