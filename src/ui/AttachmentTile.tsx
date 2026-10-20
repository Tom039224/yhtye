import { useEffect, useRef } from "react";

import type { AttachmentDelivery, AttachmentKind } from "../api/generated";
import { Icon, IconButton, type IconName } from "./Icon";

/** How each delivery reaches the agent, for the tile's badge (its tooltip and accessible name). */
export const DELIVERY_LABELS: Record<AttachmentDelivery, string> = {
  native: "モデルに直接渡す (native)",
  path: "ファイルとして保存し、パスで渡す (path)",
};
const DELIVERY_ICONS: Record<AttachmentDelivery, IconName> = { native: "bot", path: "folder" };
const KIND_ICONS: Record<AttachmentKind, IconName> = { image: "image", text: "file-text", file: "file" };
/** The longest extension shown as the file's type label (longer ones fall back to the kind). */
const MAX_EXT = 5;

/** The short type label under a file's icon: its extension, or its kind. */
export function typeLabel(name: string, kind: AttachmentKind): string {
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1) : "";
  if (ext.length > 0 && ext.length <= MAX_EXT) return ext.toUpperCase();
  return kind === "text" ? "TEXT" : kind === "image" ? "IMAGE" : "FILE";
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface Props {
  name: string;
  kind: AttachmentKind;
  /** A `data:` URL of the image (`null`: the file's icon, also while an image loads). */
  preview: string | null;
  /** The badge at the upper left (`null` while it is not known yet). */
  delivery: AttachmentDelivery | null;
  /** The composer's delivery is a prediction (the core decides when it is sent). */
  predicted?: boolean;
  status?: "uploading" | "staged" | "failed";
  /** Why it failed (`failed`). */
  error?: string | null;
  size?: number;
  /** Added to the tooltip (why an image's preview could not be read). */
  detail?: string;
  /** The × at the upper right (the composer's tiles). */
  onRemove?: () => void;
  /** Makes the tile a button (an image opens large). */
  onOpen?: () => void;
}

/**
 * One attachment as a square tile (composer and transcript alike): an image's
 * thumbnail or a file's type icon, how it is delivered at the upper left, and
 * in the composer a × at the upper right.
 */
export function AttachmentTile({ name, kind, preview, delivery, predicted = false, status = "staged", error = null, size, detail, onRemove, onOpen }: Props) {
  const details = [name, size === undefined ? null : formatSize(size), status === "failed" ? `添付できませんでした: ${error ?? ""}` : null, detail];
  const title = details.filter(Boolean).join(" · ");
  const face = preview ? (
    <img className="attachment-thumb" src={preview} alt={name} draggable={false} />
  ) : (
    <span className="attachment-file">
      <Icon name={KIND_ICONS[kind]} size={26} />
      <span className="attachment-ext">{typeLabel(name, kind)}</span>
      <span className="attachment-name">{name}</span>
    </span>
  );
  return (
    <div className={`attachment-tile attachment-tile-${status}`} role="listitem" title={title} aria-busy={status === "uploading" || undefined}>
      {onOpen ? (
        <button type="button" className="attachment-face" aria-label={`${name} を拡大表示`} onClick={onOpen}>
          {face}
        </button>
      ) : (
        <span className="attachment-face">{face}</span>
      )}
      {status === "uploading" ? (
        <span className="attachment-overlay" role="status" aria-label={`${name} を添付中`}>
          <Icon name="loader" size={18} spin />
        </span>
      ) : null}
      {status === "failed" ? (
        <span className="attachment-overlay attachment-overlay-failed" role="img" aria-label={`${name} を添付できませんでした`}>
          <Icon name="alert" size={18} />
        </span>
      ) : null}
      {delivery && status === "staged" ? <DeliveryBadge delivery={delivery} predicted={predicted} /> : null}
      {onRemove ? <IconButton icon="x" size={12} className="attachment-remove" label={`${name} を外す`} onClick={onRemove} /> : null}
    </div>
  );
}

function DeliveryBadge({ delivery, predicted }: { delivery: AttachmentDelivery; predicted: boolean }) {
  const label = predicted ? `${DELIVERY_LABELS[delivery]} · 送信時に確定` : DELIVERY_LABELS[delivery];
  return (
    <span className={`attachment-delivery attachment-delivery-${delivery}`} role="img" aria-label={label} title={label} data-delivery={delivery}>
      <Icon name={DELIVERY_ICONS[delivery]} size={11} />
    </span>
  );
}

/** An image shown large over the app; Esc, the × or a click outside it closes it. */
export function AttachmentViewer({ name, src, onClose }: { name: string; src: string; onClose: () => void }) {
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      opener?.focus?.();
    };
  }, [onClose]);
  return (
    <div
      className="modal-backdrop attachment-viewer"
      role="dialog"
      aria-modal="true"
      aria-label={name}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <figure className="attachment-viewer-body">
        <img src={src} alt={name} />
        <figcaption>{name}</figcaption>
      </figure>
      <IconButton ref={close} icon="x" className="attachment-viewer-close" label="閉じる" onClick={onClose} />
    </div>
  );
}
