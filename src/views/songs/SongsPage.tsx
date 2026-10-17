// src/views/songs/SongsPage.tsx — the Songs tab's content column (look B): the shared
// `PageColumn` (title row over a scrolling body), 720 px at most and left-aligned.

import type { ReactNode } from "react";

import { PageColumn } from "../../ui/PageColumn";

/** The widest the Songs content column grows (Settings uses 640). */
const SONGS_COLUMN_MAX = 720;

export interface SongsPageProps {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

export function SongsPage(props: SongsPageProps) {
  return <PageColumn maxWidth={SONGS_COLUMN_MAX} {...props} />;
}
