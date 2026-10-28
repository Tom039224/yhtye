import { useCallback, useEffect, useState } from "react";

import type { AttachmentMeta } from "../api/generated";
import { AttachmentTile, AttachmentViewer } from "./AttachmentTile";

/** Reads a sent attachment's content as a `data:` URL (cached by the store). */
export type LoadAttachment = (id: string) => Promise<string>;

interface Props {
  attachments: AttachmentMeta[];
  /** Absent: images show their icon (no chat to read them from). */
  load?: LoadAttachment;
}

/**
 * The attachments of a user message in the transcript: the composer's square
 * tiles without the ×, the delivery badge showing how they were actually
 * delivered. An image is read when shown and opens large on click.
 */
export function SentAttachments({ attachments, load }: Props) {
  const [viewing, setViewing] = useState<{ name: string; src: string } | null>(null);
  const close = useCallback(() => setViewing(null), []);
  return (
    <>
      <div className="msg-attachments" role="list" aria-label="添付ファイル">
        {attachments.map((a) => (
          <SentTile key={a.id} meta={a} load={load} onOpen={setViewing} />
        ))}
      </div>
      {viewing ? <AttachmentViewer name={viewing.name} src={viewing.src} onClose={close} /> : null}
    </>
  );
}

function SentTile({ meta, load, onOpen }: { meta: AttachmentMeta; load?: LoadAttachment; onOpen: (v: { name: string; src: string }) => void }) {
  const { src, error } = usePreview(meta, load);
  return (
    <AttachmentTile
      name={meta.name}
      kind={meta.kind}
      preview={src}
      delivery={meta.delivery}
      size={meta.size}
      detail={error ? `プレビューを読めませんでした: ${error}` : undefined}
      onOpen={src ? () => onOpen({ name: meta.name, src }) : undefined}
    />
  );
}

function usePreview(meta: AttachmentMeta, load?: LoadAttachment): { src: string | null; error: string | null } {
  const [state, setState] = useState<{ src: string | null; error: string | null }>({ src: null, error: null });
  useEffect(() => {
    if (!load || meta.kind !== "image") return;
    let live = true;
    load(meta.id).then(
      (src) => {
        if (live) setState({ src, error: null });
      },
      (e: unknown) => {
        // The tile keeps the image icon; why is in its tooltip.
        if (live) setState({ src: null, error: e instanceof Error ? e.message : String(e) });
      },
    );
    return () => {
      live = false;
    };
  }, [load, meta.id, meta.kind]);
  return state;
}
