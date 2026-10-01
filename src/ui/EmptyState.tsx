import { Icon, type IconName } from "./Icon";

/** A quiet empty state: a faint icon and one short line. */
export function EmptyState({ icon, text, testId, tone = "dim" }: { icon: IconName; text: string; testId?: string; tone?: "dim" | "error" }) {
  return (
    <div className={`empty-state ${tone === "error" ? "error-text" : ""}`} data-testid={testId}>
      <Icon name={icon} size={20} />
      <span>{text}</span>
    </div>
  );
}
