# Driving the app's native window from a Claude session

How to click/inspect the real running app window (the macOS WKWebView) headlessly. For
UI-journey automation prefer the Playwright harness (`bun run e2e`, see
`notes/e2e-test-plan.md`); this path is only for literal native-window pixels.

- **Click with `cliclick`, not `osascript`:** macOS System Events `click at` is rejected
  (`error -25208`). Use `cliclick c:x,y` with **logical points** — `screencapture -R x,y,w,h`
  takes logical coords but the PNG is 2× retina (a 900×680 window → 1800×1360 px), so never
  derive coords by dividing pixel positions.
- **Click by FRACTION, not raw pixels:** the `screencapture -R` PNG renders at varying scale,
  so derive coords from the element's _fractional position within the captured region_
  (`abs_x = origin_x + fx·w`, `abs_y = origin_y + fy·h`), never from displayed-pixel offsets.
- **Raise frontmost BEFORE clicking:** `cliclick` posts to whatever app has focus, so a tap
  silently MISSES (lands on the terminal) when the companion window isn't frontmost — run
  `osascript -e 'tell application "System Events" to tell (first process whose name contains
"tmp-companion") to set frontmost to true'` first, then re-query the bounds (raising can
  move it), then click.
- **Secondary-display caveat:** `cliclick` taps don't land when the window sits on a
  left/negative-x secondary display — move it to the primary display first.
- **Click cadence:** ~500 ms between clicks is enough for the WKWebView to settle.
- **Keyboard input doesn't reach WKWebView:** `cliclick kp:return` does NOT fire a React
  inline-form's `onKeyDown` submit, and neither `cliclick t:<text>` nor `osascript … keystroke`
  enters TEXT into a focused field — **form fields can't be filled headlessly** (open + cancel
  a form to verify it renders; you can't submit one with input). Click the explicit ✓ / submit
  affordance.
- **No scroll in `cliclick`** — warp the cursor over the pane then post a Quartz line-scroll:
  `python3 -c 'import Quartz; Quartz.CGEventPost(Quartz.kCGHIDEventTap,
Quartz.CGEventCreateScrollWheelEvent(None, Quartz.kCGScrollEventUnitLine, 1, -3))'`
  (negative = down).
- **Locked-screen signature:** a locked Mac makes `screencapture -R` print "could not create
  image from rect" and the front-window bounds query return "Invalid index. window 1" — that's
  a locked screen, not a crashed app (`pgrep` it to confirm).
- **Stale-dev caveats:** a `tauri dev` left running from a prior session holds **port 1421**
  (vite's bind), so a fresh `bun run tauri dev` silently fails to start — kill the stale tree
  first (`pkill -f "node_modules/.bin/vite"` + the `target/debug/tmp-companion` app), then
  relaunch. Same-session: the dev file-watcher can die silently after a couple of hours — a
  src-tauri edit then produces NO "Rebuilding application" line and the running binary stays
  stale; after any src-tauri edit confirm the rebuild line appears, else kill + relaunch.
- **Stale-server FAKE-ONLINE trap (e2e):** an orphaned `e2e_server` on **port 7600** +
  Playwright's `reuseExistingServer: true` makes a `TMP_E2E_ONLINE=1` run silently REUSE the
  old server — if that stale one was OFFLINE (SimDevice), the "online" suite passes GREEN
  without ever touching the device (the converse strands the device seized). ALWAYS
  `lsof -ti tcp:7600 | xargs kill` before an online run, and confirm the log prints
  `ONLINE — seeded snapshot from the real device`. When pre-starting servers to reuse across
  single-spec runs, the device handshake happens ONCE at `e2e_server` startup — verify it
  landed before trusting any spec result. (`scripts/e2e.sh` does both guards for you.)
