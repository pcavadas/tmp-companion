// src/views/Disclaimer.tsx — the first-run page (look B): the only thing the app shows
// until it is accepted, with no toolbar. One white card centred on the window grey, 520
// wide: the backup title, what saves to the unit and how to get presets back, then the
// non-affiliation and liability line ("You use TMP Companion at your own risk." keeps
// its words). Bottom row: "Don't show this again" on the left, the primary "I've backed
// up, continue" on the right. Accepting always sets the session key; the permanent key
// only when the box is ticked (App owns both).

import { useState } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Button, Checkbox } from "../ui/primitives";

interface DisclaimerProps {
  onAccept: (permanent: boolean) => void;
}

export function Disclaimer({ onAccept }: DisclaimerProps) {
  const { t } = useTheme();
  const [dontShow, setDontShow] = useState(false);
  const para = {
    margin: 0,
    fontSize: t.fontBody,
    lineHeight: `${String(t.lineBody + 1)}px`,
  };

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: t.sp24,
        overflowY: "auto",
      }}
    >
      <div
        style={{
          width: 520,
          maxWidth: "100%",
          boxSizing: "border-box",
          padding: t.sp24 + t.sp4,
          background: t.surface,
          border: `1px solid ${t.border}`,
          borderRadius: t.radiusGroup,
          display: "flex",
          flexDirection: "column",
          gap: t.sp12 + t.sp2,
          fontFamily: t.fontSans,
          color: t.text,
        }}
      >
        <h1
          style={{
            margin: 0,
            fontSize: t.fontTitle,
            lineHeight: `${String(t.lineTitle)}px`,
            fontWeight: t.weightSemibold,
          }}
        >
          Back up your Tone Master Pro first
        </h1>
        <p style={para}>
          Leveling, Copy and Songs save changes to your Tone Master Pro right
          away, and they can&apos;t be undone here. A backup made in Fender Tone
          Master Pro Control is how you get your presets back.
        </p>
        <p style={{ ...para, color: t.textSecondary }}>
          This app isn&apos;t made by Fender. It only talks to your own Tone
          Master Pro, over USB. You use TMP Companion at your own risk.
        </p>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: t.sp12,
            marginTop: t.sp8,
          }}
        >
          <Checkbox
            checked={dontShow}
            label="Don't show this again"
            onChange={setDontShow}
          />
          <Button
            variant="primary"
            size="lg"
            onClick={() => {
              onAccept(dontShow);
            }}
          >
            I&apos;ve backed up, continue
          </Button>
        </div>
      </div>
    </div>
  );
}
