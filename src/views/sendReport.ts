// sendReport.ts — POSTs a bug report (description + identity meta + the tar
// bundle) to the maintainer's Cloudflare Worker. Pure aside from `fetch`, so
// it's unit-testable with fetch mocked; BugReportDialog owns all UI state and
// the local-save fallback on a non-`ok` outcome.

import { REPORT_ENDPOINT, REPORT_TOKEN } from "../lib/reportEndpoint";

/** Outer meta lets the Worker index a report without untarring; app/version
 * already ride inside the bundle's meta.json, so only firmware travels here. */
export interface SendReportMeta {
  firmware: string | null;
}

export interface SendReportInput {
  description: string;
  meta: SendReportMeta;
  bundleBytes: ArrayBuffer;
}

export type SendReportOutcome = { ok: true; reportId: number } | { ok: false };

function isReportIdBody(v: unknown): v is { reportId: number } {
  return (
    typeof v === "object" &&
    v !== null &&
    typeof (v as { reportId?: unknown }).reportId === "number"
  );
}

/** Bound the whole request — a hung Worker must fall back, not freeze Send. */
const SEND_TIMEOUT_MS = 15_000;

/** POST the report; `{ ok: false }` on ANY failure (no endpoint configured,
 * network error, timeout, non-200, or an unexpected response body) — the
 * caller falls back to a local save on any non-`ok` outcome, so this never
 * throws. */
export async function sendReport({
  description,
  meta,
  bundleBytes,
}: SendReportInput): Promise<SendReportOutcome> {
  if (REPORT_ENDPOINT === "") return { ok: false };

  const form = new FormData();
  form.append("description", description);
  form.append("meta", JSON.stringify(meta));
  form.append(
    "bundle",
    new File([bundleBytes], "report.tar", { type: "application/x-tar" }),
  );

  try {
    const res = await fetch(`${REPORT_ENDPOINT}/report`, {
      method: "POST",
      headers: { "x-report-token": REPORT_TOKEN },
      body: form,
      signal: AbortSignal.timeout(SEND_TIMEOUT_MS),
    });
    if (!res.ok) return { ok: false };
    const body: unknown = await res.json();
    if (!isReportIdBody(body)) return { ok: false };
    return { ok: true, reportId: body.reportId };
  } catch {
    return { ok: false };
  }
}
