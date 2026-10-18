---
paths:
  - "docs/**"
---

# Marketing site rules

`docs/` is the GitHub Pages site and nothing else: `index.html`, `assets/` and `.nojekyll`.
Everything under it is published, so developer docs live in `notes/`, never here.
This is a project repo, so the site is served under the `/tmp-companion/` subpath: every asset path must be RELATIVE.
`.github/workflows/pages.yml` publishes `docs/` together with the `codex/linux-package-repos` branch (`scripts/compose-pages.sh`).
Download buttons point at the stable release asset `releases/latest/download/TMP-Companion-macOS.dmg`; release facts are in [`notes/releasing.md`](../../notes/releasing.md).
