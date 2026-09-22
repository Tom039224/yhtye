// Event streams recorded from the real core with fake agents
// (crates/yhtye-core/tests/core_facade.rs, `pnpm record:fixtures`).

import type { ApiEvent, ProjectInfo, Snapshot } from "../api/generated";
import cancelRun from "./fixtures/fake-cancel.json";
import fullRun from "./fixtures/fake-run.json";

export interface Recording {
  start: Snapshot;
  events: ApiEvent[];
  end: Snapshot;
}

// JSON imports are typed structurally from the file; the recording is produced
// by serializing the Rust types the generated TypeScript types describe.
export const FULL_RUN = fullRun as unknown as Recording;
export const CANCEL_RUN = cancelRun as unknown as Recording;

export const PROJECT: ProjectInfo = { id: "repo", name: "repo", path: "/tmp/repo", open: true };

export function durable(r: Recording): ApiEvent[] {
  return r.events.filter((e) => !e.live);
}
