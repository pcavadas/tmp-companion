# Notes index

The one index of `notes/`; each note owns its subject, and module headers (`//!`, `//`) stay the authority for code detail.

- [`architecture.md`](architecture.md): tabs, data paths, platforms, module map. Read first, when you need to know where something lives.
- [`device/session.md`](device/session.md): transports, the exclusive seize, the 750 ms session lapse, request batching, reply queues, the monitor. Read when touching `hid.rs`, `session/` or `monitor.rs`, or debugging a connection.
- [`device/presets.md`](device/presets.md): slot addressing, reads, saves, loads, imports, rejected bodies, preset identity, footswitch JSON, structural edits. Read before any code that reads, saves, imports or edits a preset.
- [`device/scenes.md`](device/scenes.md): scene context, Scene Change Behavior, where scene writes land, recall behaviour, scene bounds. Read before any scene-context write or measurement.
- [`device/reamp-audio.md`](device/reamp-audio.md): the re-amp toggle, engage and capture accuracy, silent captures, stuck re-amp, sample rate, routing, platform audio. Read before touching re-amp or capture code.
- [`leveling.md`](leveling.md): the Level tab's lanes (base, scene, footswitch, rebalance), outcomes, Stop, instrument profiles, metering, timing re-validation. Read when changing leveling logic or its UI contract.
- [`pickup-topologies.md`](pickup-topologies.md): what a stimulus template encodes and how pickups map to one. Read before adding an alias or topology.
- [`doctor.md`](doctor.md): the Doctor's metric, rules, thresholds, capture, prescriptions and playback-level offsets. Read when changing Doctor analysis or wording.
- [`benchmarks.md`](benchmarks.md): held-out accuracy benchmarks for the Doctor and leveling. Read before re-deriving thresholds or claiming an accuracy change.
- [`copy.md`](copy.md): the Copy tab's flow, live edits and working-copy read-back. Read when changing Copy or block-mutation code.
- [`songs-setlists.md`](songs-setlists.md): song and setlist reads, writes, slot shifts and guards. Read when changing the Songs tab.
- [`e2e-fixtures.md`](e2e-fixtures.md): the scenario fixtures' tiers, cab rule, authoring constraints and deviations (the matrix is `e2e/fixtures/COVERAGE.md`). Read before changing a fixture or writing an e2e spec.
- [`e2e-recording.md`](e2e-recording.md): online run folders, the wire recorder and the TCT v1 trace format. Read when reading run evidence or arming a trace.
- [`releasing.md`](releasing.md): the release pipeline, macOS signing, Windows and Linux packages, the marketing site, the app icon. Read when touching `release.yml`, packaging, `docs/` or icons.
- [`open-questions.md`](open-questions.md): device questions still open, with stable ids. Read when code depends on an unknown, or before a hardware session.
