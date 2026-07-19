// Bug-report Cloudflare Worker endpoint + shared auth token.
//
// Placeholder until the Worker is deployed — fill in from the maintainer's
// `wrangler deploy` output (the Worker URL) and its SHARED_TOKEN. An empty
// REPORT_ENDPOINT means sending is treated as unavailable: the Send button
// still renders, but clicking it goes straight to the local-save fallback
// (see sendReport.ts) so the UI stays shippable pre-deploy.

export const REPORT_ENDPOINT = "" as string;
// NB: a client-shipped token is NEVER a secret (extractable from the app, and
// visible in this public repo) — it is an abuse speed bump, not auth; the real
// secret (the GitHub token) lives server-side in the Worker. Don't "protect"
// this with a CI-injected env var thinking that hides it.
export const REPORT_TOKEN = "" as string;
