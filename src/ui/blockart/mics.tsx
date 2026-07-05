// Microphones — block-art renderer(s) split from the BlockArt engine.
// MicBody dispatches on the mic icon id to a per-mic body; the bodies live in
// ./micBodiesCondenser + ./micBodiesDynamic (split to keep each file ≤500 lines).
// Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
//
// The 7 cab-mic models, each drawn HORIZONTAL (capsule to the left, as a mic sits
// on a stand in front of a speaker) in its own silhouette:
//   mic_c414  — AKG C414: squared LDC head, round grille, blue LED
//   mic_421   — Sennheiser MD421: tapered "ice-cream-cone", ribbed basket
//   mic_re20  — EV RE20: long fat cylinder, mesh windscreen, body bands
//   mic_sm7b  — Shure SM7B: round foam windscreen + flat body on a yoke
//   mic_ribbon— Royer R-121: slim chrome cylinder, ribbed band, green dot
//   mic_sm57  — Shure SM57: tapered grille cap + dark barrel
//   mic_pencil— Earthworks M23: slim tapered pencil condenser
import type { PedalTone } from "./shared";
import {
  MicC414Body,
  Mic421Body,
  MicRibbonBody,
  MicPencilBody,
} from "./micBodiesCondenser";
import { MicRe20Body, MicSm7bBody, MicSm57Body } from "./micBodiesDynamic";

// ============================================================================
export function MicBody({
  c: _c,
  g,
  lab: _lab,
  uid,
}: {
  c: PedalTone;
  g: string;
  lab: string;
  uid: string;
}) {
  if (g === "mic_c414") return <MicC414Body uid={uid} />;
  if (g === "mic_421") return <Mic421Body uid={uid} />;
  if (g === "mic_re20") return <MicRe20Body uid={uid} />;
  if (g === "mic_sm7b") return <MicSm7bBody uid={uid} />;
  if (g === "mic_ribbon") return <MicRibbonBody uid={uid} />;
  if (g === "mic_sm57") return <MicSm57Body uid={uid} />;
  return <MicPencilBody />;
}
