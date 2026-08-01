---
paths:
  - "**/*.sh"
---

# Shell-script rules

**Scripts must survive bash 3.2.** macOS system `/bin/bash` is 3.2.57. Under `set -euo pipefail` an empty-array expansion `"${arr[@]}"` **ABORTS** with "unbound variable" — guard with `"${arr[@]:-}"`, a length check, or the positional params `"$@"` (always empty-safe). No `mapfile`, no associative arrays, no `${var^^}`.

**TEST under `/bin/bash` explicitly**, not the dev's newer PATH bash, which masks the failure. This failure hit `scripts/e2e.sh` twice: the default no-arg run aborted, and `online all` silently ran a single spec.
