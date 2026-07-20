// src/views/settings/SupportSection.tsx — Settings "Support" section: a one-click
// "Save support bundle" that writes recent logs + captured device settings + app
// info (+ an opt-in preset) to a .tar in Downloads, for a manual bug report.
//
// The optional preset is sourced from the SHARED library scan store (never
// triggers a scan). The store keeps a parsed signal `graph` per preset, not the
// raw device presetJson — so the bundle's preset.json is that graph (see the
// deviation note where it's stringified). Disconnected / pre-scan → the picker is
// a disabled empty state.

import { useState, useSyncExternalStore } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { AlertBanner, Button, Toast, MenuItem } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { saveSupportBundle } from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import { subscribeLibraryScan, getLibraryScan } from "../level/libraryScan";

interface SupportSectionProps {
  connected: boolean;
  /** Connected unit's firmware version (null while disconnected). */
  firmware: string | null;
}

export function SupportSection({ connected, firmware }: SupportSectionProps) {
  const { t } = useTheme();
  const s = useStyles();
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);

  const [pickedSlot, setPickedSlot] = useState<number | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const hasPresets = lib.presets.length > 0;
  const pickedPreset =
    pickedSlot == null
      ? null
      : (lib.presets.find((p) => p.slot === pickedSlot) ?? null);

  async function save() {
    setSaving(true);
    setError(null);
    setSavedPath(null);
    const graph =
      pickedPreset != null
        ? lib.graphByIndex.get(pickedPreset.slot)
        : undefined;
    try {
      // presetJson: the parsed graph the store already holds (no device read).
      const res = await saveSupportBundle(
        firmware,
        graph ? JSON.stringify(graph) : null,
        pickedPreset?.name ?? null,
      );
      setSavedPath(res.path);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div style={{ marginTop: t.space10 }}>
      <div style={s.kicker(t.accentDeep)}>Support</div>
      <div style={{ display: "flex", flexDirection: "column", gap: t.space5 }}>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsUi,
            color: t.ink2,
            lineHeight: 1.5,
            maxWidth: 480,
            textWrap: "pretty",
          }}
        >
          Bundles recent logs, device settings, and app info into a file in
          Downloads — attach it when reporting a problem.
        </div>

        {/* Optional preset row */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: t.space5,
          }}
        >
          <span style={{ fontFamily: t.sans, fontSize: t.fsUi, color: t.ink2 }}>
            Include a preset
          </span>
          {hasPresets ? (
            <span style={{ position: "relative" }}>
              <Button
                variant="ghost"
                small
                onClick={() => {
                  setMenuOpen((o) => !o);
                }}
              >
                {pickedPreset?.name ?? "No preset"}
              </Button>
              {menuOpen && (
                <Menu
                  onClose={() => {
                    setMenuOpen(false);
                  }}
                  minWidth={200}
                >
                  <MenuItem
                    label="No preset"
                    onClick={() => {
                      setPickedSlot(null);
                      setMenuOpen(false);
                    }}
                  />
                  {lib.presets.map((p) => (
                    <MenuItem
                      key={p.slot}
                      label={p.name}
                      onClick={() => {
                        setPickedSlot(p.slot);
                        setMenuOpen(false);
                      }}
                    />
                  ))}
                </Menu>
              )}
            </span>
          ) : (
            <span
              title="Connect the Tone Master Pro to include a preset"
              style={{
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                color: t.faint,
                fontStyle: "italic",
              }}
            >
              {connected ? "reading library…" : "connect to include a preset"}
            </span>
          )}
        </div>

        <Button
          variant="ghost"
          small
          disabled={saving}
          onClick={() => {
            void save();
          }}
          style={{ alignSelf: "flex-start" }}
        >
          {saving ? "Saving…" : "Save support bundle"}
        </Button>

        {error != null && (
          <AlertBanner>Could not save the bundle: {error}</AlertBanner>
        )}
      </div>

      {savedPath != null && (
        <Toast
          status="success"
          title="Support bundle saved"
          message={savedPath}
          onDismiss={() => {
            setSavedPath(null);
          }}
        />
      )}
    </div>
  );
}

export default SupportSection;
