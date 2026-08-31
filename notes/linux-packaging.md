# Linux packaging

TMP Companion builds a `.deb` and a `.rpm` in the same release run as the macOS DMG, and
publishes them on that release **when their build succeeds** — the Linux jobs are
non-blocking, so a release whose Linux build failed still ships its DMG, just without the
Linux assets (see "Release pipeline shape" below). Both carry the "alpha" qualifier in their filename
(`TMP-Companion-linux-alpha-*`): Linux is a HW-validated _development_ platform (Level and
Doctor re-amp end to end over ALSA/hidraw — see `CONTRIBUTING.md`'s "Developing on Linux"
and `gotchas.md`'s Linux sections), but the packaging itself has far less field exposure
than the macOS path's signing/notarization pipeline.

## Why `.deb` + `.rpm`, no AppImage

An AppImage cannot run a post-install step, so it cannot do either of the two things a
Linux install of this app needs done automatically:

- **Install the udev rule.** `/dev/hidraw*` is root-only by default; without
  `packaging/udev/70-fender-tone-master-pro.rules` landing in
  `/usr/lib/udev/rules.d/` and udev reloading, the app finds no device at all
  (`EACCES` — see `hid.rs`'s `open_device()`).
- **Guarantee `sqlite3` on `PATH`.** `backup_read.rs` shells out to the `sqlite3` CLI for
  the whole library-scan/block-discovery/scene-handle path — an out-of-Cargo runtime
  dependency an AppImage cannot bundle a guarantee for.

A real package solves both as metadata: `bundle.linux.deb/rpm.files` ships the rule,
`postInstallScript` (`packaging/linux/postinst.sh`) reloads udev, and `depends` pulls in
`sqlite3`/`libasound2` (deb) or `sqlite`/`alsa-lib` (rpm). Arch, NixOS, and other
non-deb/rpm distros are expected to build from source (`CONTRIBUTING.md`).

## The udev rule and postinst

`packaging/udev/70-fender-tone-master-pro.rules` tags the TMP's two HID interfaces
`uaccess`, granting the logged-in user access via a systemd-logind ACL — no group, no
replug needed once udev reloads. `packaging/linux/postinst.sh` runs `udevadm
control --reload-rules` + `udevadm trigger --subsystem-match=hidraw` after install, so an
already-connected unit works immediately. Both steps are `|| true`: a container or chroot
install has no running udev, and that must not fail the package install.

The same file is the manual-install path for a source build — see `CONTRIBUTING.md`.

## Why the `.rpm` builds in a Fedora container

GitHub offers no Fedora _runner_, but `container: fedora:<N>` on an `ubuntu-latest` host
gives a genuine Fedora build: the binary links Fedora's libraries and the `depends` names
(`sqlite`, `alsa-lib`, `webkit2gtk4.1`, …) resolve natively there. Building the rpm on
Ubuntu instead would mean hand-writing Fedora dependency names against Ubuntu-linked
libraries — a mismatch with no compiler or linker to catch it.

Tauri's rpm bundler is the pure-Rust `rpm` crate — no `rpmbuild` needed on the build host
(confirmed by building both bundles from this repo on a plain Debian dev box with no
`rpm`/`rpmbuild` installed at all).

## `tauri.linux.conf.json`

Tauri 2 auto-merges a `tauri.<platform>.conf.json` file over the base `tauri.conf.json`
(JSON Merge Patch, RFC 7396) — no `--bundles` flag, no risk to the macOS bundle config.
One caveat that bit the first draft of this file: **merge-patch REPLACES arrays, it does
not extend them** — `bundle.targets` here (`["deb", "rpm"]`) fully supersedes the base's
`["app", "dmg"]` rather than adding to it.

`createUpdaterArtifacts` is explicitly `false` here (the base sets `true`): the Linux
build jobs carry no `TAURI_SIGNING_PRIVATE_KEY`, and a `true` here fails the build outright
for no benefit — see the next section.

## No Linux updater channel (yet)

`scripts/latest-json.mjs` emits only `darwin-aarch64`/`darwin-x86_64` keys. A Linux
install's in-app update check finds nothing at that endpoint and stays silent
(`useUpdater.ts` treats any check failure as silent-fail by design) — Linux users update by
re-downloading the latest `.deb`/`.rpm`. Not worth standing up and signing a second
updater channel for an alpha; revisit once Linux has left alpha.

## Release pipeline shape

```text
resolve-version (ubuntu)              semantic-release --dry-run → next version, or none
   │
   ├─► build-deb (ubuntu)             continue-on-error: true
   └─► build-rpm (ubuntu, fedora container)   continue-on-error: true
              │
              ▼
        release (macos-14)            downloads both bundles, runs the real semantic-release
```

Both Linux build jobs are `continue-on-error`, so a broken Linux bundle never blocks the
signed, notarized macOS DMG — the release still ships, just without that platform's
artifact. That non-blocking design is why a Linux-bundling job also runs in `ci.yml` on
every PR: it is the only thing that would catch a broken bundler config before release
time, since a `continue-on-error` failure at release produces a green run with a silently
missing asset.
