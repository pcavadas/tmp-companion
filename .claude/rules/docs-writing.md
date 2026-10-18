---
paths:
  - "notes/**"
  - "**/*.md"
---

# Documentation rules

Coding agents are the main readers; write for a reader who greps and lands mid-file.
[`notes/README.md`](../../notes/README.md) is the index: add a line there for a new note.

- **One sentence per line** (semantic line breaks); never hard-wrap inside a sentence.
- **Current state only.**
  No history, incident stories, dates, PR or issue numbers, plan labels, or "used to / no longer / now".
- **One home per fact.**
  Link the owning section instead of restating it; device facts live in `notes/device/`.
- **Evidence tags** go at the end of a device-fact sentence, exactly: `[HW]`, `[RE]`, `[RE, inferred]`, `[RE, likely]` or `[HW+RE]`.
  A device doc says once near its top which firmware its facts are for; a line names another firmware only when it differs.
  A static-model result (analysis of the code or the firmware without the unit) is never tagged `[HW]`.
- **Open questions** live only in [`notes/open-questions.md`](../../notes/open-questions.md); link them as `notes/open-questions.md#<id>`.
- **Headings are short noun phrases** with no tags, firmware versions or claims, because they are link anchors: keep them stable, and when you rename or move one, update every link to it (code comments included).
- **Sections are self-contained**: link the target section, never "see above" or "below".
- **Greppable:** name exact symbols and file paths; give a wire message its name with the field number on first use.
- **Tables only for short cells**, never padded; use a list for anything longer.
- **No private references:** nothing that points outside this public repo or at a local machine (private repos, local paths, session links, decompile addresses).
