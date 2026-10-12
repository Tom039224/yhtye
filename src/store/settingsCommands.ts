// The commands of the settings screens (agents, harnesses, secrets, remote
// access): plain requests to the core that reject with a CommandError for the
// screen to show (unlike the app store's own commands, which push app errors),
// and the notices that settings changed. The app store extends it.

import type {
  AgentRole,
  AgentSettingsView,
  ApiCommand,
  ApiEventBody,
  ApiResponse,
  HarnessDetection,
  HarnessModels,
  ModelEfforts,
  PairingCode,
  RemoteDevice,
  RemoteStatus,
  RoleSettings,
  SettingsKind,
} from "../api/generated";
import { Listeners, type Transport } from "../api/transport";

/**
 * Settings some client changed (`settings_changed`), for the settings screen on
 * display to read again. `kind: null`: anything may have changed (events were
 * missed: a reconnect or a `resync`).
 */
export interface SettingsChange {
  kind: SettingsKind | null;
  project: string | null;
  /** The agent settings' new revision (`kind: "agent_settings"`). */
  revision: number | null;
}

/** The remote access events: the server's status, and the paired devices. */
export type RemoteEventBody = Extract<ApiEventBody, { type: "remote_status_changed" | "remote_devices_changed" }>;

type Expect<T extends ApiResponse["type"]> = Extract<ApiResponse, { type: T }>;

export class SettingsCommands {
  /** Notices for the settings screens; the app store emits them from the core's events. */
  protected readonly settingsChanges = new Listeners<SettingsChange>();

  constructor(protected readonly transport: Transport) {}

  // ---- agent settings (Stage 7b) --------------------------------------------
  // Read and written by the settings panel, which shows failures itself
  // (these reject with a CommandError instead of pushing an app error).

  /** The harness × model settings, globally (`null`) or for a project. */
  async getAgentSettings(project: string | null): Promise<AgentSettingsView> {
    const r = await this.invoke({ type: "get_agent_settings", project: project ?? undefined }, "agent_settings");
    return r.settings;
  }

  /**
   * Replaces (or with `null` removes) one role of the global / a project's layer.
   * With `expectedRevision` (the revision shown), rejects with `conflict` if
   * another client changed the settings since.
   */
  async setAgentSettings(
    project: string | null,
    role: AgentRole,
    settings: RoleSettings | null,
    expectedRevision?: number,
  ): Promise<AgentSettingsView> {
    const cmd: ApiCommand = {
      type: "set_agent_settings",
      project: project ?? undefined,
      role,
      settings,
      expected_revision: expectedRevision,
    };
    const r = await this.invoke(cmd, "agent_settings");
    return r.settings;
  }

  /** The models a harness offers (read from it by the core; cached there). */
  async listHarnessModels(harness: string, refresh = false): Promise<HarnessModels> {
    const r = await this.invoke({ type: "list_harness_models", harness, refresh }, "harness_models");
    return r.models;
  }

  /** The efforts of one model whose efforts the model list did not include (Stage 7d). */
  async listModelEfforts(harness: string, model: string): Promise<ModelEfforts> {
    const r = await this.invoke({ type: "list_model_efforts", harness, model }, "model_efforts");
    return r.efforts;
  }

  // ---- harness detection -----------------------------------------------------
  // The core looks for the harnesses' executables again on each of these and
  // registers the ones found, so the agent settings offer them afterwards.
  // Like the agent settings, failures reject for the section to show.

  /** Whether each harness is installed, and where its executables are. */
  async getHarnesses(): Promise<HarnessDetection[]> {
    return (await this.invoke({ type: "get_harnesses" }, "harnesses")).harnesses;
  }

  /** Like `getHarnesses`, and also forgets the models read from every harness. */
  async detectHarnesses(): Promise<HarnessDetection[]> {
    return (await this.invoke({ type: "detect_harnesses" }, "harnesses")).harnesses;
  }

  /** Sets the path of a harness's main executable (`null`: detect it automatically). */
  async setHarnessPath(harness: string, path: string | null): Promise<HarnessDetection[]> {
    return (await this.invoke({ type: "set_harness_path", harness, path }, "harnesses")).harnesses;
  }

  // ---- secret environment variables (Stage 7e) ------------------------------
  // Only names come back; a value goes to the core once and is not kept here.
  // Like the agent settings, failures reject for the section to show.

  /** The registered names, sorted. */
  async listSecretEnv(): Promise<string[]> {
    return (await this.invoke({ type: "list_secret_env" }, "secret_env")).names;
  }

  /** Stores `value` in the OS keyring under `name` (replacing an earlier one). */
  async setSecretEnv(name: string, value: string): Promise<string[]> {
    return (await this.invoke({ type: "set_secret_env", name, value }, "secret_env")).names;
  }

  async deleteSecretEnv(name: string): Promise<string[]> {
    return (await this.invoke({ type: "delete_secret_env", name }, "secret_env")).names;
  }

  // ---- remote access (core-design.md §18) ----------------------------------
  // Local-only commands for the settings' "リモート接続" section. Failures
  // reject for the section to show; a server that fails to start is not a
  // failure but the status's `error`.

  async getRemoteStatus(): Promise<RemoteStatus> {
    return (await this.invoke({ type: "get_remote_status" }, "remote_status")).status;
  }

  /** Turns remote access on or off (`port`: also change it). Enabling it again retries a failed start. */
  async setRemoteSettings(enabled: boolean, port?: number): Promise<RemoteStatus> {
    const cmd: ApiCommand = port === undefined ? { type: "set_remote_settings", enabled } : { type: "set_remote_settings", enabled, port };
    return (await this.invoke(cmd, "remote_status")).status;
  }

  /** A new pairing code (the previous one stops working). Only while the server runs. */
  async createPairingCode(): Promise<PairingCode> {
    return (await this.invoke({ type: "create_pairing_code" }, "pairing_code")).pairing;
  }

  async listRemoteDevices(): Promise<RemoteDevice[]> {
    return (await this.invoke({ type: "list_remote_devices" }, "remote_devices")).devices;
  }

  /** Forgets a paired device; its connections are closed at once. */
  async revokeRemoteDevice(device: string): Promise<RemoteDevice[]> {
    return (await this.invoke({ type: "revoke_remote_device", device }, "remote_devices")).devices;
  }

  /** Tells the settings screens that settings were changed (by any client) or may have been. */
  onSettingsChanged(handler: (change: SettingsChange) => void): () => void {
    return this.settingsChanges.add(handler);
  }

  /** Receives the app-wide remote access events (they belong to no project). */
  onRemoteEvent(handler: (body: RemoteEventBody) => void): () => void {
    return this.transport.subscribe((ev) => {
      if (ev.body.type === "remote_status_changed" || ev.body.type === "remote_devices_changed") handler(ev.body);
    });
  }

  protected async invoke<T extends ApiResponse["type"]>(cmd: ApiCommand, expect: T): Promise<Expect<T>> {
    const r = await this.transport.invoke(cmd);
    if (r.type !== expect) {
      throw new Error(`unexpected reply to ${cmd.type}: ${r.type}`);
    }
    return r as Expect<T>;
  }
}
