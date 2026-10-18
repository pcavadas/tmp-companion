# Releasing

One manually dispatched `release.yml` run (Actions tab → release → Run workflow, on `main`) ships every platform.
There is no `push:` trigger: releases are batched by hand.
semantic-release (`.releaserc.json`) picks the version from the conventional commits since the last tag.
The macOS DMG is the release; the Linux and Windows jobs are `continue-on-error`, so a failed Linux or Windows build ships the release without that platform's asset.
Developer setup per platform (Linux udev, Windows Sound settings, Windows installer builds) lives in `CONTRIBUTING.md`.

## Release pipeline

```text
resolve-version (ubuntu)              semantic-release --dry-run → next version, or none
   │
   ├─► build-deb (ubuntu-22.04)       continue-on-error
   ├─► build-rpm (ubuntu, fedora container)   continue-on-error
   └─► build-nsis (windows)           continue-on-error
              │
              ▼
        release (macos-14)            builds, signs and notarizes the DMG, runs the real semantic-release with the Linux bundles
              │
              ├─► publish-windows (ubuntu)      attaches the .exe when build-nsis produced one
              ▼
        publish-linux-repos (ubuntu)  commits apt/ + rpm/ to codex/linux-package-repos
              │
              ▼
        deploy-linux-repos            calls pages.yml to compose and deploy the site
```

`resolve-version` needs `permissions: contents: write` although it writes nothing.
semantic-release runs `verifyAuth` (a `git push --dry-run … HEAD:main`) before it branches on `--dry-run`, so a read-only `GITHUB_TOKEN` gets a 403 and the run dies with `EGITNOPERMISSION` instead of printing a version.

`ci.yml` also bundles Linux on every PR.
It is the only check that catches a broken bundler config before release time, because a `continue-on-error` failure at release is a green run with a missing asset.

## macOS

- `scripts/release-build.sh` builds `--target universal-apple-darwin`: one fat binary with arm64 and x86_64 slices; the Rust code is arch-agnostic and gates only on `target_os = "macos"`.
- The `macos-14` ARM runner cross-builds the x86_64 slice; the toolchain step installs both `aarch64-apple-darwin` and `x86_64-apple-darwin`, and output paths use the `universal-apple-darwin/` target directory.
- The DMG is code-signed (`release.yml` sets the `APPLE_*` secrets), notarized with `xcrun notarytool submit --wait` and stapled with `xcrun stapler staple` (`scripts/release-build.sh`).
- The DMG publishes under the stable asset name `TMP-Companion-macOS.dmg`, so the site's download buttons point at `/releases/latest/download/TMP-Companion-macOS.dmg`.
- The README must not describe an unsigned, `xattr` or Gatekeeper workaround.

**Updater keys.** The Tauri updater looks up `latest.json` by the running slice's arch: `darwin-aarch64` on Apple Silicon, `darwin-x86_64` on Intel, and there is no `darwin-universal` key.
`scripts/latest-json.mjs` therefore emits both keys pointing at the same universal tarball; without `darwin-x86_64`, Intel installs run but never auto-update.

## Windows

- `build-nsis` runs `bun run tauri build --bundles nsis` on `windows-latest`; `src-tauri/tauri.windows.conf.json` merges over the base config on a Windows host and sets `targets: ["nsis"]` and `createUpdaterArtifacts: false`.
- The job copies the one `*-setup.exe` (an exactly-one guard refuses a stale second file) to `TMP-Companion-windows-alpha-x64-setup.exe` beside a `version.txt`.
- `publish-windows` attaches it after the macOS release lands, and drops it when its `version.txt` disagrees with the release version.
- The installer is unsigned (no Windows code-signing certificate, so SmartScreen warns on first launch) and has no updater feed.

## Linux

The release run builds a `.deb` and a `.rpm` and attaches them as `TMP-Companion-linux-alpha-amd64.deb` and `TMP-Companion-linux-alpha-x86_64.rpm`.
Linux is a hardware-validated development platform (Level and Doctor re-amp end to end over ALSA and hidraw), but the packaging has far less field exposure than the macOS pipeline, hence "alpha".

### Package formats

