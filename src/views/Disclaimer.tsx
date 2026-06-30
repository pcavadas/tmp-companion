import { useState } from "react";

import { useTheme } from "../theme/ThemeContext";
import type { IconName } from "../ui/Icon";
import { Icon } from "../ui/Icon";
import { Button, Checkbox } from "../ui/primitives";

interface DisclaimerProps {
  onAccept: (permanent: boolean) => void;
}

function CalloutRow({
  icon,
  iconColor,
  children,
}: {
  icon: IconName;
  iconColor: string;
  children: React.ReactNode;
}) {
  const { t } = useTheme();
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
      <span style={{ flexShrink: 0, marginTop: 2 }}>
        <Icon name={icon} size={15} stroke={iconColor} strokeWidth={1.8} />
      </span>
      <span
        style={{
          fontFamily: t.sans,
          fontSize: 12.5,
          lineHeight: 1.55,
          color: t.ink2,
        }}
      >
        {children}
      </span>
    </div>
  );
}

export function Disclaimer({ onAccept }: DisclaimerProps) {
  const { t } = useTheme();
  const [dontShow, setDontShow] = useState(false);

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        position: "relative",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          backgroundImage:
            "repeating-linear-gradient(135deg, rgba(15,17,21,0.09) 0 1px, transparent 1px 13px)",
          opacity: 0.45,
          pointerEvents: "none",
        }}
      />

      <div
        style={{
          position: "relative",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          textAlign: "center",
          maxWidth: 460,
          padding: "0 20px",
        }}
      >
        <div
          style={{
            width: 64,
            height: 64,
            borderRadius: 999,
            background: t.bgAlt,
            border: `0.5px solid ${t.hairlineStrong}`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            marginBottom: 28,
          }}
        >
          <Icon
            name="shield"
            size={28}
            stroke={t.accentDeep}
            strokeWidth={1.5}
          />
        </div>

        <div
          style={{
            fontFamily: t.mono,
            fontSize: 10,
            letterSpacing: "0.18em",
            textTransform: "uppercase",
            color: t.faint,
            marginBottom: 8,
          }}
        >
          BEFORE YOU BEGIN
        </div>

        <div
          style={{
            fontFamily: t.serif,
            fontSize: 26,
            lineHeight: 1.15,
            letterSpacing: "-0.01em",
            color: t.ink,
            textWrap: "balance",
          }}
        >
          Back up your Tone Master Pro
        </div>

        <div
          style={{
            fontFamily: t.sans,
            fontSize: 13.5,
            lineHeight: 1.65,
            color: t.mutedInk,
            marginTop: 16,
            maxWidth: 420,
            textWrap: "pretty",
          }}
        >
          Leveling and song editing write changes directly to your device&apos;s
          presets. Please create a full backup using{" "}
          <span style={{ fontFamily: t.mono, fontSize: 12.5, color: t.ink2 }}>
            Fender Tone Master Pro Control
          </span>{" "}
          before proceeding.
        </div>

        <div
          style={{
            marginTop: 24,
            borderRadius: 8,
            border: `0.5px solid ${t.hairline}`,
            background: t.bgAlt,
            padding: "16px 22px",
            maxWidth: 420,
            textAlign: "left",
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <CalloutRow icon="warn-tri" iconColor={t.accentDeep}>
            TMP Companion is not responsible for any data loss. Changes are
            written to the unit in real time and may not be reversible without a
            backup.
          </CalloutRow>
          <CalloutRow icon="shield" iconColor={t.good}>
            This is an independent, unaffiliated tool. It talks to your unit over
            USB and acts only on your own device and presets.
          </CalloutRow>
        </div>

        <div
          style={{
            marginTop: 28,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 16,
          }}
        >
          <Button
            variant="primary"
            onClick={() => {
              onAccept(dontShow);
            }}
          >
            I&apos;ve backed up — continue
          </Button>
          <div
            onClick={() => {
              setDontShow((v) => !v);
            }}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              cursor: "pointer",
              userSelect: "none",
            }}
          >
            <Checkbox checked={dontShow} />
            <span
              style={{ fontFamily: t.sans, fontSize: 12, color: t.mutedInk }}
            >
              Don&apos;t show again
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
