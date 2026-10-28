// The composer's attachments: staged with the core as soon as they are added,
// discarded when taken out, and the previews of sent ones read once.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { CommandError } from "../api/transport";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { chatSnapshot } from "../test/chats";
import { Attachments, attachmentsReady, guessKind, PREVIEW_CACHE_SIZE, stagedIds } from "./attachments";
import { type DraftAttachment, type Drafts, memoryDrafts } from "./drafts";

const PNG = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "shot.png", { type: "image/png" });
const LOG = new File(["line 1\n"], "build.log", { type: "text/plain" });

let transport: MemoryTransport;
let core: FakeCore;
let drafts: Drafts;
let errors: string[];
let attachments: Attachments;

const list = () => drafts.attachments("repo", "C-1");

beforeEach(() => {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.attach(transport);
  drafts = memoryDrafts();
  errors = [];
  attachments = new Attachments(transport, drafts, (m) => errors.push(m));
});

describe("attaching", () => {
  it("stages each file with its name, type and content, and shows it staged", async () => {
    await attachments.attach("repo", "C-1", [PNG, LOG]);
    expect(transport.callsOf("stage_attachment")).toEqual([
      { type: "stage_attachment", project: "repo", chat: "C-1", name: "shot.png", mime: "image/png", data_base64: "iVBORw==" },
      { type: "stage_attachment", project: "repo", chat: "C-1", name: "build.log", mime: "text/plain", data_base64: "bGluZSAxCg==" },
    ]);
    const [png, log] = list();
    expect(png).toMatchObject({ status: "staged", kind: "image", preview: "data:image/png;base64,iVBORw==" });
    expect(png.meta?.delivery).toBe("native");
    expect(log).toMatchObject({ status: "staged", kind: "text", preview: null, name: "build.log" });
    expect(stagedIds(list())).toEqual([png.meta?.id, log.meta?.id]);
    expect(attachmentsReady(list())).toBe(true);
  });

  it("shows a file as uploading until the core answers", async () => {
    let answer: () => void = () => {};
    core.before = (cmd) => (cmd.type === "stage_attachment" ? new Promise<void>((r) => (answer = r)) : undefined);
    const done = attachments.attach("repo", "C-1", [LOG]);
    expect(list()).toMatchObject([{ status: "uploading", name: "build.log", kind: "text", meta: null }]);
    expect(attachmentsReady(list())).toBe(false);
    await vi.waitFor(() => expect(transport.callsOf("stage_attachment")).toHaveLength(1));
    answer();
    await done;
    expect(list()[0].status).toBe("staged");
  });

  it("marks a file the core refused as failed, with the reason", async () => {
    core.failures.set("stage_attachment", new CommandError("invalid_argument", "bad base64"));
    await attachments.attach("repo", "C-1", [LOG]);
    expect(list()).toMatchObject([{ status: "failed", error: "bad base64", meta: null }]);
    expect(stagedIds(list())).toEqual([]);
    expect(attachmentsReady(list())).toBe(false);
  });

  it("names a file that has no name", async () => {
    await attachments.attach("repo", "C-1", [new File(["x"], "", { type: "" })]);
    expect(transport.callsOf("stage_attachment")[0]).toMatchObject({ name: "attachment", mime: undefined });
  });

  it("keeps the drafts of two chats apart", async () => {
    await attachments.attach("repo", "C-2", [LOG]);
    expect(list()).toEqual([]);
    expect(drafts.attachments("repo", "C-2")).toHaveLength(1);
  });
});

