// src/models/deviceId.ts — device FenderId → catalog id resolution: the ONE
// suffix-stripping rule every catalog-keyed lookup (block art, CPU cost, form and
// strip-group lookups, amp detection) shares. A zero-import LEAF on purpose: `cpu`,
// `catalog` and `blockArt` all import it, so it can never close a module-init cycle
// and a CPU-cost check never pulls in the block-art tables. Mirrored in the Rust
// `is_amp_model_id` (`src-tauri/src/probe_api/scene_jobs.rs`) — a change to the
// suffix set or the NoFx bridge must edit both.

// Device FenderIds carry cab/IR/convolution suffixes the catalog id omits
// (e.g. ACD_TweedDeluxeCabIR → ACD_TweedDeluxe). Strip them one at a time,
// checking after each. NoFx is part of real base ids so it is NOT stripped.
const SUFFIX = /(ConvRvb|CabIR|NoCab|Cab|IR)$/;

/** Resolve a device FenderId to a catalog id by stripping cab/IR/convolution suffixes
 *  one at a time, CHECKING `inSet` BEFORE each strip — so an id already catalogued WITH
 *  a suffix (the `…CabIRConvRvb` reverb amps) matches directly and is never
 *  over-stripped, while a bare-catalogued amp discovered with an extra suffix
 *  (`ACD_HiwattDR103CanModCabIR` → `ACD_HiwattDR103CanMod`) still matches. The last-gap
 *  bridge appends `NoFx` once: a device "wet" amp id (…BlondeVibratoCabIRConvRvb) strips
 *  to a bare id (…BlondeVibrato) the catalog only carries WITH the NoFx token
 *  (…BlondeVibratoNoFx); NoFx is never stripped, so it must be re-added to match.
 *  Returns the first form satisfying `inSet`, else the fully-stripped id. */
export function resolveDeviceId(
  model: string,
  inSet: (id: string) => boolean,
): string {
  let m = model;
  for (let i = 0; i < 6; i++) {
    if (inSet(m)) return m;
    const next = m.replace(SUFFIX, "");
    if (next === m) break;
    m = next;
  }
  if (inSet(m)) return m;
  if (!m.endsWith("NoFx") && inSet(m + "NoFx")) return m + "NoFx";
  return m;
}
