import { describe, expect, it } from "vitest";

import type { ProjectInfo } from "../api/generated";
import { type HostProject, LOCAL_HOST } from "../store/hosts";
import { newProjectView } from "../store/project";
import { isCurrentRow, openedRows } from "./useProjects";

const info = (id: string, open = true): ProjectInfo => ({ id, name: id, path: `/work/${id}`, open, running: false, awake: false });
const row = (id: string, open = true, host = LOCAL_HOST): HostProject => ({ host, remote: host !== LOCAL_HOST, hostName: null, project: info(id, open) });

describe("isCurrentRow", () => {
  it("is the project on screen of the host on screen, not the same id of another host", () => {
    const view = newProjectView(info("a"));
    expect(isCurrentRow(row("a"), LOCAL_HOST, view)).toBe(true);
    expect(isCurrentRow(row("b"), LOCAL_HOST, view)).toBe(false);
    expect(isCurrentRow(row("a", true, "sid-1"), LOCAL_HOST, view)).toBe(false);
    expect(isCurrentRow(row("a"), LOCAL_HOST, null)).toBe(false);
  });
});

describe("openedRows", () => {
  const rows = [row("a"), row("b", false), row("c"), row("a", true, "sid-1"), row("z", false, "sid-1")];

  it("lists the projects the cores have open, in the order given, across hosts", () => {
    expect(openedRows(rows, LOCAL_HOST, null).map((r) => `${r.host}:${r.project.id}`)).toEqual(["local:a", "local:c", "sid-1:a"]);
  });

  it("adds the project on screen even if its core does not say it is open", () => {
    const shown = openedRows(rows, LOCAL_HOST, newProjectView(info("b", false)));
    expect(shown.map((r) => r.project.id)).toEqual(["a", "b", "c", "a"]);
  });

  it("lists the project on screen when the list does not have it at all", () => {
    const shown = openedRows(rows, "sid-1", newProjectView(info("fresh", false)));
    expect(shown.map((r) => `${r.host}:${r.project.id}`)).toEqual(["local:a", "local:c", "sid-1:a", "sid-1:fresh"]);
    expect(shown.at(-1)).toMatchObject({ host: "sid-1", remote: true });
  });

  it("is empty when nothing is open", () => {
    expect(openedRows([row("b", false)], LOCAL_HOST, null)).toEqual([]);
    expect(openedRows([], LOCAL_HOST, null)).toEqual([]);
  });
});
