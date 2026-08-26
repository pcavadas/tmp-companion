#!/usr/bin/env bash
# Regression spec for scripts/tauri-dev-env.sh.
#
# Bash 3.2-compatible (see .claude/rules/shell-scripts.md) — no arrays, no
# mapfile. Run explicitly under /bin/bash, not the dev's newer PATH bash,
# which would mask a 3.2 syntax break.
#
# Fixture: a temp dir prepended to PATH holds stub `uname` and `tauri`
# binaries, so no real Wayland session or tauri install is needed. The
# `tauri` stub echoes the GDK_BACKEND it saw, plus argc and one `arg=<...>`
# line per argument it was forwarded — never `"$*"`, which joins on IFS and
# so cannot tell quoted `"$@"` forwarding apart from an unquoted `$@` that
# word-splits a multi-word argument.
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
target="$script_dir/tauri-dev-env.sh"

stub_dir="$(mktemp -d)"
trap 'rm -rf "$stub_dir"' EXIT

cat > "$stub_dir/uname" <<'STUB'
#!/bin/sh
printf '%s\n' "$SPEC_UNAME"
STUB
chmod +x "$stub_dir/uname"

cat > "$stub_dir/tauri" <<'STUB'
#!/bin/sh
printf 'GDK_BACKEND=%s\n' "${GDK_BACKEND:-}"
printf 'argc=%s\n' "$#"
for a in "$@"; do
  printf 'arg=%s\n' "$a"
done
STUB
chmod +x "$stub_dir/tauri"

fail=0

# <label> <uname> <XDG_SESSION_TYPE> <GDK_BACKEND in, or "-" for unset> \
#   <expected GDK_BACKEND> <script args...>
run_case() {
  label="$1"
  uname_val="$2"
  session="$3"
  backend_in="$4"
  expected_backend="$5"
  shift 5
  expected_args="$(printf 'argc=%s\n' "$#"; for a in "$@"; do printf 'arg=%s\n' "$a"; done)"

  if [ "$backend_in" = "-" ]; then
    out="$(PATH="$stub_dir:$PATH" SPEC_UNAME="$uname_val" XDG_SESSION_TYPE="$session" \
      env -u GDK_BACKEND /bin/bash "$target" "$@" 2>&1)" || {
      printf 'FAIL %s: script exited non-zero\n' "$label"
      fail=1
      return
    }
  else
    out="$(PATH="$stub_dir:$PATH" SPEC_UNAME="$uname_val" XDG_SESSION_TYPE="$session" \
      GDK_BACKEND="$backend_in" /bin/bash "$target" "$@" 2>&1)" || {
      printf 'FAIL %s: script exited non-zero\n' "$label"
      fail=1
      return
    }
  fi

  actual_backend="$(printf '%s\n' "$out" | sed -n 's/^GDK_BACKEND=//p')"
  actual_args="$(printf '%s\n' "$out" | grep -E '^(argc|arg)=')"

  if [ "$actual_backend" != "$expected_backend" ]; then
    printf 'FAIL %s: GDK_BACKEND=%s, expected %s\n' "$label" "$actual_backend" "$expected_backend"
    fail=1
    return
  fi
  if [ "$actual_args" != "$expected_args" ]; then
    printf 'FAIL %s: args=\n%s\nexpected:\n%s\n' "$label" "$actual_args" "$expected_args"
    fail=1
    return
  fi
  printf 'PASS %s\n' "$label"
}

run_case "dev + Linux + wayland + no GDK_BACKEND -> forces x11" \
  Linux wayland - x11 dev

run_case "dev + Linux + wayland + existing GDK_BACKEND -> preserved" \
  Linux wayland broadway broadway dev

run_case "dev + Darwin + wayland -> no-op (not Linux)" \
  Darwin wayland - "" dev

run_case "dev + Linux + x11 session -> no-op (not Wayland)" \
  Linux x11 - "" dev

run_case "build + Linux + wayland -> no-op (not dev)" \
  Linux wayland - "" build

run_case "dev + Linux + wayland + extra args -> forwarded verbatim" \
  Linux wayland - x11 dev --foo bar

run_case "dev + Linux + wayland + multi-word arg -> word boundary preserved" \
  Linux wayland - x11 dev --label "two words"

if [ "$fail" -ne 0 ]; then
  printf '\ntauri-dev-env.spec.sh: FAILED\n' >&2
  exit 1
fi
printf '\ntauri-dev-env.spec.sh: all cases passed\n'
