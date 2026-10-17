// src/views/settings/SettingsPage.tsx — one Settings page's content column (look B): the
// shared `PageColumn` (title row over a scrolling body), 640 px at most and
// centred beside the sidebar.

import type { ReactNode } from "react";

import { PageColumn } from "../../ui/PageColumn";

/** The widest a Settings page grows (Songs uses 720). */
const SETTINGS_COLUMN_MAX = 640;

export interface SettingsPageProps {
  title: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}

export function SettingsPage(props: SettingsPageProps) {
  return <PageColumn maxWidth={SETTINGS_COLUMN_MAX} {...props} />;
}
