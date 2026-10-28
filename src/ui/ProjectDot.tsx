import type { Ring } from "./branchTree";

/**
 * A project's status dot (the rail, the dashboard): lit green while work is on
 * in the project (`running`), hollow and neutral otherwise (awake and idle, or
 * asleep). The three-way state is kept in `data-ring` for styling and tests.
 */
export function ProjectDot({ ring, testId }: { ring: Ring; testId?: string }) {
  return <span className={`project-dot project-dot-${ring === "running" ? "running" : "idle"}`} data-ring={ring} data-testid={testId} aria-hidden="true" />;
}