An AppImage cannot run a post-install step, so it cannot install the udev rule or reload udev.
`/dev/hidraw*` is root-only by default, so without `packaging/udev/70-fender-tone-master-pro.rules` in `/usr/lib/udev/rules.d/` the app finds no device (`EACCES` in `hid.rs`'s `open_device()`).
The `.deb` and `.rpm` declare the rule, the hooks and the runtime libraries in their metadata: `bundle.linux.deb/rpm.files` ships the rule, `postInstallScript` reloads udev and `postRemoveScript` updates device access after removal.
Backup reads use `rusqlite`'s bundled SQLite; the packages still declare `sqlite3` / `sqlite` dependencies, which nothing uses.
Arch, NixOS and other non-deb/rpm distros build from source (`CONTRIBUTING.md`).

### udev rule and install hooks

`packaging/udev/70-fender-tone-master-pro.rules` tags the unit's two HID interfaces `uaccess`, which grants the logged-in user access through a systemd-logind ACL: no group, and no replug once udev reloads.
`packaging/linux/postinst.sh` runs `udevadm control --reload-rules` and `udevadm trigger --subsystem-match=hidraw`, so an already-connected unit works at once.
`packaging/linux/postrm.sh` reloads and re-triggers too, because a reload alone updates only future events and a unit still plugged in would keep the ACL the removed rule gave it.
Every step is `|| true`: a container or chroot install has no running udev, and a non-zero exit would leave the package half-installed or half-removed.
The same rule file is the manual-install path for a source build (`CONTRIBUTING.md`).

### RPM build container

GitHub offers no Fedora runner, but `container: fedora:<N>` on an `ubuntu-latest` host is a genuine Fedora build: the binary links Fedora's libraries and the `depends` names (`sqlite`, `alsa-lib`, `webkit2gtk4.1`, …) resolve natively.
Tauri's rpm bundler is the pure-Rust `rpm` crate, so the host needs no `rpm` or `rpmbuild`.
`build-deb` runs on `ubuntu-22.04`, the oldest image GitHub offers, for the lowest glibc floor.

### `tauri.linux.conf.json`

Tauri 2 merges `tauri.<platform>.conf.json` over `tauri.conf.json` as a JSON Merge Patch (RFC 7396), and a merge patch REPLACES arrays.
So `bundle.targets` (`["deb", "rpm"]`) supersedes the base `["app", "dmg"]`.
`createUpdaterArtifacts` is `false`: the Linux build jobs carry no `TAURI_SIGNING_PRIVATE_KEY`, and `true` would fail the build.

`bundle.icon` is its own list, because the bundler files each PNG under `/usr/share/icons/hicolor/<dir>/apps/` with a directory derived from the file name.
A file named `@2x` lands in a `…@2` directory, which hicolor defines as Scale=2: `icons/128x128@2x.png` (256×256 pixels) would land in `256x256@2`, a slot that expects 512×512 pixels.
The Linux list is `32x32 / 64x64 / 128x128 / 256x256`, each landing in the directory that matches its pixel size; `icons/256x256.png` is the same art as `128x128@2x.png`.
`1024x1024` is not a hicolor directory, so `icons/icon.png` stays out of the list, and the replaced array keeps `icon.icns` and `icon.ico` out of the Linux bundle.

### Install test

`ci.yml`'s `bundle-linux` installs each package, runs `.github/scripts/check-linux-payload.sh` against the installed tree, then removes the package.
The check asserts the binary, the udev rule, a desktop entry carrying `StartupWMClass`, and that every icon sits in a hicolor directory matching its pixel size, with no strays.
It reads each PNG's size from its IHDR rather than trusting the path, so a correctly named file rendered at the wrong size fails.
Set `PAYLOAD_ROOT` to an unpacked package tree to run the check without installing anything.

### Updates

`scripts/latest-json.mjs` emits only `darwin-*` keys, so a Linux install's in-app update check finds nothing and stays silent (`useUpdater.ts` treats any check failure as silent by design).
Linux users update through apt or dnf, or by downloading the latest package.

### Apt and dnf repositories

A release with Linux artifacts publishes signed package repositories at `https://pcavadas.github.io/tmp-companion/apt` and `https://pcavadas.github.io/tmp-companion/rpm`.
Package files and generated metadata are committed to the `codex/linux-package-repos` branch under `apt/` and `rpm/`, never to `main` or the website source.

**Publication.**
`publish-linux-repos` downloads whatever Linux builds succeeded.
An API error, an expired artifact or a failed download of an existing artifact fails publication; an absent artifact is an allowed skip.
With no artifacts the job skips key import, signing, the generated-repository checkout and the push.
With one format, only that format's repository is replaced and the other's published files stay.
Both replacement builds are verified before the generated checkout is updated and pushed as one commit.

`scripts/build-linux-repos.sh` renders `packaging/apt/conf/distributions` into a temporary directory and substitutes the imported key's full fingerprint; the tracked placeholder stays unchanged.
Publication supplies the `sound` section, which the Tauri Debian bundle omits, and its `optional` priority.
The APT database, the pool and dists output, and the RPM metadata are rebuilt from scratch outside the checkout, so each format serves only the current build; older blobs stay in Git history and in older release assets.

**Signing.**
A dedicated RSA-4096 key, separate from the Tauri updater key, is stored as `APT_GPG_PRIVATE_KEY` in the `release` environment, which allows only `main`; the release and publication jobs also guard on `main`.
The passphrase-free key is imported into a temporary GnuPG home that is removed when the job ends.
The public keys ship with the repositories: `apt/pubkey.gpg` is binary for apt's `Signed-By`, and `rpm/RPM-GPG-KEY-tmp-companion` is armored for dnf and yum.
APT signs the Release and InRelease metadata.
RPM signs `repodata/repomd.xml`, whose checksums cover the package indexes and packages; the packages themselves are unsigned, so the repo config sets `gpgcheck=0` and `repo_gpgcheck=1`.

**Install and upgrade.**

```bash
# apt (Debian/Ubuntu)
curl -fsSL https://pcavadas.github.io/tmp-companion/apt/pubkey.gpg \
  | sudo tee /usr/share/keyrings/tmp-companion.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/tmp-companion.gpg] https://pcavadas.github.io/tmp-companion/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/tmp-companion.list
sudo apt update && sudo apt install tmp-companion
# Later: sudo apt update && sudo apt install --only-upgrade tmp-companion

# dnf/yum (Fedora/RHEL-family)
sudo curl -fsSL -o /etc/yum.repos.d/tmp-companion.repo \
  https://pcavadas.github.io/tmp-companion/rpm/tmp-companion.repo
sudo dnf install tmp-companion
# Later: sudo dnf upgrade tmp-companion
```

**Validation.**
CI builds two versions of each Tauri package, and `.github/scripts/test-linux-repos.py` checks them against local repositories signed with temporary RSA keys: signatures, native apt and dnf installs and upgrades, payloads, tamper rejection, repeated publication, pruning without an APT database, missing-format preservation, the no-artifact path, failed replacement builds, the unchanged signing template and Pages composition.
Its Git tests push only to a temporary local bare repository and verify that publication never changes `main`.

## Marketing site

The site is `docs/index.html` plus `docs/assets/` and `docs/.nojekyll`, served by GitHub Pages at `https://pcavadas.github.io/tmp-companion/`.
It is a project repo, so the site lives under the `/tmp-companion/` subpath and every asset path must be relative.
Everything under `docs/` is published, so `docs/` holds only the site; developer docs live in `notes/`.
`.github/workflows/pages.yml` checks out `main`'s `docs/`, overlays the package repositories from `codex/linux-package-repos` with `scripts/compose-pages.sh`, and deploys one Pages artifact.
Website changes trigger it directly; after package publication the release workflow calls it explicitly, because a `GITHUB_TOKEN` push triggers no other workflow.
A missing package branch still deploys the website alone.
Pages must use the **GitHub Actions** deployment source.

## App icon

The mark is a flat terracotta (`#d97757`) macOS-squircle tile with three white, bottom-aligned level bars in a 6:11:8 height rhythm.

- **Masters.** The design handoff's two SVG masters (`icon.svg`, the baked squircle, and `icon-square.svg`, full-bleed) are the source of truth; the bundle is regenerated from them. `src-tauri/icons/source.png` is the archival full-bleed master, and the vector sources are `src-tauri/icons/tmp-icon-{master,foreground,background,fullbleed,dark,monochrome}.svg`.
- **macOS.** `icon.icns` and `dock.png` carry the baked squircle (superellipse n≈5, transparent corners), full-bleed with no inset. A Tauri-bundled `.icns` renders verbatim (the macOS 26 auto-enclosure applies only to Icon Composer `.icon` / `Assets.car`), so the rounding must be in the pixels. `dock.png` is the baked 256 px PNG, because the dev Dock does not round a programmatically set icon (`dock.rs`).
- **Desktop ladder and Windows.** Baked-squircle PNGs LANCZOS-resized; Windows is square full-bleed. `bundle.icon` is `32x32 / 128x128 / 128x128@2x / icon.icns / icon.ico`.
- **iOS.** Full-bleed, fully opaque and mask-free: iOS draws the enclosure, so never re-mask the iOS slots.
- **Android.** The adaptive foreground is the bars inside the safe zone plus a monochrome (themed-icon) layer; `mipmap-anydpi-v26/ic_launcher{,_round}.xml` carry `<monochrome>` with `@mipmap/...` refs and the solid-terracotta `@mipmap/ic_launcher_background`.
- **Gate.** `src/__tests__/icon-assets.test.ts` pins the shipped assets' sha256s; recompute them whenever the bundle changes.
