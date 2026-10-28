// The words for the remote hosts (T-98): their state, and what went wrong when pairing.

import { describe, expect, it } from "vitest";

import { CommandError } from "../api/transport";
import { compatibilityNote, describePairError, remoteAddress, remoteState, tokenNote } from "./hostLabels";
import { hostInfo } from "../test/remoteHosts";

const error = (code: ConstructorParameters<typeof CommandError>[0], message: string) => describePairError(new CommandError(code, message));

describe("describePairError", () => {
  it("tells a wrong, expired or used code from the other causes of a refusal (401)", () => {
    const problem = error("forbidden", "the pairing code was refused: expired");
    expect(problem.text).toContain("コードが違うか、有効期限が切れたか、すでに使われています");
    expect(problem.detail).toBe("expired");
    expect(error("forbidden", "the pairing code was refused").detail).toBeUndefined();
  });

  it("says a host of another Tailscale user, or a tagged node, does not take this PC (403)", () => {
    const problem = error("forbidden", "the host does not take devices of other Tailscale users");
    expect(problem.text).toContain("同じ Tailscale ユーザーの端末でない");
    expect(problem.text).toContain("タグ付きノード");
  });

  it("says when another Yhtye answered than the link's, and to remove this device there", () => {
    const problem = error("conflict", "another Yhtye answered (expected a, got b)");
    expect(problem.text).toContain("別の Yhtye が答えました");
    expect(problem.text).toContain("端末一覧から外してください");
    expect(problem.detail).toContain("expected a, got b");
  });

  it("says when the host cannot be reached, with the reason as the detail", () => {
    const problem = error("unavailable", "could not reach 100.64.0.7:1423: timed out");
    expect(problem.text).toContain("接続先に届きません");
    expect(problem.text).toContain("Tailscale");
    expect(problem.detail).toBe("could not reach 100.64.0.7:1423: timed out");
    expect(error("unavailable", "the host answered with an error (500)").text).toContain("接続先がエラーを返しました");
  });

  it("names the address that is not a Tailscale one", () => {
    const problem = error("invalid_argument", "192.168.1.5 is not a Tailscale address");
    expect(problem.text).toContain("192.168.1.5 は Tailscale のアドレスではありません");
  });

  it("words each way an input can be no good", () => {
    const text = (message: string) => error("invalid_argument", message).text;
    expect(text("this is not a Yhtye pairing code")).toContain("ペアリングリンクではありません");
    expect(text("this pairing code is of a newer format (v=2); update Yhtye")).toContain("Yhtye を更新");
    expect(text("no host was given")).toBe("ホストを入力してください");
    expect(text("the port must be a number from 1 to 65535")).toContain("1〜65535");
    expect(text("no pairing code was given")).toBe("コードを入力してください");
    expect(text("the pairing code must be 8 letters and digits (no 0, O, 1, I or L)")).toContain("英数字 8 文字");
    expect(error("invalid_argument", "the host did not accept the request: the device name must be 1 to 64 characters").detail).toBe("the device name must be 1 to 64 characters");
  });

  it("shows what it does not know as it is", () => {
    expect(error("internal", "boom")).toEqual({ text: "boom" });
    expect(error("transport", "gone")).toEqual({ text: "gone" });
    expect(error("forbidden", "something else")).toEqual({ text: "接続先に拒否されました", detail: "something else" });
  });
});

describe("a remote host's state", () => {
  const entry = (connection: Parameters<typeof hostInfo>[1], store: "open" | "connecting" | "closed" = "open") => ({ info: hostInfo("s", connection), connection: store });

  it("reads as connected only while the host is connected and its store hears it", () => {
    expect(remoteState(entry({ state: "connected", session: 1 }))).toEqual({ text: "接続中", down: false });
    expect(remoteState(entry({ state: "connected", session: 1 }, "connecting"))).toEqual({ text: "接続を確認中", down: true });
    expect(remoteState(entry({ state: "connected", session: 1 }, "closed"))).toEqual({ text: "切断", down: true });
  });

  it("words the connection that is not one", () => {
    expect(remoteState(entry({ state: "connecting", attempt: 0 }))).toMatchObject({ text: "接続しています…", down: true });
    expect(remoteState(entry({ state: "waiting_to_reconnect", attempt: 1, delay_ms: 1, reason: null }))).toMatchObject({ text: "切断 (再接続待ち)", down: true });
    expect(remoteState(entry({ state: "revoked" }))).toMatchObject({ text: "このホストから外されました", down: true });
    expect(remoteState(entry({ state: "rejected", message: null }))).toMatchObject({ down: true });
    expect(remoteState(entry({ state: "idle" }))).toMatchObject({ text: "切断", down: true });
  });

  it("shows the address, and warns about a token that is not kept and a version that does not fit", () => {
    expect(remoteAddress({ hosts: ["100.64.0.7", "pc.tail.ts.net"], port: 1423 })).toBe("100.64.0.7:1423");
    expect(tokenNote({ place: "keyring" })).toBeNull();
    expect(tokenNote({ place: "memory_only", reason: "no secret service" })).toContain("再起動すると");
    expect(tokenNote({ place: "missing" })).toContain("ペアリングし直してください");
    expect(compatibilityNote({ type: "compatible" })).toBeNull();
    expect(compatibilityNote({ type: "unknown" })).toBeNull();
    expect(compatibilityNote(null)).toBeNull();
    expect(compatibilityNote({ type: "incompatible", host: "0.2.0", app: "0.1.0" })).toContain("接続先 0.2.0 / この PC のアプリ 0.1.0");
  });
});
