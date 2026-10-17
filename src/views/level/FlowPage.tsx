// src/views/level/FlowPage.tsx — the one page frame of a flow (look B): Level's Set up, run
// and Summary, and Doctor's Instrument, Check and Results pages, are each a page in their
// tab's content area, under the toolbar.
//
// Top to bottom: a main area on the window grey (padding 16 24 20, 14 between its parts,
// at most 960 wide and centred on a wider window) that starts with the page title and the
// flow's Steps at its right, then the page's own content; then the page's Footer, which
// never moves. By default the whole main area scrolls (Set up, Summary). `fill` keeps the
// title and the top of the content still and leaves the scrolling to the content's own
// list (the run page, whose list scrolls under the now card).

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { PageHead } from "../../ui/PageHead";
import { Steps } from "../../ui/Steps";
import { LEVEL_STEPS } from "./setupLayout";

/** The content column's widest (px) by default: wider windows centre it. */
const MAX_CONTENT_W = 960;

export interface FlowPageProps {
  title: ReactNode;
  /** A line under the title, only when it adds a fact. */
  subtitle?: ReactNode;
  /** The current step of `steps` (Level: 0 Set up, 1 Level, 2 Summary). */
  step: number;
  /** The flow's steps, shown at the title's right. Default `LEVEL_STEPS`. */
  steps?: readonly string[];
  /** The content column's widest (px). Default 960. */
  maxWidth?: number;
  /** Accessible name of the page's region, when it needs one. */
  label?: string;
  /** Space between the main area's parts (px). Default 14. */
  gap?: number;
  /** The main area does not scroll; the content scrolls its own list. */
  fill?: boolean;
  children: ReactNode;
  /** The page's Footer. */
  footer: ReactNode;
}

export function FlowPage({
  title,
  subtitle,
  step,
  steps = LEVEL_STEPS,
  maxWidth = MAX_CONTENT_W,
  label,
  gap,
  fill = false,
  children,
  footer,
}: FlowPageProps) {
  const { t } = useTheme();
  const padX = t.sp24;
  return (
    <div
      role={label != null ? "region" : undefined}
      aria-label={label}
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: t.window,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          overflowY: fill ? "hidden" : "auto",
        }}
      >
        <div
          style={{
            width: "100%",
            maxWidth: maxWidth + 2 * padX,
            margin: "0 auto",
            boxSizing: "border-box",
            padding: `${String(t.sp16)}px ${String(padX)}px ${String(t.sp16 + t.sp4)}px`,
            display: "flex",
            flexDirection: "column",
            gap: gap ?? t.sp12 + t.sp2,
            ...(fill ? { flex: 1, minHeight: 0 } : { flexShrink: 0 }),
          }}
        >
          <PageHead
            title={title}
            subtitle={subtitle}
            actions={<Steps steps={steps} current={step} />}
          />
          {children}
        </div>
      </div>
      {footer}
    </div>
  );
}
