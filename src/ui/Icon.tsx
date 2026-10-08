import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";

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
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  history: (
    <>
      <path d="M4.2 12A7.8 7.8 0 1 0 6.6 6.4" />
      <path d="M4.5 4v4h4M12 8v4l2.5 1.5" />
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
  "plug-off": <path d="M9 4v4M15 4v4M7 8h10v3a5 5 0 0 1-10 0zM12 16v4M4 4l16 16" />,
  alert: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5v5M12 16h.01" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5M12 7.8h.01" />
    </>
  ),
  help: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M9.6 9.6a2.4 2.4 0 1 1 3.5 2.1c-.7.4-1.1.9-1.1 1.8M12 16.6h.01" />
    </>
  ),
  ban: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M6 6l12 12" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  more: (
    <>
      <circle cx="5.5" cy="12" r="1" />
      <circle cx="12" cy="12" r="1" />
      <circle cx="18.5" cy="12" r="1" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  "chevron-down": <path d="M6.5 9.5l5.5 5.5 5.5-5.5" />,
  "chevrons-up": <path d="M7 11l5-5 5 5M7 18l5-5 5 5" />,
  "arrow-left": <path d="M19 12H5M11 6l-6 6 6 6" />,
  "arrow-right": <path d="M5 12h14M13 6l6 6-6 6" />,
  user: (
    <>
      <circle cx="12" cy="8.5" r="3.5" />
      <path d="M5 19.5c.8-3.6 3.6-5.5 7-5.5s6.2 1.9 7 5.5" />
    </>
  ),
  eye: (
    <>
      <path d="M3 12s3.4-6 9-6 9 6 9 6-3.4 6-9 6-9-6-9-6z" />
      <circle cx="12" cy="12" r="2.6" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M14.6 14.6L20 20" />
    </>
  ),
  flag: <path d="M6 20V4M6 5h11l-2.2 4 2.2 4H6" />,
  hourglass: <path d="M7 4.5h10M7 19.5h10M8 4.5c0 4.2 4 4.6 4 7.5s-4 3.3-4 7.5M16 4.5c0 4.2-4 4.6-4 7.5s4 3.3 4 7.5" />,
  loader: <path d="M12 4.5a7.5 7.5 0 1 0 7.5 7.5" />,
  activity: <path d="M3.5 12h4l2.5-6 4 12 2.5-6h4" />,
  wrench: <path transform="rotate(45 12 12)" d="M13.37 3.24A4 4 0 0 1 13.4 10.75V18a1.4 1.4 0 0 1-2.8 0V10.75A4 4 0 0 1 10.63 3.24L12 6z" />,
  bolt: <path d="M13 4L6.5 13.5H12L11 20l6.5-9.5H12z" />,
  bell: <path d="M7 16.5V11a5 5 0 0 1 10 0v5.5l1.5 1.5h-13zM10.4 20.5a1.8 1.8 0 0 0 3.2 0" />,
  "file-text": <path d="M7 4h7l4 4v12H7zM14 4v4h4M9.7 12.5h5M9.7 15.8h5" />,
  message: <path d="M5 6.5A1.5 1.5 0 0 1 6.5 5h11A1.5 1.5 0 0 1 19 6.5v8a1.5 1.5 0 0 1-1.5 1.5H11l-4 3.5V16H6.5A1.5 1.5 0 0 1 5 14.5z" />,
  "message-plus": (
    <>
      <path d="M5 6.5A1.5 1.5 0 0 1 6.5 5h11A1.5 1.5 0 0 1 19 6.5v8a1.5 1.5 0 0 1-1.5 1.5H11l-4 3.5V16H6.5A1.5 1.5 0 0 1 5 14.5z" />
      <path d="M12 8v5M9.5 10.5h5" />
    </>
  ),
  thought: (
    <>
      <path d="M5 6.5A1.5 1.5 0 0 1 6.5 5h11A1.5 1.5 0 0 1 19 6.5v8a1.5 1.5 0 0 1-1.5 1.5H11l-4 3.5V16H6.5A1.5 1.5 0 0 1 5 14.5z" />
      <path d="M9 10.5h.01M12 10.5h.01M15 10.5h.01" />
    </>
  ),
  "git-merge": (
    <>
      <circle cx="6" cy="6" r="2" />
      <circle cx="6" cy="18" r="2" />
      <circle cx="18" cy="12.5" r="2" />
      <path d="M6 8v8M8 6c6 0 10 2.5 10 4.5" />
    </>
  ),
  folder: <path d="M4 7.5A1.5 1.5 0 0 1 5.5 6H10l2 2.5h6.5A1.5 1.5 0 0 1 20 10v7.5a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z" />,
  globe: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.4 2.3 3.6 5.1 3.6 8.5s-1.2 6.2-3.6 8.5c-2.4-2.3-3.6-5.1-3.6-8.5S9.6 5.8 12 3.5" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15.5" r="3.5" />
      <path d="M10.5 13L19 4.5M16.5 7l2.5 2.5M13.5 10l2 2" />
    </>
  ),
  star: <path d="M12 4l2.4 5 5.4.7-4 3.8 1 5.4L12 16.2l-4.8 2.7 1-5.4-4-3.8 5.4-.7z" />,
  list: <path d="M9 7h11M9 12h11M9 17h11M4.5 7h.01M4.5 12h.01M4.5 17h.01" />,
  layers: <path d="M12 4.5l8 4.2-8 4.2-8-4.2zM4 12.8l8 4.2 8-4.2M4 16.8l8 4.2 8-4.2" />,
  minimize: <path d="M9 4.5V9H4.5M15 4.5V9h4.5M9 19.5V15H4.5M15 19.5V15h4.5" />,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof GLYPHS;

/** Every glyph of the set (the icon test draws each one). */
export const ICON_NAMES = Object.keys(GLYPHS) as IconName[];

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
  /** The button element, for moving focus to it. */
  ref?: Ref<HTMLButtonElement>;
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
  ref,
  ...rest
}: IconButtonProps) {
  const classes = ["icon-button", tone === "default" ? "" : `icon-button-${tone}`, className ?? ""].filter(Boolean);
  return (
    <button ref={ref} type={type} className={classes.join(" ")} aria-label={label} title={title ?? label} {...rest}>
      <Icon name={icon} size={size} spin={spin} />
    </button>
  );
}
