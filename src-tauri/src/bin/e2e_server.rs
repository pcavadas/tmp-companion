// Offline-UI-e2e backend. Run with `cargo run --features e2e --bin e2e_server`; the
// Playwright bridge-client POSTs invokes to it (see e2e/). Without the
// feature it is an inert stub so a default `cargo build` still compiles every target.
fn main() {
    #[cfg(feature = "e2e")]
    tmp_companion_lib::run_e2e_server();
    #[cfg(not(feature = "e2e"))]
    eprintln!("e2e_server requires `--features e2e`");
}
