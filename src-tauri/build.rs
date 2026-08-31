use std::path::Path;

fn main() {
    // Fresh clone / git worktree: `dist/` is gitignored, so a bare
    // `cargo {test,clippy,build}` reaches `tauri_build::build()` →
    // `generate_context!`, which panics when `frontendDist` (../dist) is
    // absent. Stub an index.html so the Rust checks run without a prior
    // `bun run build`. A real `tauri build` runs `beforeBuildCommand`
    // (bun run build) first, and Vite empties/rewrites dist/, so this stub
    // never reaches a bundle. Mirrors scripts/e2e.sh's ensure_dist().
    let dist = Path::new("../dist");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(dist);
        let _ = std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>stub</title>",
        );
    }

    // Windows: give TEST executables the Common-Controls-v6 manifest tauri-build
    // only embeds into the app binaries — without it the `--features e2e` harness
    // (MockRuntime → comctl32 v6 imports) dies at load with
    // STATUS_ENTRYPOINT_NOT_FOUND. The MSVC linker embeds it directly.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        // `rustc-link-arg-tests` only covers integration-test targets, not the lib's
        // unit-test harness, so the flags go on every target and are then cancelled
        // for the app binaries — link.exe keeps the LAST `/MANIFEST` it sees, and the
        // bins already carry tauri-build's manifest as an embedded resource (a second
        // one would fail the link with LNK1123).
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        // …which makes link.exe warn LNK4075 that the input is ignored: expected.
        println!("cargo:rustc-link-arg-bins=/IGNORE:4075");
    }

    tauri_build::build()
}
