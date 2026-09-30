import type { ButtonHTMLAttributes, ReactNode } from "react";

import "./icon.css";

/**
 * The app's own line icons: a 24x24 grid, 1.75 strokes, round caps and joins,
 * drawn in `currentColor`. Add a glyph by adding an entry here; keep it to
 * strokes so the set stays one family (the stop square is the one filled shape).
 */
const GLYPHS = {
  send: <path d="M12 19V5M6 11l6-6 6 6" />,
  stop: <rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor" />,
  refresh: (
    <>
      <path d="M19 12a7 7 0 1 1-2.1-5" />
      <path d="M19.5 4.5v4h-4" />
    </>
  ),
  x: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  trash: <path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12M10.5 11v5M13.5 11v5" />,
  edit: <path d="M5 19.5l.8-4L16 5.3a2.1 2.1 0 0 1 3 3L8.8 18.7zM14 7.3l3 3" />,
  at: (
    <>
      <circle cx="12" cy="12" r="3.5" />
      <path d="M15.5 9v4a2.5 2.5 0 0 0 5 0v-1a8.5 8.5 0 1 0-3.4 6.8" />
    </>
  ),
  code: <path d="M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5M13 6l-2 12" />,
  history: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  sliders: (
    <>
      <path d="M4 6.5h3M11 6.5h9M4 12h9M17 12h3M4 17.5h2M10 17.5h10" />
      <circle cx="9" cy="6.5" r="2" />
      <circle cx="15" cy="12" r="2" />
      <circle cx="8" cy="17.5" r="2" />
    </>
  ),
  "git-branch": (
    <>
      <circle cx="7" cy="6" r="2" />
      <circle cx="7" cy="18" r="2" />
      <circle cx="17" cy="8.5" r="2" />
      <path d="M7 8v8M17 10.5c0 3-3 4-10 5.5" />
    </>
  ),
  terminal: <path d="M4.5 6.5h15v11h-15zM8 10.5l2.5 2L8 14.5M12.5 15h3.5" />,
  bot: (
    <>
      <rect x="5" y="8" width="14" height="10.5" rx="3" />
      <path d="M12 4.5V8M9.5 13h.01M14.5 13h.01" />
    </>
  ),
  plug: <path d="M9 4v4M15 4v4M7 8h10v3a5 5 0 0 1-10 0zM12 16v4" />,
  "plug-off": <path d="M9 4v4M15 4v4M7 8h10v3a5 5 0 0 1-10 0zM12 16v4M4 4l16 16" />,
  alert: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5v5M12 16h.01" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof GLYPHS;

interface IconProps {
  name: IconName;
  /** Width and height in px. */
  size?: number;
  /** Turns the icon slowly (a busy indicator; stops under reduced motion). */
  spin?: boolean;
  className?: string;
}

const DEFAULT_SIZE = 16;

/** Decorative: hidden from assistive tech, so its button carries the label. */
export function Icon({ name, size = DEFAULT_SIZE, spin = false, className }: IconProps) {
  return (
    <svg
      className={["icon", spin ? "icon-spin" : "", className ?? ""].filter(Boolean).join(" ")}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {GLYPHS[name]}
    </svg>
  );
}

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label" | "children"> {
  icon: IconName;
  /** The accessible name; also the tooltip unless `title` says more. */
  label: string;
  title?: string;
  size?: number;
  /** `primary` is the filled call to action, `danger` a destructive one. */
  tone?: "default" | "primary" | "danger";
  spin?: boolean;
}

/** An icon-only button. The label is required, so it always has an aria-label and a tooltip. */
export function IconButton({
  icon,
  label,
  title,
  size,
  tone = "default",
  spin,
  className,
  type = "button",
  ...rest
}: IconButtonProps) {
  const classes = ["icon-button", tone === "default" ? "" : `icon-button-${tone}`, className ?? ""].filter(Boolean);
  return (
    <button type={type} className={classes.join(" ")} aria-label={label} title={title ?? label} {...rest}>
      <Icon name={icon} size={size} spin={spin} />
    </button>
  );
}
