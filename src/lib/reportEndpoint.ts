// Bug-report Cloudflare Worker endpoint + shared auth token.
//
// Values come from the maintainer's `wrangler deploy` output (the Worker URL)
// and its SHARED_TOKEN secret. An empty REPORT_ENDPOINT means sending is
// treated as unavailable: the Send block is hidden and only the local-save
// flow renders (see sendReport.ts / SupportSection.tsx), so the UI stays
// shippable if the endpoint is ever retired.

export const REPORT_ENDPOINT =
  "https://tmp-companion-reports.pcavadas.workers.dev" as string;
// NB: a client-shipped token is NEVER a secret (extractable from the app, and
// visible in this public repo) — it is an abuse speed bump, not auth; the real
// secret (the GitHub token) lives server-side in the Worker. Don't "protect"
// this with a CI-injected env var thinking that hides it.
export const REPORT_TOKEN =
  "bcc532b444496074a64063ec533f12e6f93d674338566235" as string; // gitleaks:allow — public speed-bump token, not a secret (see NB above)
