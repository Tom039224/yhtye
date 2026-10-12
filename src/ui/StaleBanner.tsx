import { Icon } from "./Icon";

/** Settings on screen were changed elsewhere (another device) while the user was editing them. */
export function StaleBanner({ onReload }: { onReload: () => void }) {
  return (
    <p className="agent-warning" role="status" data-testid="settings-stale">
      <Icon name="refresh" size={14} />
      <span>他の端末で設定が変更されました。表示は古いままです。</span>
      <button type="button" className="text-button" onClick={onReload}>
        再読込
      </button>
    </p>
  );
}
