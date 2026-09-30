import { describe, expect, it } from "vitest";

import { formatPingAge, hostTone, hostToneLabel, pingAgeSeconds } from "./hostStatus";

describe("formatPingAge", () => {
  it("says now for 5 seconds or less", () => {
    expect([0, 1, 4, 5].map(formatPingAge)).toEqual(["now", "now", "now", "now"]);
  });

  it("counts seconds, then minutes, hours and days", () => {
    expect(formatPingAge(6)).toBe("6s");
    expect(formatPingAge(59)).toBe("59s");
    expect(formatPingAge(60)).toBe("1m");
    expect(formatPingAge(3599)).toBe("59m");
    expect(formatPingAge(3600)).toBe("1h");
    expect(formatPingAge(86_399)).toBe("23h");
    expect(formatPingAge(86_400)).toBe("1d");
  });
});

describe("pingAgeSeconds", () => {
  it("is the rounded time since the last answer, never negative", () => {
    expect(pingAgeSeconds(10_000, null)).toBeNull();
    expect(pingAgeSeconds(10_000, 4_400)).toBe(6);
    expect(pingAgeSeconds(10_000, 4_600)).toBe(5);
    expect(pingAgeSeconds(10_000, 12_000)).toBe(0);
  });
});

describe("hostTone", () => {
  it("is green while the core answers", () => {
    expect(hostTone("open", 0)).toBe("ok");
    expect(hostTone("open", 12)).toBe("ok");
  });

  it("is amber while connecting or before the first answer", () => {
    expect(hostTone("connecting", 0)).toBe("busy");
    expect(hostTone("open", null)).toBe("busy");
  });

  it("is red when disconnected or an open connection stopped answering", () => {
    expect(hostTone("closed", 1)).toBe("bad");
    expect(hostTone("open", 13)).toBe("bad");
  });

  it("names each state", () => {
    expect(hostToneLabel("ok", "open")).toBe("接続中");
    expect(hostToneLabel("bad", "closed")).toBe("切断");
    expect(hostToneLabel("bad", "open")).toBe("応答なし");
    expect(hostToneLabel("busy", "connecting")).toBe("接続を確認中");
  });
});
