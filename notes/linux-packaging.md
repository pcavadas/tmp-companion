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
re-downloading the latest `.deb`/`.rpm`. Not worth standing up and signing a second
updater channel for an alpha; revisit once Linux has left alpha.

## Apt + dnf/yum repositories

Every release that produces a `.deb`/`.rpm` also publishes it into a real,
standards-compliant package repository hosted on this same GitHub Pages site —
`https://pcavadas.github.io/tmp-companion/apt` (reprepro, pool/dists layout) and
`https://pcavadas.github.io/tmp-companion/rpm` (createrepo_c, repodata layout).
Both are rebuilt fresh every release run, not incrementally maintained —
`docs/apt/db/` (reprepro's working Berkeley DB) is gitignored and rebuilt from
scratch each run; `docs/rpm/repodata/` has no separate working state since
`createrepo_c` is stateless.

**Signing.** One dedicated RSA-4096 sign-only GPG key (distinct from the Tauri
updater minisign key and the Apple signing certs), private key held as the
`APT_GPG_PRIVATE_KEY` secret scoped to the `release` GitHub Environment — same
trust boundary as the Apple secrets. Passphrase-less: the key material is
already protected as a scoped Actions secret, and a passphrase only adds CI
complexity (loopback pinentry) for no additional real security. RSA over
Ed25519 specifically because the rpm side only needs broad client
compatibility, not signature size/speed — RSA has zero known verification gaps
across every apt or dnf/yum client that has shipped; Ed25519 support in older
enterprise dnf/GPGME stacks is not universally reliable. The public key is
re-exported from the imported private key on every CI run — `docs/apt/pubkey.gpg`
(dearmored, for apt's `Signed-By=`) and `docs/rpm/RPM-GPG-KEY-tmp-companion`
(armored, the dnf/yum convention) — so it's always in sync automatically,
never hand-maintained.

**Apt repo signs are Release-level, not per-package** — that's `reprepro`'s
normal signing model. **The rpm repo signs `repodata/repomd.xml` only**, not
each individual `.rpm` (`rpm --addsign` is deliberately skipped — see the CI
job comments). `primary.xml.gz` records each rpm's SHA-256, and `repomd.xml`
records a checksum of `primary.xml.gz`, so signing `repomd.xml` transitively
covers every listed package — the same trust chain apt's own
Release→Packages.gz→.deb chain gives. The `.repo` file therefore ships
`gpgcheck=0`, `repo_gpgcheck=1` — `gpgcheck=1` with no embedded per-package
signature would make every `dnf install` fail outright.

**Pruning: only the latest version is kept in the working tree.** Each publish
run does `reprepro remove stable <pkg>` before `includedeb`, and rewrites
`docs/rpm/*.rpm` outright before `createrepo_c`. This keeps the _served_ repo
small; it does **not** shrink git history — old pool/rpm blobs remain in past
commits, an accepted tradeoff of a git-backed static host. Given the project's
alpha posture and the existing "users re-download the latest `.deb`/`.rpm`"
update model (see "No Linux updater channel" above), keeping only the latest
version is the right default: there is no in-place upgrade story that depends
on an old version staying installable from this repo, and a growing served
pool with no expiry policy is a worse failure mode (unbounded Pages storage)
than "reinstall a needed old version from a GitHub Release asset instead" —
Assets already keep every past release's artifact indefinitely.

**Install (end users).**

```bash
# apt (Debian/Ubuntu)
curl -fsSL https://pcavadas.github.io/tmp-companion/apt/pubkey.gpg \
  | sudo tee /usr/share/keyrings/tmp-companion.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/tmp-companion.gpg] https://pcavadas.github.io/tmp-companion/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/tmp-companion.list
sudo apt update && sudo apt install tmp-companion

# dnf/yum (Fedora/RHEL-family)
sudo curl -fsSL -o /etc/yum.repos.d/tmp-companion.repo \
  https://pcavadas.github.io/tmp-companion/rpm/tmp-companion.repo
sudo dnf install tmp-companion
```

**CI job.** `publish-linux-repos` in `release.yml`, gated on
`needs.release.result == 'success'` only — never on `build-deb`/`build-rpm`'s
`result`, which `continue-on-error: true` forces to report `success` even when
the underlying build genuinely failed. Whether there's anything to publish is
determined at runtime from artifact presence (`hashFiles(...)`), the same
tolerant pattern the `release` job already uses for its own Linux-asset
download. Combined into one job rather than split `publish-apt`/`publish-rpm`
so there is exactly one checkout → commit → push to `main` per run — two
independent jobs each pushing in the same run would race each other's
fast-forward.

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
        publish-linux-repos (ubuntu)  publishes docs/apt + docs/rpm to GitHub Pages
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
