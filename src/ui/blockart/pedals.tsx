// Stompbox pedals (knob-row motifs) — block-art renderer(s) split from the BlockArt
// engine. PedalBody computes the enclosure + LED + footswitch and delegates the
// control-zone MOTIF to `pedalMotif`, whose per-family branches live in
// ./pedalsMotif{Drive,DynEq,Mod,Time,Pitch,Concept} (split to keep each file ≤500
// lines); the full-custom enclosures live in ./pedalsSpecial. Shared data/helpers
// (tones, cloth, chassis, speakers) live in ./shared.
import type { PedalTone } from "./shared";
import { GearPedalBody, LabBoostBody, GruntBoostBody } from "./pedalsSpecial";
import { driveMotif } from "./pedalsMotifDrive";
import { dynEqMotif } from "./pedalsMotifDynEq";
import { modMotif } from "./pedalsMotifMod";
import { timeMotif } from "./pedalsMotifTime";
import { pitchMotif } from "./pedalsMotifPitch";
import { conceptMotif } from "./pedalsMotifConcept";

// The control-zone motif dispatch: try each pedal family in turn. conceptMotif is
// the terminal family (its `default` returns the generic two-knob motif), so the
// chain always resolves to an element.
function pedalMotif(
  g: string,
  c: PedalTone,
  edge: string,
  jewel: string,
  lab: string,
  loopNum: string,
) {
  return (
    driveMotif(g, c, edge, jewel, lab) ??
    dynEqMotif(g, c, edge, jewel) ??
    modMotif(g, c, edge, jewel) ??
    timeMotif(g, c, edge, jewel) ??
    pitchMotif(g, c, edge, loopNum) ??
    conceptMotif(g, c, edge, jewel)
  );
}

