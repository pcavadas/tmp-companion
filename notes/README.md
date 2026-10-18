# Notes index

The one index of `notes/`; each note owns its subject, and module headers (`//!`, `//`) stay the authority for code detail.

- [`architecture.md`](architecture.md): read first, to find where something lives — tabs, data paths, platforms, module map.
- [`device/session.md`](device/session.md): read when touching `hid.rs`, `session/` or `monitor.rs`, or debugging a connection — transports, the exclusive seize, the 750 ms session lapse, request batching, reply queues, the monitor.
- [`device/presets.md`](device/presets.md): read before any code that reads, saves, imports or loads a preset — slot addressing, reads, saves, loads, imports, rejected bodies, preset identity, footswitch JSON.
- [`device/structural-edits.md`](device/structural-edits.md): read before any code that replaces, inserts or removes a block — edit ops, confirms, node ids, the insert abort, block caps.
- [`device/scenes.md`](device/scenes.md): read before any scene-context write or measurement — scene context, Scene Change Behavior, where scene writes land, recall behaviour, scene bounds.
- [`device/reamp-audio.md`](device/reamp-audio.md): read before touching re-amp or capture code — the re-amp toggle, engage and capture accuracy, silent captures, stuck re-amp, sample rate, routing, platform audio.
- [`leveling.md`](leveling.md): read when changing the leveling model, the base lane, run order, outcomes or Stop — also the timing re-validation oracles and the re-amp run end.
- [`leveling-scenes.md`](leveling-scenes.md): read when changing the scene or footswitch lanes — amp choice, scene documents, the batch save, correction, footswitch isolation and secant, rebalance, verify-by-ear causes, post-save verification.
- [`leveling-calibration.md`](leveling-calibration.md): read when changing the leveling stimulus or the meter — instrument profiles, playback-level compensation, the capture window, the metering convention.
- [`pickup-topologies.md`](pickup-topologies.md): read before adding an alias or topology — what a stimulus template encodes and how pickups map to one.
- [`doctor.md`](doctor.md): read when changing the Doctor's flow, rules, prescriptions or wording — flow, diagnoses, capture, prescriptions, words on screen, scene consistency, validation arms.
- [`doctor-thresholds.md`](doctor-thresholds.md): read when changing the Doctor's metric or a threshold — the metric, threshold derivation, gate evidence, onset alignment, calibrated DI captures, playback-level offsets.
- [`benchmarks.md`](benchmarks.md): read before re-deriving thresholds or claiming an accuracy change — held-out accuracy benchmarks for the Doctor and leveling.
- [`copy.md`](copy.md): read when changing Copy or block-mutation code — the Copy tab's flow, live edits and working-copy read-back.
- [`songs-setlists.md`](songs-setlists.md): read when changing the Songs tab — song and setlist reads, writes, slot shifts and guards.
- [`e2e-fixtures.md`](e2e-fixtures.md): read before changing a fixture or writing an e2e spec — the scenario fixtures' tiers, cab rule, authoring constraints and deviations (the matrix is `e2e/fixtures/COVERAGE.md`).
- [`e2e-recording.md`](e2e-recording.md): read when reading run evidence or arming a trace — online run folders, the wire recorder and the TCT v1 trace format.
- [`releasing.md`](releasing.md): read when touching `release.yml`, packaging, `docs/` or icons — the release pipeline, macOS signing, Windows and Linux packages, the marketing site, the app icon.
