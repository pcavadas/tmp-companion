#!/usr/bin/env python3
"""Re-apply `strip_name` to src/models/tmp-model-guide.json after a catalog regen.

WHY THIS EXISTS
---------------
`strip_name` is the footswitch SCRIBBLE-STRIP name — what the unit prints under a
switch when its `customLabel` is empty. It is `info.displayNames.name8` in the
firmware's per-block CDD records, the sibling of `name32`, which the catalog already
carries as `pro_control_name`. The original catalog extraction took `name32` and
dropped `name8`; rows then displayed a de-camel-cased FenderId ("Blues Driver")
where the hardware says "Sapphire OD".

The catalog is GENERATED (see `.claude/rules/models-catalog.md`), and its generator
(`expand_catalog.py`) lives outside this repo — so a regen would silently drop
`strip_name` again. Run this after any regen to put it back, then re-run
`bun run test` (the strip-name gate in `src/__tests__/leveling-order.test.ts` is what
catches the drop).

USAGE
-----
    python3 scripts/extract_strip_names.py <path-to-firmware-CDD-JSONs-dir>

e.g. .../firmware/1.8.45/rootfs/home/root/tm-stomp-cdd/JSONs

The firmware tree is NOT vendored (it is not ours to redistribute); only the extracted
names land in the repo, exactly as `block_name` / `pro_control_name` already do.

SAFETY
------
A row is skipped — left with NO strip name rather than a guessed one — when the block
has no CDD record, or when the guide's `pro_control_name` disagrees with the record's
`name32`. A disagreement means the two extractions do not agree on what that block_id
IS (the guide's own field_notes flag the EVH 5150III 6L6 channels as having engine
records but no display-name records), and a wrong name on a footswitch row is worse
than none: consumers fall back to `block_name`.
"""

import glob
import json
import os
import sys

GUIDE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "src",
    "models",
    "tmp-model-guide.json",
)

FIELD_NOTE = (
    "Footswitch scribble-strip short name (name8) from the SAME firmware "
    "fenderId->displayNames table pro_control_name (name32) comes from. This is what "
    "the unit prints under a switch when the footswitch carries no customLabel - e.g. "
    "ACD_BluesDriver is 'Sapphire Drive' (name32) but 'Sapphire OD' on the strip, and "
    "ACD_ObsessiveDrive is 'Comic Sans Drive' but 'CSD'. Differs from name32 for 188 "
    "blocks. Absent where the block has no display-name record (the ACD_FxLoop* "
    "pseudo-blocks) or where pro_control_name and name32 disagree, which means the two "
    "extractions do not agree on what that block_id is (the EVH 5150III 6L6 channels) - "
    "a wrong strip name is worse than none, and consumers fall back to block_name. "
    "REGENERATION: this field is NOT produced by expand_catalog.py; re-apply it with "
    "scripts/extract_strip_names.py after any catalog regen."
)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    cdd_dir = sys.argv[1]
    if not os.path.isdir(cdd_dir):
        print(f"not a directory: {cdd_dir}")
        return 2

    names: dict[str, tuple[str | None, str]] = {}
    for f in sorted(glob.glob(os.path.join(cdd_dir, "CDD_*.json"))):
        try:
            d = json.load(open(f, encoding="utf-8"))
        except (OSError, ValueError) as e:
            # A malformed CDD file must be visible, not silently skipped.
            print(f"  SKIP {os.path.basename(f)}: {e}")
            continue
        fid = d.get("fenderId")
        dn = (d.get("info") or {}).get("displayNames") or {}
        if fid and dn.get("name8"):
            names[fid] = (dn.get("name32"), dn["name8"])
    print(f"CDD records carrying name8: {len(names)}")
    if not names:
        print("no display-name records found - is this the CDD JSONs directory?")
        return 1

    guide = json.loads(open(GUIDE, encoding="utf-8").read())
    added, skipped_unknown, skipped_mismatch = 0, set(), set()
    for row in guide["blocks"]:
        bid = row.get("block_id")
        entry = names.get(bid) if bid else None
        if entry is None:
            if bid:
                skipped_unknown.add(bid)
            continue
        name32, name8 = entry
        pcn = row.get("pro_control_name")
        if pcn and name32 and pcn != name32:
            skipped_mismatch.add(bid)
            continue
        rebuilt = {}
        for k, v in row.items():
            rebuilt[k] = v
            if k == "pro_control_name":
                rebuilt["strip_name"] = name8
        rebuilt.setdefault("strip_name", name8)
        row.clear()
        row.update(rebuilt)
        added += 1

    guide["field_notes"]["strip_name"] = FIELD_NOTE
    open(GUIDE, "w", encoding="utf-8").write(
        json.dumps(guide, indent=2, ensure_ascii=False)
    )
    print(f"rows given a strip_name: {added}")
    print(f"no CDD record:           {sorted(skipped_unknown)}")
    print(f"identity in doubt:       {sorted(skipped_mismatch)}")
    print(f"written: {GUIDE}")
    print("now run: bunx prettier --write src/models/tmp-model-guide.json && bun run test")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
