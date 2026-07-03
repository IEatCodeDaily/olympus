import React from "react";

const paths: Record<string, React.ReactNode> = {
  "panel-left": <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M9 3v18" /></>,
  "message-square": <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2z" />,
  "folder": <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.6 3.9A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />,
  "book": <><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20V4a2 2 0 0 0-2-2H6.5A2.5 2.5 0 0 0 4 4.5z" /><path d="M20 17v5H6.5a2.5 2.5 0 0 1 0-5" /></>,
  "pin": <><path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z" /></>,
  "bell": <><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.9 1.9 0 0 0 3.4 0" /></>,
  "check": <path d="M20 6 9 17l-5-5" />,
  "alert": <><circle cx="12" cy="12" r="9" /><path d="M12 8v5M12 16h.01" /></>,
  "file": <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></>,
  "kanban": <><path d="M6 5v11M12 5v6M18 5v14" /><rect x="3" y="3" width="18" height="18" rx="2" /></>,
  "workflow": <><rect x="3" y="3" width="6" height="6" rx="1" /><rect x="15" y="15" width="6" height="6" rx="1" /><path d="M6 9v3a3 3 0 0 0 3 3h6" /></>,
  "puzzle": <path d="M19.4 14a1.6 1.6 0 0 0-1.4-1.6 1.6 1.6 0 0 1 0-3.2A1.6 1.6 0 0 0 19.4 8V6a2 2 0 0 0-2-2h-2a1.6 1.6 0 0 1-1.6-1.4 1.6 1.6 0 0 0-3.2 0A1.6 1.6 0 0 1 9 4H7a2 2 0 0 0-2 2v2a1.6 1.6 0 0 0 1.4 1.6 1.6 1.6 0 0 1 0 3.2A1.6 1.6 0 0 0 5 16v2a2 2 0 0 0 2 2h2a1.6 1.6 0 0 0 1.6-1.4 1.6 1.6 0 0 1 3.2 0A1.6 1.6 0 0 0 17 20h2a2 2 0 0 0 2-2z" />,
  "server": <><rect x="2" y="3" width="20" height="8" rx="1" /><rect x="2" y="13" width="20" height="8" rx="1" /><path d="M6 7h.01M6 17h.01" /></>,
  "gear": <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8 2 2 0 1 1-2.8 2.8 1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5 2 2 0 0 1-4 0 1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3 2 2 0 1 1-2.8-2.8 1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1 2 2 0 0 1 0-4 1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8 2 2 0 1 1 2.8-2.8 1.6 1.6 0 0 0 1.8.3 1.6 1.6 0 0 0 1-1.5 2 2 0 0 1 4 0 1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3 2 2 0 1 1 2.8 2.8 1.6 1.6 0 0 0-.3 1.8 1.6 1.6 0 0 0 1.5 1 2 2 0 0 1 0 4 1.6 1.6 0 0 0-1.5 1z" /></>,
  "search": <><circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" /></>,
  "chevron-down": <path d="m6 9 6 6 6-6" />,
  "chevron-right": <path d="m9 18 6-6-6-6" />,
  "chevron-left": <path d="m15 18-6-6 6-6" />,
  "plus": <path d="M12 5v14M5 12h14" />,
  "bot": <><rect x="3" y="8" width="18" height="12" rx="2" /><path d="M12 8V4M9 14h.01M15 14h.01" /></>,
  "activity": <path d="M22 12h-4l-3 9L9 3l-3 9H2" />,
  "panel-bottom": <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M3 15h18" /></>,
  "panel-right": <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M15 3v18" /></>,
  "ellipsis": <><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>,
  "paperclip": <path d="m21 8-9.5 9.5a3.5 3.5 0 0 1-5-5L14 4a2.5 2.5 0 0 1 3.5 3.5L9 16" />,
  "git-branch": <><line x1="6" y1="3" x2="6" y2="15" /><circle cx="18" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M18 9a9 9 0 0 1-9 9" /></>,
  "arrow-up": <path d="M12 19V5M5 12l7-7 7 7" />,
  "trash": <path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />,
  "layout-grid": <><rect x="3" y="3" width="7" height="7" rx="1" /><rect x="14" y="3" width="7" height="7" rx="1" /><rect x="14" y="14" width="7" height="7" rx="1" /><rect x="3" y="14" width="7" height="7" rx="1" /></>,
  "list": <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />,
  "settings-2": <path d="M20 7h-9M14 17H5M17 3a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM7 13a2 2 0 1 0 0 4 2 2 0 0 0 0-4z" />,
  "globe": <><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z" /></>,
  "git-compare": <><circle cx="6" cy="18" r="3" /><circle cx="18" cy="6" r="3" /><path d="M18 9v3a3 3 0 0 1-3 3H9M6 15v-3a3 3 0 0 1 3-3h6" /></>,
  "sparkles": <path d="M12 3l1.6 4.6L18 9l-4.4 1.4L12 15l-1.6-4.6L6 9l4.4-1.4z" />,
  "archive": <><rect x="3" y="4" width="18" height="4" rx="1" /><path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4" /></>,
  "copy": <><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></>,
  "at": <><circle cx="12" cy="12" r="4" /><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8" /></>,
  "hash": <><line x1="4" y1="9" x2="20" y2="9" /><line x1="4" y1="15" x2="20" y2="15" /><line x1="10" y1="3" x2="8" y2="21" /><line x1="16" y1="3" x2="14" y2="21" /></>,
  "slash": <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="m9 15 6-6" /></>,
  "cpu": <><rect x="4" y="4" width="16" height="16" rx="2" /><rect x="9" y="9" width="6" height="6" /><path d="M9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3" /></>,
  "shield": <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />,
  "send": <path d="M12 19V5M5 12l7-7 7 7" />,
  "x": <><path d="M18 6 6 18" /><path d="m6 6 12 12" /></>,
  "brain": <path d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18ZM12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18ZM12 5v13" />,
  "zap": <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />,
  "clock": <><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></>,
  "terminal": <><path d="m4 17 6-6-6-6" /><path d="M12 19h8" /></>,
  "dollar": <path d="M12 2v20M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6" />,
};

export type IconName = keyof typeof paths;

export function Icon({
  name,
  size = 14,
  className,
  style,
}: {
  name: IconName;
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={style}
      aria-hidden="true"
    >
      {paths[name] ?? null}
    </svg>
  );
}
