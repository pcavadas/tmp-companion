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
- **Run install and removal hooks.** They reload udev and re-trigger connected HID devices
  when the access rule is added or removed. Backup reads now use bundled SQLite through
  `rusqlite`; that path no longer requires command-line SQLite.

A real package declares these files, hooks and runtime libraries in its metadata:
`bundle.linux.deb/rpm.files` ships the rule, `postInstallScript` reloads udev, and
`postRemoveScript` updates device access after removal. Arch, NixOS, and other
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

## The Linux icon list is its own `bundle.icon`

`tauri.linux.conf.json` overrides `bundle.icon` rather than inheriting the base
list, because the bundler files each PNG into `/usr/share/icons/hicolor/<dir>/apps/`
using a directory derived from the FILE NAME, and the base list is named for macOS.

`icons/128x128@2x.png` is a 256×256 file, and the `@2x` made the bundler file it
under `256x256@2` — which hicolor defines as `Size=256, Scale=2`, i.e. a slot that
expects **512×512 physical pixels**. A 256×256 file there is a size/scale mismatch,
so every HiDPI 256pt lookup got half the pixels it asked for. There was also no
plain `256x256` and no `64x64` at all (`icons/64x64.png` exists on disk and was
simply never listed), leaving `128x128` as the largest usable icon.

The Linux list is therefore spelled with plainly-named files only —
`32x32 / 64x64 / 128x128 / 256x256` — each landing in a directory that matches its
own pixel size. `icons/256x256.png` is the same art as `128x128@2x.png`, named for
hicolor rather than for macOS. Note `1024x1024` is NOT a hicolor directory, so
`icons/icon.png` must stay out of this list even though it is the largest asset.

Remember merge-patch REPLACES arrays: this list fully supersedes the base's, which
is exactly what keeps `icon.icns`/`icon.ico` out of the Linux bundle.

## The `.deb`/`.rpm` are install-tested, not just build-tested

`ci.yml`'s `bundle-linux` installs each package it builds, runs
`.github/scripts/check-linux-payload.sh` against the installed tree, then removes
it again. A `tauri build` only proves the bundler config parses — it cannot catch a
file that is present, correct and filed where nothing looks for it, which is
precisely how the `256x256@2` mismatch above survived. The check asserts the binary,
the udev rule, a desktop entry carrying `StartupWMClass`, and that every shipped
icon sits in a hicolor directory matching its pixel size (with no strays). It
reads that size out of each PNG's IHDR rather than trusting the path — a file
named and placed correctly but rendered at the wrong size is the same defect
wearing a disguise, and a path-only check would pass it.

The removal half is a gate too: `postrm.sh` reloads udev AND re-triggers hidraw after the rule is
gone — a reload alone updates only future events, so a unit still plugged in at uninstall would keep
the `uaccess` ACL the departed rule gave it (the mirror of why `postinst.sh` triggers), and
a non-zero exit there would leave the package half-removed. Set `PAYLOAD_ROOT` to an
unpacked package tree to run the check locally without installing anything.

## No Linux updater channel (yet)

`scripts/latest-json.mjs` emits only `darwin-aarch64`/`darwin-x86_64` keys. A Linux
install's in-app update check finds nothing at that endpoint and stays silent
(`useUpdater.ts` treats any check failure as silent-fail by design) — Linux users update by
installing updates through apt/dnf or re-downloading the latest `.deb`/`.rpm`. Not worth standing up and signing a second
updater channel for an alpha; revisit once Linux has left alpha.

## Apt + dnf/yum repositories

A release with Linux artifacts publishes signed package repositories at
`https://pcavadas.github.io/tmp-companion/apt` and
`https://pcavadas.github.io/tmp-companion/rpm`. Package files and generated metadata
are committed to `codex/linux-package-repos`, under `apt/` and `rpm/`.
They are not committed to protected `main` or mixed into the website's source.

