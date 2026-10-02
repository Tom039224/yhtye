import { EmptyState } from "./EmptyState";

/** The runs (history) view: not designed yet; the design's own empty state (§3.2). */
export function RunsView() {
  return (
    <section className="runs-view" aria-label="runs">
      <EmptyState icon="history" text="現状は何もありません" />
    </section>
  );
}
