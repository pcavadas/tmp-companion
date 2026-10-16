// src/views/overlays/WizardFooter.tsx — the wizard's two-slot footer (secondary left ·
// primary right), the same slots on every step: the DS `Footer`.

import type { ReactNode } from "react";

import { Footer } from "../../ui/Footer";

// ---- footer (two fixed slots) ----------------------------------------------
export interface WizardFooterProps {
  left: ReactNode;
  right: ReactNode;
}

export function WizardFooter({ left, right }: WizardFooterProps) {
  return <Footer left={left} right={right} />;
}
