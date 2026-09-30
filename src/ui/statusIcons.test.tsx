// The state → icon tables: the words move into the tooltip, the shape and colour carry the meaning.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { StepKind } from "../api/generated";
import { EmptyState } from "./EmptyState";
import { groupIcon, HELP_ICON, StatusChip, stepIcon, ToolStatus, toneIcon } from "./statusIcons";

describe("status icons", () => {
  it("gives every task tone its own icon, and an investigation searches instead of writing code", () => {
    expect(toneIcon("implementing", "code")).toBe("code");
    expect(toneIcon("implementing", "investigate")).toBe("search");
    expect(toneIcon("reviewing", "investigate")).toBe("eye");
    const tones = ["waiting", "implementing", "reviewing", "handling", "done", "cancelled"] as const;
    expect(new Set(tones.map((t) => toneIcon(t, "code"))).size).toBe(tones.length);
  });

  it("draws the steps of a task as code / eye / flag / check", () => {
    const steps: StepKind[] = ["implement", "review", "checkpoint", "done"];
    expect(steps.map((s) => stepIcon(s, "code"))).toEqual(["code", "eye", "flag", "check"]);
    expect(stepIcon("implement", "investigate")).toBe("search");
  });

  it("tells a group waiting to be finished apart from one in progress", () => {
    expect(groupIcon("active", false)).toBe("layers");
    expect(groupIcon("active", true)).toBe("hourglass");
    expect(groupIcon("finishing", false)).toBe("git-merge");
    expect(groupIcon("merge_blocked", false)).toBe("alert");
  });

  it("has an icon for every kind of help", () => {
    expect(HELP_ICON.question).toBe("help");
    expect(HELP_ICON.merge_conflict).toBe("git-merge");
    expect(Object.values(HELP_ICON).every(Boolean)).toBe(true);
  });
});

describe("StatusChip", () => {
  it("keeps the words as the tooltip and as hidden text", () => {
    render(<StatusChip className="tone-done" icon="check" label="完了" testId="chip" />);
    const chip = screen.getByTestId("chip");
    expect(chip).toHaveAttribute("title", "完了");
    expect(chip).toHaveClass("tone-done");
    expect(chip).toHaveTextContent("完了");
    expect(chip.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("breathes only while an agent works", () => {
    const { rerender } = render(<StatusChip className="tone-implementing" icon="code" label="実装中" testId="chip" active />);
    expect(screen.getByTestId("chip")).toHaveClass("status-chip-active");
    rerender(<StatusChip className="tone-done" icon="check" label="完了" testId="chip" />);
    expect(screen.getByTestId("chip")).not.toHaveClass("status-chip-active");
  });
});

describe("ToolStatus", () => {
  it("shows a check, a cross or a turning arc, and keeps the status word for assistive tech", () => {
    const { container, rerender } = render(<ToolStatus status="completed" />);
    expect(container).toHaveTextContent("completed");
    expect(container.querySelector(".tool-status-completed")).not.toBeNull();
    rerender(<ToolStatus status="failed" />);
    expect(container.querySelector(".tool-status-failed")).not.toBeNull();
    rerender(<ToolStatus status="in_progress" />);
    expect(container.querySelector("svg")).toHaveClass("icon-spin");
    rerender(<ToolStatus status={null} />);
    expect(container).toHaveTextContent("pending");
  });
});

describe("EmptyState", () => {
  it("is a faint icon and one line", () => {
    render(<EmptyState icon="list" text="タスクはまだありません" testId="empty" />);
    expect(screen.getByTestId("empty")).toHaveTextContent("タスクはまだありません");
    expect(screen.getByTestId("empty").querySelector("svg")).not.toBeNull();
  });
});
