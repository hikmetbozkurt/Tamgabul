import type { ReactNode } from "react";

// Stroke icons drawn for the design canvas; decorative unless `label` is given.
const PATHS: Record<string, ReactNode> = {
  search: (<><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></>),
  memory: (<><ellipse cx="12" cy="6" rx="7" ry="3" /><path d="M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6" /><path d="M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6" /></>),
  models: (<><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z" /><path d="M4 7.5l8 4.5 8-4.5" /><path d="M12 12v9" /></>),
  folder: <path d="M3 6h6l2 2h10v11H3z" />,
  settings: (<><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7 7 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z" /></>),
  mic: (<><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></>),
  arrowRight: (<><path d="M5 12h14" /><path d="M13 6l6 6-6 6" /></>),
  file: (<><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></>),
  fileText: (<><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /><path d="M9 13h6" /><path d="M9 17h4" /></>),
  image: (<><rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="9" cy="11" r="2" /><path d="M21 17l-5-5-8 7" /></>),
  external: (<><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5H5V6h5" /></>),
  eye: (<><path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>),
  clock: (<><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>),
  chevronDown: <path d="M6 9l6 6 6-6" />,
  chevronUp: <path d="M6 15l6-6 6 6" />,
  minimize: <path d="M5 12h14" />,
  maximize: <rect x="5" y="5" width="14" height="14" />,
  close: (<><path d="M5 5l14 14" /><path d="M19 5L5 19" /></>),
  chip: (<><rect x="6" y="6" width="12" height="12" rx="2" /><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" /></>),
  layers: (<><path d="M12 3l9 5-9 5-9-5z" /><path d="M3 13l9 5 9-5" /></>),
  speaker: (<><path d="M4 9h4l5-4v14l-5-4H4z" /><path d="M16 9a4 4 0 0 1 0 6" /></>),
  speakerWaves: (<><path d="M4 9h4l5-4v14l-5-4H4z" /><path d="M16 9a4 4 0 0 1 0 6" /><path d="M19 6a8 8 0 0 1 0 12" /></>),
  plus: <path d="M12 5v14M5 12h14" />,
  trash: (<><path d="M4 7h16" /><path d="M10 11v6M14 11v6" /><path d="M6 7l1 13h10l1-13" /><path d="M9 7V4h6v3" /></>),
  check: <path d="M5 12l5 5 9-10" />,
  info: (<><circle cx="12" cy="12" r="9" /><path d="M12 11v6" /><path d="M12 7.5h.01" /></>),
  warn: (<><path d="M12 3l10 18H2z" /><path d="M12 10v5" /><path d="M12 18h.01" /></>),
  expand: (<><path d="M15 3h6v6" /><path d="M9 21H3v-6" /><path d="M21 3l-7 7" /><path d="M3 21l7-7" /></>),
  reveal: (<><path d="M3 6h6l2 2h10v11H3z" /><path d="M12 12v4" /><path d="M10 14l2 2 2-2" /></>),
  stop: <rect x="7" y="7" width="10" height="10" rx="1.5" />,
  play: <path d="M8 5l11 7-11 7z" />,
  refresh: (<><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></>),
  globe: (<><circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z" /></>),
  keyboard: (<><rect x="2" y="6" width="20" height="12" rx="2" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" /></>),
};

const FILLED: Record<string, ReactNode> = {
  spark: <path d="M12 2l1.8 6.2L20 10l-6.2 1.8L12 18l-1.8-6.2L4 10l6.2-1.8z" />,
  user: (<><circle cx="12" cy="8" r="4" /><path d="M4 21c0-4 3.6-7 8-7s8 3 8 7z" /></>),
  drop: <path d="M12 3c3 4 6 7.5 6 11a6 6 0 0 1-12 0c0-3.5 3-7 6-11z" />,
  more: (<><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></>),
};

export type IconName = keyof typeof PATHS | keyof typeof FILLED;

export function Icon({
  name,
  size = 18,
  stroke = 1.7,
  color = "currentColor",
  label,
  className,
}: {
  name: IconName;
  size?: number;
  stroke?: number;
  color?: string;
  label?: string;
  className?: string;
}) {
  const a11y = label ? { role: "img", "aria-label": label } : { "aria-hidden": true };
  if (name in FILLED) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill={color} className={className} {...a11y}>
        {FILLED[name]}
      </svg>
    );
  }
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      {...a11y}
    >
      {PATHS[name]}
    </svg>
  );
}

export function FolderIcon({ tone = "normal", size = 26 }: { tone?: "normal" | "off" | "err"; size?: number }) {
  const fill = tone === "normal" ? "var(--folder-fill)" : tone === "err" ? "var(--err-soft)" : "none";
  const line = tone === "normal" ? "var(--folder-line)" : tone === "err" ? "var(--err)" : "var(--ink-2)";
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={fill} stroke={line} strokeWidth={1.4} strokeLinejoin="round" aria-hidden="true">
      <path d="M3 6h6l2 2h10v11H3z" />
    </svg>
  );
}

export function Logo({ size = 22 }: { size?: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        width: size, height: size, borderRadius: 6, background: "var(--logo)",
        display: "inline-flex", alignItems: "center", justifyContent: "center", flex: "none",
      }}
    >
      <Icon name="drop" size={Math.round(size * 0.55)} color="#FFFFFF" />
    </span>
  );
}
