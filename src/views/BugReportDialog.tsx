// src/views/BugReportDialog.tsx — "Report a bug" (look B, a 460-wide Dialog), opened from
// the native Help menu (bootstrap.rs emits tmp://open-bug-report; App listens) and from
// Settings › About and updates. Bundles recent logs + captured device settings + app info
// (+ an opt-in preset) and either sends it straight to the maintainer (sendReport.ts) or
// saves it to Downloads for a manual report — falling back to the save automatically when
// sending fails (no endpoint configured, network error, non-200).
//
// What happened shows as one Notice above the buttons, in plain words: no file path, no
// backend text (a failed save is already in the log, which is what the bundle carries).
// The close (x) button stays at the top right; the backdrop closes it too (it writes
// nothing to the unit).
//
// The optional preset is sourced from the SHARED library scan store (never triggers a
// scan). The store keeps a parsed signal `graph` per preset, not the raw device
// presetJson — so the bundle's preset.json is that graph (see the deviation note
// where it's stringified).

import { useState, useSyncExternalStore } from "react";

import { useTheme } from "../theme/ThemeContext";
import { sectionLabel } from "../theme/tokens";
import { Dialog, DialogHeader, DialogBody, DialogFooter } from "../ui/Dialog";
import { Button, Notice, TextArea, type NoticeTone } from "../ui/primitives";
import { Select } from "../ui/Select";
import { saveSupportBundle, buildSupportBundle } from "../lib/invoke";
import { subscribeLibraryScan, getLibraryScan } from "../device/libraryScan";
import { sendReport, type SendReportOutcome } from "./sendReport";
import { REPORT_ENDPOINT } from "../lib/reportEndpoint";

const NO_PRESET_ID = "none";
const TITLE = "Report a bug";

/** How the last save or send ended. */
type Outcome =
  | { kind: "sent"; reportId: number }
  | { kind: "fellBack" }
  | { kind: "saved" }
  | { kind: "failed" };

function outcomeNotice(o: Outcome): { tone: NoticeTone; text: string } {
  switch (o.kind) {
    case "sent":
      return {
        tone: "info",
        text: `Report ${String(o.reportId)} sent. Keep this number in case you need to ask about it.`,
      };
    case "fellBack":
      return {
        tone: "caution",
        text: "Couldn't send the report, so it's saved in your Downloads folder. You can send it yourself.",
      };
    case "saved":
      return { tone: "info", text: "Report saved in your Downloads folder." };
    case "failed":
      return { tone: "danger", text: "Couldn't save the report. Try again." };
  }
}

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
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [description, setDescription] = useState("");

  const canSend = REPORT_ENDPOINT !== "";
  const hasPresets = lib.presets.length > 0;
  // Connected, but the shared scan hasn't settled: the presets are on their way.
  const readingPresets = !hasPresets && connected && !lib.ready;
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
  async function localSave(): Promise<void> {
    const { presetJson, presetName } = pickedPresetArgs();
    await saveSupportBundle(firmware, presetJson, presetName);
  }

  async function save() {
    setBusy("save");
    setOutcome(null);
    try {
      await localSave();
      setOutcome({ kind: "saved" });
    } catch {
      // Already in the log (the invoke wrapper logs every failed command).
      setOutcome({ kind: "failed" });
    } finally {
      setBusy(null);
    }
  }

  async function send() {
    setBusy("send");
    setOutcome(null);
    try {
      let sent: SendReportOutcome = { ok: false };
      try {
        const { presetJson, presetName } = pickedPresetArgs();
        const bundleBytes = await buildSupportBundle(
          firmware,
          presetJson,
          presetName,
        );
        sent = await sendReport({
          description: description.trim(),
          meta: { firmware },
          bundleBytes,
        });
      } catch {
        // Building the bundle or reaching the endpoint failed — fall through
        // to the local-save fallback below (never a hard failure to the user).
        sent = { ok: false };
      }
      if (sent.ok) {
        setOutcome({ kind: "sent", reportId: sent.reportId });
        return;
      }
      await localSave();
      setOutcome({ kind: "fellBack" });
    } catch {
      setOutcome({ kind: "failed" });
    } finally {
      setBusy(null);
    }
  }

  const notice = outcome != null ? outcomeNotice(outcome) : null;
  const describeFirst = canSend && description.trim().length === 0;

  return (
    <Dialog size="sm" onClose={onClose} label={TITLE}>
      <DialogHeader>
        <h2
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
            color: t.text,
          }}
        >
          {TITLE}
        </h2>
        <Button
          variant="plain"
          size="sm"
          icon="x"
          aria-label="Close"
          onClick={onClose}
        />
      </DialogHeader>

      <DialogBody>
        <div style={{ display: "flex", flexDirection: "column", gap: t.sp12 }}>
          <p
            style={{
              margin: 0,
              lineHeight: `${String(t.lineBody + 1)}px`,
              color: t.textSecondary,
            }}
          >
            {canSend
              ? "Describe what happened. The report adds recent logs, your Tone Master Pro's settings and the app version."
              : "The report adds recent logs, your Tone Master Pro's settings and the app version."}
          </p>

          <div style={{ display: "flex", flexDirection: "column", gap: t.sp4 }}>
            <span style={sectionLabel(t)}>Include a preset</span>
            {hasPresets ? (
              <Select
                label="Include a preset"
                width={220}
                value={pickedSlot == null ? NO_PRESET_ID : String(pickedSlot)}
                options={[
                  { value: NO_PRESET_ID, label: "No preset" },
                  ...lib.presets.map((p) => ({
                    value: String(p.slot),
                    label: p.name,
                  })),
                ]}
                onChange={(id) => {
                  setPickedSlot(id === NO_PRESET_ID ? null : Number(id));
                }}
              />
            ) : readingPresets ? (
              <Select
                label="Include a preset"
                width={220}
                value=""
                placeholder="Reading presets…"
                attention={false}
                disabled
                options={[]}
                onChange={() => undefined}
              />
            ) : (
              <>
                <Select
                  label="Include a preset"
                  width={220}
                  value={NO_PRESET_ID}
                  options={[{ value: NO_PRESET_ID, label: "No preset" }]}
                  disabled
                  onChange={() => undefined}
                />
                {!connected && (
                  <span
                    style={{
                      fontSize: t.fontCaption,
                      lineHeight: `${String(t.lineCaption)}px`,
                      color: t.textSecondary,
                    }}
                  >
                    Connect your Tone Master Pro to include a preset.
                  </span>
                )}
              </>
            )}
          </div>

          {canSend && (
            <TextArea
              label="What happened?"
              placeholder="What did you expect to happen?"
              value={description}
              onChange={setDescription}
            />
          )}

          {notice != null && <Notice tone={notice.tone}>{notice.text}</Notice>}
        </div>
      </DialogBody>

      <DialogFooter
        start={
          describeFirst ? (
            <span
              style={{
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                color: t.textSecondary,
              }}
            >
              Describe what happened first.
            </span>
          ) : undefined
        }
      >
        <Button
          variant={canSend ? "secondary" : "primary"}
          busy={busy === "save"}
          disabled={busy != null}
          onClick={() => {
            void save();
          }}
        >
          Save to Downloads
        </Button>
        {canSend && (
          <Button
            variant="primary"
            busy={busy === "send"}
            disabled={busy != null || describeFirst}
            onClick={() => {
              void send();
            }}
          >
            Send report
          </Button>
        )}
      </DialogFooter>
    </Dialog>
  );
}
