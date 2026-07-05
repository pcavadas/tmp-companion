// src/ui/blockart/ampsHeadFender.tsx — Fender-lineage amp-HEAD bodies (tweed/Blues
// Jr, '68 Custom silverface, EVH 5150 III), split from ./amps so each file stays
// ≤500 lines. Dispatched by AmpBody. Shared data/helpers live in ./shared/./parts.
import { evhAccentColor, type PedalTone } from "./shared";
import {
  GrilleCloth,
  ChassisBody,
  AluGradient,
  SkirtedKnob,
  EvhAccent,
} from "./parts";

// tweed amps AND the Hot-Rod series (Blues Jr) carry their controls on a rear/top
// chassis, so the head front is just tolex + a big grille filling the face — no
// faceplate, knobs or handle (a Hot Rod head reads like a tweed but in
// black tolex). The grille cloth follows the tone (tweed = oxblood, Blues Jr's
// blackface = silver Fender), matching its own combo.
export function AmpHeadTweed({
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
        k="th"
      />
      <GrilleCloth x={9} y={24} w={46} h={14} rx={2} tone={t} uid={uid + "h"} />
    </g>
  );
}

// '68 Custom silverface head: black tolex head chassis carrying the SAME
// brushed-alu panel as the combo (skirted knobs + blue silkscreen + red
// pilot), with a polished-alu trim bezel over a short sparkle grille below.
export function AmpHeadSilverface({
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
  const gid = "sfh" + uid;
  return (
    <g>
      <defs>
        <AluGradient id={gid} />
      </defs>
      <rect
        x="5"
        y="20"
        width="54"
        height="22"
        rx="3"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* alu panel — same parts as the combo, shifted down onto the head */}
      <rect
        x="8"
        y="22.4"
        width="48"
        height="7"
        rx="1.2"
        fill={`url(#${gid})`}
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.4"
      />
      <rect
        x="8.4"
        y="22.9"
        width="47.2"
        height="0.7"
        rx="0.35"
        fill="#dfe3e6"
        opacity="0.7"
      />
      <rect
        x="10"
        y="23.8"
        width="44"
        height="0.5"
        rx="0.25"
        fill="#2f6c98"
        opacity="0.55"
      />
      {[10.4, 12.6].map((cx, i) => (
        <circle
          key={`j${String(i)}`}
          cx={cx}
          cy="26.1"
          r="0.95"
          fill="rgba(0,0,0,0.55)"
          stroke="#cfd3d7"
          strokeWidth="0.35"
        />
      ))}
      {[17.4, 23.6, 29.8, 36, 42.2, 48.4].map((cx, i) => (
        <SkirtedKnob key={`k${String(i)}`} x={cx} y={26.1} />
      ))}
      <circle
        cx="53.4"
        cy="26.1"
        r="1.25"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.6)"
        strokeWidth="0.3"
      />
      {/* polished-alu trim bezel over the short grille */}
      <rect
        x="8"
        y="30.4"
        width="48"
        height="9.2"
        rx="1.4"
        fill={`url(#${gid})`}
        stroke="rgba(0,0,0,0.35)"
        strokeWidth="0.4"
      />
      <GrilleCloth
        x={9.6}
        y={31.8}
        w={44.8}
        h={6.4}
        rx={1}
        tone={t}
        uid={uid + "h"}
      />
    </g>
  );
}

// EVH 5150 III head (evhmodern, ivory): WHITE control faceplate + silver knob row
// across the top, a full-width black EVH grille below, and the EVH mark centered on
// it (ref: 5150 III 6L6). Channel jewel + red power dot on the right of the panel.
export function AmpHeadEvhModern({
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
  const acc = evhAccentColor(lab);
  return (
    <g>
      {/* ivory cabinet */}
      <rect
        x="5"
        y="20"
        width="54"
        height="22"
        rx="3"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* white control faceplate */}
      <rect
        x="8"
        y="22.4"
        width="48"
        height="6.6"
        rx="1.1"
        fill="#f1efe8"
        stroke="rgba(0,0,0,0.18)"
        strokeWidth="0.3"
      />
      {/* input jack hint (left) */}
      <circle
        cx="11"
        cy="25.7"
        r="0.8"
        fill="rgba(0,0,0,0.5)"
        stroke="#9aa0a6"
        strokeWidth="0.3"
      />
      {/* silver knob row */}
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <g key={i} transform={`translate(${String(17 + i * 6)},25.7)`}>
          <circle
            r="1.9"
            fill="#cfd3d7"
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.4"
          />
          <line
            x1="0"
            y1="0"
            x2="0"
            y2="-1.6"
            stroke="#2a2a2c"
            strokeWidth="0.55"
            strokeLinecap="round"
          />
        </g>
      ))}
      {/* channel jewel + red power dot (right) */}
      <circle
        cx="51.5"
        cy="25.7"
        r="1.1"
        fill={acc}
        stroke="rgba(0,0,0,0.45)"
        strokeWidth="0.3"
      />
      <circle
        cx="54.4"
        cy="25.7"
        r="1.1"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.5)"
        strokeWidth="0.3"
      />
      {/* full-width black EVH grille + centered EVH mark */}
      <GrilleCloth
        x={8}
        y={30.4}
        w={48}
        h={9.2}
        rx={1.4}
        tone={t}
        uid={uid + "h"}
      />
      <EvhAccent cx={32} cy={35} s={6.5} stroke="#eef0f2" />
    </g>
  );
}
