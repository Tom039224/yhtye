import { useMemo } from "react";
import { encode } from "uqr";

/** Quiet zone around the code, in modules (the QR standard asks for 4). */
const QUIET_ZONE = 4;

/** The dark modules of `value`'s QR code as one SVG path, and the code's width in modules. */
export function qrPath(value: string): { path: string; size: number } {
  const { data, size } = encode(value, { ecc: "M", border: 0 });
  const parts: string[] = [];
  data.forEach((row, y) =>
    row.forEach((dark, x) => {
      if (dark) parts.push(`M${x + QUIET_ZONE} ${y + QUIET_ZONE}h1v1h-1z`);
    }),
  );
  return { path: parts.join(""), size: size + 2 * QUIET_ZONE };
}

/**
 * A QR code drawn in the page (black on white whatever the theme, so phones
 * read it). Made here: the content never leaves the PC.
 */
export function QrCode({ value, label, size = 208 }: { value: string; label: string; size?: number }) {
  const qr = useMemo(() => qrPath(value), [value]);
  return (
    <svg className="qr-code" role="img" aria-label={label} width={size} height={size} viewBox={`0 0 ${qr.size} ${qr.size}`} shapeRendering="crispEdges">
      <rect width={qr.size} height={qr.size} fill="#fff" />
      <path d={qr.path} fill="#000" />
    </svg>
  );
}