export function PedalBody({
  c: cIn,
  g,
  lab,
  footswitch = "round",
  accent,
  panel,
}: {
  c: PedalTone;
  g: string;
  lab: string;
  footswitch?: "plate" | "metal" | "round";
  /** Fender reverb-chassis accent (footswitch colour). When present the pedal
   *  renders the cream chassis and the screen icon is drawn in this colour. */
  accent?: string;
  /** recessed coloured control panel behind the knobs/sliders (MEGA EQ-5, FILTRON). */
  panel?: string;
}) {
  // On the Fender reverb chassis the body is cream and the reverb-type icon reads in
  // the accent colour, so draw the motif with knob=accent.
  const c = accent ? { ...cIn, knob: accent } : cIn;
  // FX-loop number (1/2/3/4/3+4) — the ONLY text an fxloop block carries.
  const loopNum = lab.replace(/^.*?FX\s*-?\s*/i, "").trim();
  const edge = "rgba(0,0,0,0.4)";
  const jewel = "#f5a986";
  // Glooper (EHX-style glitch looper) wears a holographic/iridescent finish — a
  // wash of shifting greens/blues/purples over its dark body.
  const isGloop = /GLOOP/i.test(lab);
  // Footswitch well — a recessed darker rounded rect behind the round footswitch.
  // EVERY round-footswitch pedal gets one (for consistency); it is SOLID BLACK on
  // the few whose ref has a black footswitch panel (Big Muff, Electric Mistress,
  // Small Stone, Memory Man / Memory Man Stereo), else a translucent recess. The
  // treadle (plate) and TS808 (metal) footswitches and the round-form (roundfuzz)
  // fuzz faces never reach the round branch, so they get no well.
  // `lab` is the normalized short (normalizeShort turns "BMP-NYC" into "BMP NYC"),
  // so match the de-hyphenated forms. "BMP NYC" is Big Fuzz only — the other Big
  // Muff variants (BMP RUSS / BMP RH) are not in the black-well list.
  const wellFill = /BMP NYC|MISTRESS|SMSTONE|DMM/i.test(lab)
    ? "#1a1a1c"
    : "rgba(0,0,0,0.22)";
  // 1.8 gear pedals + boosts → full-custom enclosures (own LED + footswitch).
  if (g === "od3") return <GearPedalBody c={c} lab={lab} />;
  if (g === "labboost") return <LabBoostBody c={c} />;
  if (g === "gruntboost") return <GruntBoostBody c={c} />;
  // motif drawn in the top control zone (~y 8..30)
  const motif = pedalMotif(g, c, edge, jewel, lab, loopNum);
  if (accent) {
    // Fender reverb chassis (8 models): cream body, a recessed screen holding the
    // reverb-type icon (the motif, in the accent colour), and a coloured footswitch
    // section at the bottom whose colour identifies the model. Ref-discovered accent.
    return (
      <g>
        <rect
          x="11"
          y="4"
          width="42"
          height="56"
          rx="6"
          fill="#e7e4d9"
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* screen recess framing the reverb-type icon */}
        <rect
          x="14"
          y="8"
          width="36"
          height="26"
          rx="2"
          fill="#d6d3c6"
          stroke="rgba(0,0,0,0.18)"
          strokeWidth="0.4"
        />
        {motif}
        {/* status LED, above the footswitch well */}
        <circle cx="32" cy="37.5" r="1.7" fill={jewel} opacity="0.9" />
        {/* coloured footswitch well — the model-identifying accent, now just the
            recess behind the switch (consistent with every other round footswitch) */}
        <rect x="19" y="43" width="26" height="14" rx="3" fill={accent} />
        {/* round footswitch */}
        <circle
          cx="32"
          cy="50"
          r="5.4"
          fill="#c9ccce"
          stroke="rgba(0,0,0,0.32)"
          strokeWidth="0.6"
        />
        <circle
          cx="32"
          cy="50"
          r="2.7"
          fill="#aeb1b4"
          stroke="rgba(0,0,0,0.25)"
          strokeWidth="0.5"
        />
      </g>
    );
  }
  return (
    <g>
      <rect
        x="11"
        y="4"
        width="42"
        height="56"
        rx="6"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {isGloop && (
        <>
          <defs>
            <clipPath id="gloopclip">
              <rect x="11" y="4" width="42" height="56" rx="6" />
            </clipPath>
          </defs>
          <g clipPath="url(#gloopclip)">
            <ellipse
              cx="20"
              cy="16"
              rx="18"
              ry="16"
              fill="#3fae84"
              opacity="0.34"
            />
            <ellipse
              cx="46"
              cy="30"
              rx="16"
              ry="20"
              fill="#4f8fd0"
              opacity="0.32"
            />
            <ellipse
              cx="22"
              cy="46"
              rx="17"
              ry="16"
              fill="#9a7bc8"
              opacity="0.3"
            />
            <ellipse
              cx="42"
              cy="54"
              rx="15"
              ry="12"
              fill="#3fb0a0"
              opacity="0.28"
            />
          </g>
        </>
      )}
      {panel && (
        // recessed coloured control panel behind the knobs/sliders
        <rect
          x="14"
          y="8"
          width="36"
          height="23"
          rx="2.5"
          fill={panel}
          stroke="rgba(0,0,0,0.25)"
          strokeWidth="0.4"
        />
      )}
      {motif}
      {/* status LED, just above the footswitch */}
      <circle cx="32" cy="37.5" r="1.7" fill={jewel} opacity="0.9" />
      {footswitch === "plate" ? (
        // big black-rubber treadle plate (Boss / metal gate / TS-10)
        <g>
          <rect x="13" y="42" width="38" height="15" rx="3" fill="#1a1a1c" />
          <rect
            x="14.2"
            y="43"
            width="35.6"
            height="1.4"
            rx="0.7"
            fill="rgba(255,255,255,0.10)"
          />
        </g>
      ) : footswitch === "metal" ? (
        // small metallic rectangle switch (Ibanez TS808)
        <g>
          <rect
            x="25"
            y="47"
            width="14"
            height="8"
            rx="1.5"
            fill="#cfd3d7"
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.5"
          />
          <rect
            x="25.7"
            y="47.7"
            width="12.6"
            height="1"
            rx="0.5"
            fill="#eef0f2"
          />
        </g>
      ) : (
        // round chrome footswitch button (default) in its recessed well
        <g>
          <rect x="19" y="43" width="26" height="14" rx="3" fill={wellFill} />
          <circle
            cx="32"
            cy="50"
            r="5.4"
            fill="#c9cdd1"
            stroke="#8a8f94"
            strokeWidth="0.8"
          />
          <circle
            cx="32"
            cy="50"
            r="2.6"
            fill="#aeb3b8"
            stroke="#7a7f84"
            strokeWidth="0.5"
          />
        </g>
      )}
    </g>
  );
}