describe("removing", () => {
  it("discards a staged file with the core", async () => {
    await attachments.attach("repo", "C-1", [LOG]);
    const [item] = list();
    await attachments.remove("repo", "C-1", item.key);
    expect(list()).toEqual([]);
    expect(transport.callsOf("discard_attachment")).toEqual([{ type: "discard_attachment", project: "repo", chat: "C-1", id: item.meta?.id }]);
  });

  it("discards a file taken out while it was uploading once it is staged", async () => {
    let answer: () => void = () => {};
    core.before = (cmd) => (cmd.type === "stage_attachment" ? new Promise<void>((r) => (answer = r)) : undefined);
    const done = attachments.attach("repo", "C-1", [LOG]);
    await attachments.remove("repo", "C-1", list()[0].key);
    expect(list()).toEqual([]);
    await vi.waitFor(() => expect(transport.callsOf("stage_attachment")).toHaveLength(1));
    answer();
    await done;
    expect(list()).toEqual([]);
    expect(transport.callsOf("discard_attachment")).toHaveLength(1);
    expect(core.attachments.size).toBe(0);
  });

  it("drops a failed file without asking the core", async () => {
    core.failures.set("stage_attachment", new CommandError("internal", "disk full"));
    await attachments.attach("repo", "C-1", [LOG]);
    await attachments.remove("repo", "C-1", list()[0].key);
    expect(list()).toEqual([]);
    expect(transport.callsOf("discard_attachment")).toHaveLength(0);
  });

  it("reports a failed discard, but not one of a file that is already gone", async () => {
    await attachments.attach("repo", "C-1", [LOG, PNG]);
    const [log, png] = list();
    core.attachments.clear();
    await attachments.remove("repo", "C-1", log.key);
    expect(errors).toEqual([]);
    core.failures.set("discard_attachment", new CommandError("invalid_state", "already sent"));
    await attachments.remove("repo", "C-1", png.key);
    expect(errors).toEqual(["discard_attachment failed: already sent"]);
  });
});

describe("previews of sent attachments", () => {
  it("reads one once and caches it", async () => {
    await attachments.attach("repo", "C-1", [PNG]);
    const id = list()[0].meta?.id ?? "";
    expect(await attachments.preview("repo", "C-1", id)).toBe("data:image/png;base64,iVBORw==");
    expect(await attachments.preview("repo", "C-1", id)).toBe("data:image/png;base64,iVBORw==");
    expect(transport.callsOf("get_attachment")).toHaveLength(1);
  });

  it("asks again after a failure", async () => {
    await attachments.attach("repo", "C-1", [PNG]);
    const id = list()[0].meta?.id ?? "";
    core.failures.set("get_attachment", new CommandError("unavailable", "offline"));
    await expect(attachments.preview("repo", "C-1", id)).rejects.toThrow("offline");
    core.failures.clear();
    expect(await attachments.preview("repo", "C-1", id)).toContain("base64,");
    expect(transport.callsOf("get_attachment")).toHaveLength(2);
  });

  it("keeps the previews of what was sent, so the transcript does not read them back", async () => {
    await attachments.attach("repo", "C-1", [PNG, LOG]);
    const sent = list();
    attachments.sent("repo", "C-1", sent);
    expect(list()).toEqual([]);
    expect(await attachments.preview("repo", "C-1", sent[0].meta?.id ?? "")).toBe("data:image/png;base64,iVBORw==");
    expect(transport.callsOf("get_attachment")).toHaveLength(0);
  });

  it("keeps a bounded number of previews, dropping the least recently used", async () => {
    const item = (n: number): DraftAttachment => ({
      key: `k${n}`,
      name: `${n}.png`,
      kind: "image",
      preview: `data:image/png;base64,${n}`,
      status: "staged",
      meta: { id: `id${n}`, name: `${n}.png`, mime: "image/png", size: 1, kind: "image", delivery: "native" },
      error: null,
    });
    attachments.sent("repo", "C-1", Array.from({ length: PREVIEW_CACHE_SIZE }, (_, n) => item(n)));
    await attachments.preview("repo", "C-1", "id0"); // used again: now the newest
    attachments.sent("repo", "C-1", [item(PREVIEW_CACHE_SIZE)]);
    expect(await attachments.preview("repo", "C-1", "id0")).toBe("data:image/png;base64,0");
    await expect(attachments.preview("repo", "C-1", "id1")).rejects.toThrow();
    expect(transport.callsOf("get_attachment").map((c) => c.id)).toEqual(["id1"]);
  });
});

describe("guessKind", () => {
  it("guesses from the file's type until the core says", () => {
    expect(guessKind("image/webp")).toBe("image");
    expect(guessKind("text/markdown")).toBe("text");
    expect(guessKind("application/json")).toBe("text");
    expect(guessKind("application/pdf")).toBe("file");
    expect(guessKind("")).toBe("file");
  });
});
