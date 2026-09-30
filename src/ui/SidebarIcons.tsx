// Small inline icons of the sidebar (no icon font, no text glyphs).

interface IconProps {
  className?: string;
}

/** A chevron pointing down; CSS turns it (up when a picker opens, right when a row is collapsed). */
export function ChevronIcon({ className = "" }: IconProps) {
  return (
    <svg
      className={`chevron ${className}`}
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M2 3.5 5 6.5 8 3.5" />
    </svg>
  );
}

export function CheckIcon({ className = "" }: IconProps) {
  return (
    <svg
      className={`check ${className}`}
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M2 5.2 4.2 7.4 8 2.8" />
    </svg>
  );
}
