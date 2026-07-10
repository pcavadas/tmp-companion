// src/ui/Icon.tsx — line-icon set, ported verbatim from chrome.jsx.
//
// 24×24 viewBox, Lucide-aligned. Implemented ONCE here; import everywhere.
// Light-only app — no sun/moon (the dark toggle was removed). Adds the prototype
// page icons: gauge · cable · music · spinner · warn-tri · info · refresh · grip ·
// trash · lock.

import type { IconName } from "./iconNames";

export type { IconName };

export interface IconProps {
  name: IconName;
  size?: number;
  stroke?: string;
  strokeWidth?: number;
}

export function Icon({
  name,
  size = 14,
  stroke = "currentColor",
  strokeWidth = 1.4,
}: IconProps) {
  const props = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke,
    strokeWidth,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  switch (name) {
    case "search":
      return (
        <svg {...props}>
          <circle cx="11" cy="11" r="7" />
          <path d="m20 20-3.5-3.5" />
        </svg>
      );
    case "plus":
      return (
        <svg {...props}>
          <path d="M12 5v14M5 12h14" />
        </svg>
      );
    case "settings":
      return (
        <svg {...props}>
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
        </svg>
      );
    case "share":
      return (
        <svg {...props}>
          <path d="M12 3v12M8 7l4-4 4 4M5 21h14" />
        </svg>
      );
    case "save":
      return (
        <svg {...props}>
          <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
          <path d="M17 21v-8H7v8M7 3v5h8" />
        </svg>
      );
    case "star":
      return (
        <svg {...props}>
          <polygon points="12 2 15 9 22 9 17 14 19 21 12 17 5 21 7 14 2 9 9 9" />
        </svg>
      );
    case "folder":
      return (
        <svg {...props}>
          <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
        </svg>
      );
    case "wave":
      return (
        <svg {...props}>
          <path d="M2 12h2l2-8 3 16 3-12 3 8 3-4 2 0" />
        </svg>
      );
    case "mic":
      return (
        <svg {...props}>
          <rect x="9" y="2" width="6" height="12" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0" />
          <line x1="12" y1="18" x2="12" y2="22" />
          <line x1="8" y1="22" x2="16" y2="22" />
        </svg>
      );
    case "sliders":
      return (
        <svg {...props}>
          <line x1="4" y1="6" x2="20" y2="6" />
          <line x1="4" y1="12" x2="20" y2="12" />
          <line x1="4" y1="18" x2="20" y2="18" />
          <circle cx="10" cy="6" r="2" fill="currentColor" stroke="none" />
          <circle cx="15" cy="12" r="2" fill="currentColor" stroke="none" />
          <circle cx="7" cy="18" r="2" fill="currentColor" stroke="none" />
        </svg>
      );
    case "list":
      return (
        <svg {...props}>
          <line x1="9" y1="6" x2="20" y2="6" />
          <line x1="9" y1="12" x2="20" y2="12" />
          <line x1="9" y1="18" x2="20" y2="18" />
          <circle cx="4.5" cy="6" r="1.2" fill="currentColor" stroke="none" />
          <circle cx="4.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
          <circle cx="4.5" cy="18" r="1.2" fill="currentColor" stroke="none" />
        </svg>
      );
    case "grid":
      return (
        <svg {...props}>
          <rect x="3" y="3" width="7" height="7" />
          <rect x="14" y="3" width="7" height="7" />
          <rect x="3" y="14" width="7" height="7" />
          <rect x="14" y="14" width="7" height="7" />
        </svg>
      );
    case "cmd":
      return (
        <svg {...props}>
          <path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z" />
        </svg>
      );
    case "arrow-right":
      return (
        <svg {...props}>
          <path d="M5 12h14M13 5l7 7-7 7" />
        </svg>
      );
    case "arrow-down":
      return (
        <svg {...props}>
          <path d="M12 5v14M5 13l7 7 7-7" />
        </svg>
      );
    case "check":
      return (
        <svg {...props}>
          <path d="M5 12l5 5L20 7" />
        </svg>
      );
    case "chev-right":
      return (
        <svg {...props}>
          <path d="M9 6l6 6-6 6" />
        </svg>
      );
    case "chev-down":
      return (
        <svg {...props}>
          <path d="M6 9l6 6 6-6" />
        </svg>
      );
    case "x":
      return (
        <svg {...props}>
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      );
    case "more":
      return (
        <svg {...props}>
          <circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none" />
          <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
          <circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none" />
        </svg>
      );
    case "metro":
      return (
        <svg {...props}>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 2" />
        </svg>
      );
    case "tune":
      return (
        <svg {...props}>
          <path d="M4 6h10M18 6h2M4 18h2M10 18h10M4 12h2M10 12h10" />
          <circle cx="16" cy="6" r="2" />
          <circle cx="8" cy="18" r="2" />
          <circle cx="8" cy="12" r="2" />
        </svg>
      );
    case "rules":
      return (
        <svg {...props}>
          <path d="M4 6h12M4 12h12M4 18h8" />
          <circle cx="20" cy="6" r="2" />
          <circle cx="20" cy="12" r="2" />
        </svg>
      );
    case "footswitch":
      return (
        <svg {...props}>
          <circle cx="7" cy="12" r="3" />
          <circle cx="17" cy="12" r="3" />
          <line x1="3" y1="18" x2="21" y2="18" />
        </svg>
      );
    case "gauge":
      return (
        <svg {...props}>
          <path d="M4 14a8 8 0 0 1 16 0" />
          <path d="M12 14l4-3" />
          <circle cx="12" cy="14" r="1.2" fill="currentColor" stroke="none" />
        </svg>
      );
    case "cable":
      return (
        <svg {...props}>
          <g
            transform="scale(0.0896861) translate(-67.3 -98.6) rotate(-135 197.5 236)"
            strokeWidth={strokeWidth / 0.0896861}
          >
            <path d="M197.5 64 L197.5 148" />
            <path d="M187 150 L208 150" />
            <path d="M161 200 Q161 152 197.5 152 Q234 152 234 200 L234 286 Q234 304 217 304 L178 304 Q161 304 161 286 Z" />
            <line x1="181" y1="350" x2="214" y2="350" />
            <path d="M181 304 L181 394 Q181 404 189 409 L197.5 414 L206 409 Q214 404 214 394 L214 304" />
          </g>
        </svg>
      );
    case "music":
      return (
        <svg {...props}>
          <path d="M9 18V5l11-2v13" />
          <circle cx="6" cy="18" r="3" />
          <circle cx="17" cy="16" r="3" />
        </svg>
      );
    case "spinner":
      return (
        <svg {...props}>
          <path d="M12 3a9 9 0 1 0 9 9" />
        </svg>
      );
    case "download":
      return (
        <svg {...props}>
          <path d="M12 4v11M7 10l5 5 5-5" />
          <path d="M5 19h14" />
        </svg>
      );
    case "warn-tri":
      return (
        <svg {...props}>
          <path d="M10.3 4l-7 12a2 2 0 0 0 1.7 3h14a2 2 0 0 0 1.7-3l-7-12a2 2 0 0 0-3.4 0z" />
          <path d="M12 9v4M12 16v.5" />
        </svg>
      );
    case "info":
      return (
        <svg {...props}>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 11.5v4.5" />
          <path d="M12 8v.4" />
        </svg>
      );
    case "refresh":
      return (
        <svg {...props}>
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M8 16H3v5" />
        </svg>
      );
    case "undo":
      return (
        <svg {...props}>
          <path d="M9 14 4 9l5-5" />
          <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H9" />
        </svg>
      );
    case "redo":
      return (
        <svg {...props}>
          <path d="m15 14 5-5-5-5" />
          <path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H15" />
        </svg>
      );
    case "grip":
      return (
        <svg {...props}>
          <circle cx="9" cy="6" r="1.3" fill="currentColor" stroke="none" />
          <circle cx="15" cy="6" r="1.3" fill="currentColor" stroke="none" />
          <circle cx="9" cy="12" r="1.3" fill="currentColor" stroke="none" />
          <circle cx="15" cy="12" r="1.3" fill="currentColor" stroke="none" />
          <circle cx="9" cy="18" r="1.3" fill="currentColor" stroke="none" />
          <circle cx="15" cy="18" r="1.3" fill="currentColor" stroke="none" />
        </svg>
      );
    case "trash":
      return (
        <svg {...props}>
          <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13" />
        </svg>
      );
    case "lock":
      return (
        <svg {...props}>
          <rect x="5" y="11" width="14" height="9" rx="1.5" />
          <path d="M8 11V8a4 4 0 0 1 8 0v3" />
        </svg>
      );
    case "shield":
      return (
        <svg {...props}>
          <path d="M12 2l7 4v5c0 5.25-3.5 9.74-7 11-3.5-1.26-7-5.75-7-11V6l7-4z" />
          <path d="M9 12l2 2 4-4" />
        </svg>
      );
    case "play":
      return (
        <svg {...props}>
          <path d="M8 5.5 L18 12 L8 18.5 Z" />
        </svg>
      );
    case "pause":
      return (
        <svg {...props}>
          <line x1="9" y1="5" x2="9" y2="19" />
          <line x1="15" y1="5" x2="15" y2="19" />
        </svg>
      );
    default:
      return null;
  }
}
