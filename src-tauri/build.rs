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
        let _ = std::fs::write(dist.join("index.html"), "<!doctype html><title>stub</title>");
    }

    tauri_build::build()
}
