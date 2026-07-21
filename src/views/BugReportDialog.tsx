// src/views/BugReportDialog.tsx — "Report a Bug" dialog, opened from the native Help
// menu (bootstrap.rs emits tmp://open-bug-report on click; App.tsx listens and mounts
// this). Bundles recent logs + captured device settings + app info (+ an opt-in
// preset) and either sends it straight to the maintainer (sendReport.ts) or saves it
// to Downloads for a manual report — falling back to the save automatically when
// sending fails (no endpoint configured, network error, non-200).
//
// The optional preset is sourced from the SHARED library scan store (never triggers a
// scan). The store keeps a parsed signal `graph` per preset, not the raw device
// presetJson — so the bundle's preset.json is that graph (see the deviation note
// where it's stringified).

import { useState, useSyncExternalStore } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Dialog, DialogHeader, DialogBody, DialogFooter } from "../ui/Dialog";
import { AlertBanner, Button } from "../ui/primitives";
import { Icon } from "../ui/Icon";
import { Pick } from "./overlays/Pick";
import { plainInput } from "../theme/tokens";
import { saveSupportBundle, buildSupportBundle } from "../lib/invoke";
import { errMsg } from "../lib/format";
import { subscribeLibraryScan, getLibraryScan } from "./level/libraryScan";
import { sendReport, type SendReportOutcome } from "./sendReport";
import { REPORT_ENDPOINT } from "../lib/reportEndpoint";

const NO_PRESET_ID = "none";

interface BugReportDialogProps {
  connected: boolean;
  /** Connected unit's firmware version (null while disconnected). */
  firmware: string | null;
  onClose: () => void;
}

export function BugReportDialog({
  connected,
  firmware,
  onClose,
}: BugReportDialogProps) {
  const { t } = useTheme();
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);

  const [pickedSlot, setPickedSlot] = useState<number | null>(null);
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
    setReportId(null);
    setSendFallbackPath(null);
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
    setSavedPath(null);
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
    <Dialog size="sm" onClose={onClose} label="Report a Bug">
      <DialogHeader>
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: t.space4,
          }}
        >
          <span
            style={{
              width: 24,
              height: 24,
              borderRadius: 7,
              background: t.accentSoft,
              display: "grid",
              placeItems: "center",
            }}
          >
            <Icon name="info" size={15} stroke={t.accentDeep} />
          </span>
          <span style={{ fontFamily: t.serif, fontSize: 18, color: t.ink }}>
            Report a Bug
          </span>
        </span>
        <button
          type="button"
          onClick={onClose}
          title="Close"
          aria-label="Close"
          style={{
            border: "none",
            background: "transparent",
            cursor: "pointer",
            display: "flex",
            padding: t.space2,
            color: t.faint,
          }}
        >
          <Icon name="x" size={16} />
        </button>
      </DialogHeader>

      <DialogBody>
        <div
          style={{ display: "flex", flexDirection: "column", gap: t.space5 }}
        >
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsUi,
              color: t.ink2,
              lineHeight: 1.5,
              textWrap: "pretty",
            }}
          >
            Bundles recent logs, device settings, and app info into a file —
            describe what happened below and send it straight to the maintainer,
            or save it to Downloads to attach manually.
          </div>

          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: t.space5,
            }}
          >
            <span
              style={{ fontFamily: t.sans, fontSize: t.fsUi, color: t.ink2 }}
            >
              Include a preset
            </span>
            {hasPresets ? (
              // Portals through DialogCardCtx (every <Dialog> provides it) so the
              // dropdown never clips past the scrolling DialogBody — the same
              // mechanism the leveling wizard's Set-up step uses.
              <Pick
                value={pickedSlot == null ? NO_PRESET_ID : String(pickedSlot)}
                options={[
                  { id: NO_PRESET_ID, label: "No preset" },
                  ...lib.presets.map((p) => ({
                    id: String(p.slot),
                    label: p.name,
                  })),
                ]}
                onChange={(id) => {
                  setPickedSlot(id === NO_PRESET_ID ? null : Number(id));
                }}
              />
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

          {REPORT_ENDPOINT !== "" && (
            <textarea
              value={description}
              onChange={(e) => {
                setDescription(e.target.value);
              }}
              placeholder="What happened? What did you expect?"
              aria-label="What happened? What did you expect?"
              rows={4}
              style={plainInput(t, {
                border: `0.5px solid ${t.hairlineStrong}`,
                borderRadius: t.rMd,
                padding: `${String(t.space5)}px ${String(t.space6)}px`,
                fontFamily: t.sans,
                fontSize: t.fsUi,
                resize: "vertical",
              })}
            />
          )}

          {error != null && (
            <AlertBanner>Something went wrong: {error}</AlertBanner>
          )}

          {reportId != null && (
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsUi,
                color: t.accentDeep,
                fontWeight: 500,
              }}
            >
              Report #{reportId} sent — quote this ID if you contact me or want
              your data deleted.
            </div>
          )}

          {sendFallbackPath != null && (
            <AlertBanner>
              Couldn&apos;t send — the bundle was saved to {sendFallbackPath}{" "}
              instead; you can send it manually.
            </AlertBanner>
          )}

          {savedPath != null && (
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsUi,
                color: t.accentDeep,
                fontWeight: 500,
              }}
            >
              Bundle saved to {savedPath}.
            </div>
          )}
        </div>
      </DialogBody>

      <DialogFooter>
        <Button
          variant="ghost"
          small
          disabled={busy != null}
          onClick={() => {
            void save();
          }}
        >
          {busy === "save" ? "Saving…" : "Save bundle"}
        </Button>
        {REPORT_ENDPOINT !== "" && (
          <Button
            variant="primary"
            small
            disabled={busy != null || description.trim().length === 0}
            onClick={() => {
              void send();
            }}
          >
            {busy === "send" ? "Sending…" : "Send report"}
          </Button>
        )}
      </DialogFooter>
    </Dialog>
  );
}

export default BugReportDialog;
