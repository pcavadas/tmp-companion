---
paths:
  - "src-tauri/src/commands/songs.rs"
  - "src-tauri/src/commands/setlists.rs"
  - "src-tauri/src/song_io.rs"
  - "src-tauri/src/setlist_io.rs"
  - "src-tauri/src/session/songs.rs"
  - "src-tauri/src/probe_api/songs.rs"
  - "src-tauri/src/probe_api/setlists.rs"
  - "src/views/songs/**"
---

# Songs and setlists rules

Read [`notes/songs-setlists.md`](../../notes/songs-setlists.md) before changing a song or setlist read, write, slot shift or delete.
The unit is the source of truth: every write is followed by a read-back.
A song row references the preset's stored row, not its slot or `preset_id` ([song links](../../notes/device/presets.md#song-links)); the song and setlist setters' wire shapes are the `tmp-companion-protocol` skill.
