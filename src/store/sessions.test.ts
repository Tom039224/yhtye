// The sessions table as the core derives it from session events: a start that
// could not restore its session records the lost ACP session id, and a later
// start that lost nothing keeps it.

import { describe, expect, it } from "vitest";

import type { ApiEventBody } from "../api/generated";
import { applySessionEvent } from "./sessions";

const KEY = "T-1/implementer";

function started(acp: string, lost?: string): ApiEventBody {
  return {
    type: "session_started",
    session: KEY,
    role: "implementer",
    task: "T-1",
    pid: 1,
    acp_session_id: acp,
    resumed: false,
    agent: null,
    ...(lost ? { lost_acp_session_id: lost } : {}),
  };
}

describe("a session that could not be restored", () => {
  it("records the lost ACP session next to the new one", () => {
    const first = applySessionEvent([], started("acp-1"));
    expect(first[0]).not.toHaveProperty("lost_acp_session_id");

    const replaced = applySessionEvent(first, started("acp-2", "acp-1"));
    expect(replaced).toHaveLength(1);
    expect(replaced[0]).toMatchObject({ acp_session_id: "acp-2", lost_acp_session_id: "acp-1" });
  });

  it("keeps it over a later start that lost nothing, until another is lost", () => {
    let sessions = applySessionEvent([], started("acp-2", "acp-1"));
    sessions = applySessionEvent(sessions, started("acp-2"));
    expect(sessions[0].lost_acp_session_id).toBe("acp-1");

    sessions = applySessionEvent(sessions, started("acp-3", "acp-2"));
    expect(sessions[0]).toMatchObject({ acp_session_id: "acp-3", lost_acp_session_id: "acp-2" });
  });
});
