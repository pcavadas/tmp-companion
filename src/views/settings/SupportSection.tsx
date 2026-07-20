// src/views/settings/SupportSection.tsx — Settings "Support" section: a one-click
// "Save support bundle" that writes recent logs + captured device settings + app
// info (+ an opt-in preset) to a .tar in Downloads, for a manual bug report; plus
// a "Send a report" block that POSTs the same bundle + a description straight to
// the maintainer (sendReport.ts), falling back to the local save automatically
// when sending fails (no endpoint configured, network error, non-200).
//
// The optional preset is sourced from the SHARED library scan store (never
// triggers a scan). The store keeps a parsed signal `graph` per preset, not the
// raw device presetJson — so the bundle's preset.json is that graph (see the
// deviation note where it's stringified). Disconnected / pre-scan → the picker is
// a disabled empty state. The picker is SHARED between the save and send flows.

import { useState, useSyncExternalStore } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { AlertBanner, Button, Toast, MenuItem } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { plainInput } from "../../theme/tokens";
import { saveSupportBundle, buildSupportBundle } from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import { subscribeLibraryScan, getLibraryScan } from "../level/libraryScan";
import { sendReport, type SendReportOutcome } from "./sendReport";
import { REPORT_ENDPOINT } from "../../lib/reportEndpoint";

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
  const [busy, setBusy] = useState<"save" | "send" | null>(null);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [description, setDescription] = useState("");
  const [reportId, setReportId] = useState<number | null>(null);
  const [sendFallbackPath, setSendFallbackPath] = useState<string | null>(null);

  const hasPresets = lib.presets.length > 0;
  const pickedPreset =
    pickedSlot == null
      ? null
      : (lib.presets.find((p) => p.slot === pickedSlot) ?? null);

  // presetJson: the parsed graph the store already holds (no device read) —
  // shared by both the save and send flows.
  function pickedPresetArgs(): {
    presetJson: string | null;
    presetName: string | null;
  } {
    const graph =
      pickedPreset != null
        ? lib.graphByIndex.get(pickedPreset.slot)
        : undefined;
    return {
      presetJson: graph ? JSON.stringify(graph) : null,
      presetName: pickedPreset?.name ?? null,
    };
  }

  // The one local-save call both flows share (save button + send fallback).
  async function localSave(): Promise<string> {
    const { presetJson, presetName } = pickedPresetArgs();
    return (await saveSupportBundle(firmware, presetJson, presetName)).path;
  }

  async function save() {
    setBusy("save");
    setError(null);
    setSavedPath(null);
    try {
      setSavedPath(await localSave());
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(null);
    }
  }

  async function send() {
    setBusy("send");
    setError(null);
    setReportId(null);
    setSendFallbackPath(null);
    try {
      let outcome: SendReportOutcome = { ok: false };
      try {
        const { presetJson, presetName } = pickedPresetArgs();
        const bundleBytes = await buildSupportBundle(
          firmware,
          presetJson,
          presetName,
        );
        outcome = await sendReport({
          description: description.trim(),
          meta: { firmware },
          bundleBytes,
        });
      } catch {
        // Building the bundle or reaching the endpoint failed — fall through
        // to the local-save fallback below (never a hard failure to the user).
        outcome = { ok: false };
      }
      if (outcome.ok) {
        setReportId(outcome.reportId);
        return;
      }
      setSendFallbackPath(await localSave());
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(null);
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
          disabled={busy != null}
          onClick={() => {
            void save();
          }}
          style={{ alignSelf: "flex-start" }}
        >
          {busy === "save" ? "Saving…" : "Save support bundle"}
        </Button>

        {error != null && (
          <AlertBanner>Something went wrong: {error}</AlertBanner>
        )}

        {/* Send a report — same bundle + preset picker above, sent straight to
            the maintainer; falls back to the local save automatically. Hidden
            until the Worker endpoint is configured (reportEndpoint.ts). */}
        {REPORT_ENDPOINT !== "" && (
          <div
            style={{
              marginTop: t.space6,
              paddingTop: t.space6,
              borderTop: `0.5px solid ${t.hairline}`,
              display: "flex",
              flexDirection: "column",
              gap: t.space5,
            }}
          >
            <div
              style={{ fontFamily: t.sans, fontSize: t.fsUi, color: t.ink2 }}
            >
              Send a report
            </div>

            <textarea
              value={description}
              onChange={(e) => {
                setDescription(e.target.value);
              }}
              placeholder="What happened? What did you expect?"
              rows={4}
              style={plainInput(t, {
                border: `0.5px solid ${t.hairlineStrong}`,
                borderRadius: t.rMd,
                padding: `${String(t.space5)}px ${String(t.space6)}px`,
                fontFamily: t.sans,
                fontSize: t.fsUi,
                resize: "vertical",
                maxWidth: 480,
              })}
            />

            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsLabel,
                color: t.faint,
                maxWidth: 480,
              }}
            >
              Sends the bundle contents listed above to the maintainer.
            </div>

            <Button
              variant="ghost"
              small
              disabled={busy != null || description.trim().length === 0}
              onClick={() => {
                void send();
              }}
              style={{ alignSelf: "flex-start" }}
            >
              {busy === "send" ? "Sending…" : "Send report"}
            </Button>

            {reportId != null && (
              <div
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsUi,
                  color: t.accentDeep,
                  fontWeight: 500,
                }}
              >
                Report #{reportId} sent — quote this ID if you contact me or
                want your data deleted.
              </div>
            )}

            {sendFallbackPath != null && (
              <AlertBanner>
                Couldn&apos;t send — the bundle was saved to {sendFallbackPath}{" "}
                instead; you can send it manually.
              </AlertBanner>
            )}
          </div>
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
