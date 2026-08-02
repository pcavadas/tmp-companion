---
name: drive-native-window
description: Driving the visible tmp-companion native window from a Claude session (cliclick taps, screenshots, scroll, launch/stale-dev caveats). Use ONLY when a task needs the literal native window — Dock icon, window chrome, OS dialogs, real-pixel eyeballing. For everything else (UI journeys, view behavior, regressions) use the headless Playwright harness (`bun run e2e`) first; it is faster, deterministic, and the user-preferred path.
---

# Driving the native window

Every rule below is HW/OS-verified on this app (Tauri 2 WKWebView, macOS). The failure modes are silent — a missed tap looks identical to a successful one — so follow the exact recipes.

## Clicking

- **Click with `cliclick`, not `osascript`:** macOS System Events `click at` is rejected (`error -25208`). Use `cliclick c:x,y` with **logical points** — `screencapture -R x,y,w,h` takes logical coords but the PNG is 2× retina (a 900×680 window → 1800×1360 px), so never derive coords by dividing pixel positions.
- **Click by FRACTION, not raw pixels:** the `screencapture -R` PNG renders at varying scale, so derive coords from the element's _fractional position within the captured region_ (`abs_x = origin_x + fx·w`, `abs_y = origin_y + fy·h`), never from displayed-pixel offsets.
- **Raise frontmost BEFORE clicking:** `cliclick` posts to whatever app has focus, so a tap silently MISSES (lands on the terminal) when the companion window isn't frontmost — run `osascript -e 'tell application "System Events" to tell (first process whose name contains "tmp-companion") to set frontmost to true'` first, then re-query the bounds (raising can move it), then click.
- **Secondary-display caveat:** `cliclick` taps don't land when the window sits on a left/negative-x secondary display — move it to the primary display first.
- **Click cadence:** ~500 ms between clicks is enough for the WKWebView to settle.

## Keyboard and scroll

- **Keyboard input doesn't reach WKWebView:** `cliclick kp:return` does NOT fire a React inline-form's `onKeyDown` submit, and neither `cliclick t:<text>` nor `osascript … keystroke` enters TEXT into a focused field — **form fields can't be filled through these native-window tools** (the Playwright harness fills them fine; here, open + cancel a form to verify it renders). Click the explicit ✓ / submit affordance.
- **No scroll in `cliclick`** — warp the cursor over the pane then post a Quartz line-scroll: `python3 -c 'import Quartz; Quartz.CGEventPost(Quartz.kCGHIDEventTap, Quartz.CGEventCreateScrollWheelEvent(None, Quartz.kCGScrollEventUnitLine, 1, -3))'` (negative = down).

## Reading the screen

- **Locked-screen signature:** a locked Mac makes `screencapture -R` print "could not create image from rect" and the front-window bounds query return "Invalid index. window 1" — that's a locked screen, not a crashed app (`pgrep` it to confirm).

## Launching / stale `tauri dev`

- A `tauri dev` left running from a prior session holds **port 1421** (vite's bind), so a fresh `bun run tauri dev` silently fails to start — kill the stale processes first, scoped by port so sibling worktrees' servers survive (`lsof -ti tcp:1421 | xargs kill`, then the `target/debug/tmp-companion` app), then relaunch.
- Same-session: the dev file-watcher can die silently after a couple of hours — a src-tauri edit then produces NO "Rebuilding application" line and the running binary stays stale. After any src-tauri edit confirm the rebuild line appears, else kill + relaunch.
