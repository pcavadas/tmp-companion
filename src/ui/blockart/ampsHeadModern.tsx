// src/ui/blockart/ampsHeadModern.tsx — Vox/Bassbreaker, Friedman, and the standard
// Fender/Marshall/boutique amp-HEAD bodies, split from ./ampsHeadBoutique so each
// file stays ≤500 lines. Dispatched by AmpBody. Shared data/helpers in ./shared/./parts.
import { ptrColor, evhAccentColor, type PedalTone } from "./shared";
import { GrilleCloth, ChassisBody, AluGradient } from "./parts";

// Vox AC30 head + Fender Bassbreaker head: NO front control panel — the head is
// just the cabinet grille (controls live on a top chassis). Vox keeps its diamond
// grille; Bassbreaker shows a black grille with the combo's brushed-alu trim strip
// across the top.
export function AmpHeadVoxBbrk({
  c,
  t,
  edge,
  uid,
  lab,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
  lab: string;
}) {
  const bbrk = /^BBRK/i.test(lab);
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="gh"
      />
      {bbrk && (
        <g>
          <rect
            x="8"
            y="23.4"
            width="48"
            height="2.6"
            rx="0.6"
            fill="#c2c6ca"
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.3"
          />
          <rect
            x="8"
            y="23.6"
            width="48"
            height="0.6"
            rx="0.3"
            fill="#e6e9ec"
            opacity="0.7"
          />
        </g>
      )}
      <GrilleCloth
        x={9}
        y={bbrk ? 27.2 : 23.5}
        w={46}
        h={bbrk ? 11.3 : 15}
        rx={2}
        tone={bbrk ? "black" : t}
        uid={uid + "h"}
      />
    </g>
  );
}

// Friedman BE-100: BLACK tolex head with a large brushed-brass control faceplate
// over the lower face — FBE-100 logo plaque top-left, INPUT jack, a row of 6 dark
// knobs, POWER toggle. No front grille (the grille is the cab).
export function AmpHeadFriedman({
  c,
  t,
  edge,
  uid,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
}) {
  return (
    <g>
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="fr"
      />
      {/* gold pinstripes on the upper black tolex */}
      {[23, 24.4].map((py, i) => (
        <rect
          key={i}
          x="7"
          y={py}
          width="50"
          height="0.5"
          rx="0.25"
          fill="#c9a23e"
          opacity="0.7"
        />
      ))}
      {/* brushed-brass control faceplate (lower face) */}
      <rect
        x="7"
        y="27"
        width="50"
        height="12.5"
        rx="1.2"
        fill={c.panel}
        stroke="rgba(0,0,0,0.35)"
        strokeWidth="0.4"
      />
      {/* FBE-100 logo plaque, top-left */}
      <rect
        x="9"
        y="28.2"
        width="13"
        height="3.8"
        rx="0.6"
        fill="#16161a"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.3"
      />
      <rect
        x="11"
        y="29.7"
        width="9"
        height="0.9"
        rx="0.45"
        fill="#c9a23e"
        opacity="0.85"
      />
      {/* input jack (far left) */}
      <circle
        cx="11"
        cy="36"
        r="1.3"
        fill="#16161a"
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.35"
      />
      {/* 6 dark knobs with white marks */}
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <g key={i}>
          <circle
            cx={20 + i * 5.6}
            cy="36"
            r="1.8"
            fill="#1a1a1c"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.4"
          />
          <line
            x1={20 + i * 5.6}
            y1="36"
            x2={20 + i * 5.6}
            y2="34.5"
            stroke="rgba(244,245,247,0.95)"
            strokeWidth="0.5"
            strokeLinecap="round"
          />
        </g>
      ))}
      {/* power toggle (far right) */}
      <rect
        x="52.4"
        y="34.4"
        width="2.6"
        height="3.2"
        rx="0.4"
        fill="#0e0e10"
      />
    </g>
  );
}

// standard head. Two layouts, by lineage:
//  • Fender brown/black/blonde: control panel across the TOP, short grille below
//    (flush against the panel).
//  • Marshall / Hiwatt / modern boutique (Bogner Uber, Diezel "Petrol", EVH
//    Stealth): full-width grille on top, control panel across the BOTTOM (the
//    real heads carry their knobs on a lower fascia — ref: JCM800 / 5150 Stealth).
export function AmpHeadDefault({
  c,
  t,
  edge,
  uid,
  lab,
}: {
  c: PedalTone;
  t: string;
  edge: string;
  uid: string;
  lab: string;
}) {
  const fenderFace = t === "blackface" || t === "brownface" || t === "blonde";
  // Marshall-lineage heads (incl. the Silver Jubilee) carry a full-width grille on
  // top and the control faceplate across the bottom.
  const bottomFace =
    t === "marshall" || t === "hiwatt" || t === "boutique" || t === "jubilee";
  const faceY = bottomFace ? 32 : 23;
  const knobY = faceY + 4.3;
  const grilleY = bottomFace ? 22 : fenderFace ? 31.6 : 33.4;
  const grilleH = bottomFace ? 9.4 : fenderFace ? 7.9 : 6.1;
  // Diezel "Petrol" carries a brushed-aluminium control fascia (ref) — give it a
  // metal faceplate + dark knobs for contrast, vs the dark fascia the other
  // bottom-face boutique heads (Bogner Uber) wear.
  const petrolFace = bottomFace && /PETROL|VH4/i.test(lab);
  const gid = "ph" + uid;
  const faceFill =
    c.panel ?? (petrolFace ? `url(#${gid})` : "rgba(0,0,0,0.32)");
  const knobFill = petrolFace ? "#1c1d20" : c.knob;
  return (
    <g>
      {petrolFace && (
        <defs>
          <AluGradient id={gid} />
        </defs>
      )}
      <ChassisBody
        x={5}
        y={20}
        w={54}
        h={22}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="sd"
      />
      <GrilleCloth
        x={8}
        y={grilleY}
        w={48}
        h={grilleH}
        rx={1.4}
        tone={t}
        uid={uid + "h"}
      />
      <rect
        x="8"
        y={faceY}
        width="48"
        height="8.6"
        rx="1.4"
        fill={faceFill}
        opacity={(c.panel ?? petrolFace) ? 0.95 : 1}
      />
      {c.jewel && (
        <circle
          cx="11.5"
          cy={knobY}
          r="1.4"
          fill={
            lab.toUpperCase().includes("5150") ? evhAccentColor(lab) : c.jewel
          }
        />
      )}
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <g key={i}>
          <circle
            cx={17 + i * 6.6}
            cy={knobY}
            r="2"
            fill={knobFill}
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.45"
          />
          <line
            x1={17 + i * 6.6}
            y1={knobY}
            x2={17 + i * 6.6}
            y2={knobY - 1.7}
            stroke={petrolFace ? "rgba(244,245,247,0.95)" : ptrColor(c)}
            strokeWidth="0.6"
            strokeLinecap="round"
          />
        </g>
      ))}
    </g>
  );
}