**Publication.** The `publish-linux-repos` job in `release.yml` downloads whatever
Linux builds succeeded. An API error, expired artifact or failed download of an
existing artifact fails publication; an absent artifact is an allowed skip. With no artifacts, the source checkout runs but publication skips key import, signing,
the generated-repository checkout and push. If only one format is available, its repository is replaced and the other
format's published files remain unchanged. Both replacement builds are verified
before the generated checkout is updated and pushed as one commit.

`scripts/build-linux-repos.sh` renders `packaging/apt/conf/distributions` into a
temporary configuration directory and substitutes the imported key's full fingerprint
automatically. The tracked placeholder stays unchanged. Publication supplies the
`sound` section, which the Tauri Debian bundle omits, and its `optional` priority. The APT database and complete
pool/dists output are built from scratch outside the checkout; an absent database
cannot leave old package blobs behind. RPM metadata and packages are also rebuilt
in a fresh directory. Each replaced format serves only the current successful build.
Older package blobs remain in Git history and older release assets remain available.

**Signing.** A dedicated RSA-4096 signing key, separate from the Tauri updater key,
is stored as `APT_GPG_PRIVATE_KEY` in the `release` environment. The environment allows
only `main`, and release/publication jobs also have explicit main-only guards.
The passphrase-free key is imported into a temporary private GnuPG home that is removed
when the job finishes. Its public keys are exported with the repository:
`apt/pubkey.gpg` is binary for apt's `Signed-By`; `rpm/RPM-GPG-KEY-tmp-companion` is
armored for dnf/yum.

APT signs Release and InRelease metadata. RPM signs `repodata/repomd.xml`, whose
checksums cover the package indexes and packages. Individual RPM packages are not
signed, so the configuration keeps `gpgcheck=0` and `repo_gpgcheck=1`.
RSA targets broad client compatibility; native Ubuntu apt and Fedora dnf validation
are the tested client boundaries.

**Pages deployment.** `.github/workflows/pages.yml` checks out the current `main`
website, overlays the generated branch's repositories with `scripts/compose-pages.sh`,
and deploys one Pages artifact. Website changes trigger it directly. After package
publication, the release workflow calls it explicitly: a `GITHUB_TOKEN` push does not
trigger another Actions workflow. A missing generated branch permits website-only
deployment. GitHub Pages must use the **GitHub Actions** deployment source.
The website stays under its existing `/tmp-companion/` project path and retains
relative asset links.

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

**Validation.** CI builds two versions of each real Tauri package. The
`.github/scripts/test-linux-repos.py` gate uses temporary RSA signing keys and
local repositories to check signatures, native apt/dnf installation and upgrades,
payloads, tamper rejection, repeated publication, pruning without an APT database,
missing-format preservation, no-artifact behavior, failed replacement builds,
the unchanged signing template, and Pages composition. Its Git tests push only to a
temporary local bare repository and verify that package publication never changes `main`.

## Release pipeline shape

```text
resolve-version (ubuntu)              semantic-release --dry-run → next version, or none
   │
   ├─► build-deb (ubuntu)             continue-on-error: true
   └─► build-rpm (ubuntu, fedora container)   continue-on-error: true
              │
              ▼
        release (macos-14)            downloads both bundles, runs the real semantic-release
              │
              ▼
        publish-linux-repos (ubuntu)  commits apt/ + rpm/ to codex/linux-package-repos
              │
              ▼
        deploy-linux-repos           calls pages.yml to compose and deploy the site
```

`resolve-version` needs `permissions: contents: write` despite writing nothing:
semantic-release runs `verifyAuth` — a `git push --dry-run … HEAD:main` — before it
branches on `--dry-run`, so a read-only `GITHUB_TOKEN` 403s and the run dies with
`EGITNOPERMISSION` instead of printing a version. That is how this job failed on its very
first run on `main`, and it is not visible from the `--dry-run` flag alone.

Both Linux build jobs are `continue-on-error`, so a broken Linux bundle never blocks the
signed, notarized macOS DMG — the release still ships, just without that platform's
artifact. That non-blocking design is why a Linux-bundling job also runs in `ci.yml` on
every PR: it is the only thing that would catch a broken bundler config before release
time, since a `continue-on-error` failure at release produces a green run with a silently
missing asset.
