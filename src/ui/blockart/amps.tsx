// Amps (heads / combos / stacks) — block-art renderer(s) split from the BlockArt
// engine. AmpBody does the per-block panel override + form/tone dispatch; the
// bodies live in ./ampsCombo (combos), ./ampsHeadFender + ./ampsHeadBritish +
// ./ampsHeadBoutique + ./ampsHeadModern (heads + the half-stack icon), split to keep each file ≤500
// lines. Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
import type { PedalTone } from "./shared";
import { AmpComboBody } from "./ampsCombo";
import {
  AmpHeadTweed,
  AmpHeadSilverface,
  AmpHeadEvhModern,
} from "./ampsHeadFender";
import {
  AmpHeadOrange,
  AmpHeadGk,
  AmpHeadRecto,
  AmpHeadSvt,
} from "./ampsHeadBritish";
import { AmpStack, AmpHeadB15, AmpHeadMesa } from "./ampsHeadBoutique";
import {
  AmpHeadVoxBbrk,
  AmpHeadFriedman,
  AmpHeadDefault,
} from "./ampsHeadModern";

export function AmpBody({
  c: cIn,
  t,
  g,
  lab,
  uid,
}: {
  c: PedalTone;
  t: string;
  g: string;
  lab: string;
  uid: string;
}) {
  // SLO-100 (Soldano) wears a silver/chrome control faceplate — the only boutique
  // head with one (the EVH 5150 III / Diezel / Bogner boutique heads keep dark
  // panels), so override the panel per-block rather than on the shared tone.
  const c = lab.includes("SLO100") ? { ...cIn, panel: "#c6cace" } : cIn;
  const edge = "rgba(0,0,0,0.4)";
  if (g === "combo")
    return <AmpComboBody c={c} t={t} lab={lab} uid={uid} edge={edge} />;
  if (g === "stack") return <AmpStack c={c} t={t} edge={edge} uid={uid} />;
  // ====== PLAIN AMP HEAD ======================================================
  // Heads are drawn as a WIDE / SHORT box (chassis x5..59 w54 · y20..42 h22) —
  // real amp-head proportions, the same width as a CabBody (x5..59 w54). The
  // half-stack icon simply stacks this head over a cab at one shared scale, so
  // editing a head here updates the Amp Heads view AND the half stack together.
  if (t === "tweed" || /^BJR/i.test(lab))
    return <AmpHeadTweed c={c} t={t} edge={edge} uid={uid} />;
  if (t === "silverface")
    return <AmpHeadSilverface c={c} t={t} edge={edge} uid={uid} />;
  if (t === "evhmodern")
    return <AmpHeadEvhModern c={c} t={t} edge={edge} uid={uid} lab={lab} />;
  if (t === "orange")
    return <AmpHeadOrange c={c} t={t} edge={edge} uid={uid} />;
  if (t === "gk") return <AmpHeadGk c={c} t={t} edge={edge} uid={uid} />;
  if (t === "recto") return <AmpHeadRecto c={c} t={t} edge={edge} uid={uid} />;
  if (t === "svt") return <AmpHeadSvt c={c} t={t} edge={edge} uid={uid} />;
  if (t === "b15") return <AmpHeadB15 c={c} edge={edge} />;
  if (t === "mesa") return <AmpHeadMesa c={c} t={t} edge={edge} uid={uid} />;
  if (t === "vox" || /^BBRK/i.test(lab))
    return <AmpHeadVoxBbrk c={c} t={t} edge={edge} uid={uid} lab={lab} />;
  if (t === "friedman")
    return <AmpHeadFriedman c={c} t={t} edge={edge} uid={uid} />;
  return <AmpHeadDefault c={c} t={t} edge={edge} uid={uid} lab={lab} />;
}
