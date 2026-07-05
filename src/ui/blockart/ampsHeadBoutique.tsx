// src/ui/blockart/ampsHeadBoutique.tsx — the half-STACK icon + the Ampeg B-15 +
// Mesa Mark IIC amp-HEAD bodies, split from ./amps so each file stays ≤500 lines.
// The Vox/Bassbreaker, Friedman + standard heads live in ./ampsHeadModern.
// Dispatched by AmpBody. Shared data/helpers in ./shared/./parts.
import { clothFor, type PedalTone } from "./shared";
import { GrilleCloth, ChassisBody, Speaker } from "./parts";

export function AmpStack({
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
  const cl = clothFor(t);
  return (
    <g>
      {/* head — control faceplate + knobs on top, grille below */}
      <ChassisBody
        x={9}
        y={6}
        w={46}
        h={17.5}
        rx={2.5}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="sh"
      />
      <rect
        x="13"
        y="8.2"
        width="38"
        height="5.6"
        rx="1"
        fill={c.panel ?? "rgba(0,0,0,0.34)"}
        opacity={c.panel ? 0.95 : 1}
      />
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <circle
          key={i}
          cx={16 + i * 6.5}
          cy={11}
          r="1.4"
          fill={c.knob}
          stroke={edge}
          strokeWidth="0.3"
        />
      ))}
      <GrilleCloth
        x={13}
        y={15.2}
        w={38}
        h={5.6}
        rx={1}
        tone={t}
        uid={uid + "h"}
      />
      {/* 4×12 cab */}
      <ChassisBody
        x={11}
        y={25.5}
        w={42}
        h={33.5}
        rx={2.5}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="sc"
      />
      <GrilleCloth
        x={14}
        y={28}
        w={36}
        h={28.5}
        rx={1.5}
        tone={t}
        uid={uid + "c"}
      />
      {[0, 1].map((ry) =>
        [0, 1].map((rx) => (
          <Speaker
            key={`${String(rx)}-${String(ry)}`}
            x={23 + rx * 18}
            y={35 + ry * 14.5}
            r={6.2}
            cl={cl}
          />
        )),
      )}
    </g>
  );
}

// Ampeg B-15 Portaflex: the exposed amp chassis — a perforated black tube cage
// up top, a silver control plate with a row of knobs + jacks below.
export function AmpHeadB15({ c, edge }: { c: PedalTone; edge: string }) {
  const holes = [];
  for (let ry = 0; ry < 3; ry++)
    for (let cx = 0; cx < 18; cx++) {
      holes.push(
        <circle
          key={`${String(cx)}-${String(ry)}`}
          cx={11.5 + cx * 2.4}
          cy={24 + ry * 2.6}
          r="0.6"
          fill="#000"
          opacity="0.55"
        />,
      );
    }
  return (
    <g>
      <rect
        x="5"
        y="20"
        width="54"
        height="22"
        rx="2"
        fill="#191a1c"
        stroke={edge}
        strokeWidth="0.7"
      />
      {/* perforated tube cage */}
      <rect
        x="9"
        y="22"
        width="46"
        height="9"
        rx="1"
        fill="#0d0e10"
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.4"
      />
      {holes}
      {/* central output transformer (the round can between the tubes — the
         Rampage '66 Flip Top's signature) */}
      <g>
        <circle
          cx="32"
          cy="26.5"
          r="3.4"
          fill="#1a1b1e"
          stroke="#3a3d42"
          strokeWidth="0.5"
        />
        <circle
          cx="32"
          cy="26.5"
          r="2.1"
          fill="none"
          stroke="#52555b"
          strokeWidth="0.5"
        />
        <circle cx="32" cy="26.5" r="0.9" fill="#52555b" />
      </g>
      {/* silver control plate */}
      <rect
        x="9"
        y="32.5"
        width="46"
        height="7"
        rx="1"
        fill={c.panel}
        stroke="rgba(0,0,0,0.3)"
        strokeWidth="0.4"
      />
      {/* input jack (far left) */}
      <circle
        cx="12"
        cy="36"
        r="1.2"
        fill="#16161a"
        stroke="rgba(0,0,0,0.4)"
        strokeWidth="0.35"
      />
      {/* 4 knobs: VOLUME · BASS · MID · TREBLE */}
      {[0, 1, 2, 3].map((i) => (
        <g key={i}>
          <circle
            cx={20 + i * 8.2}
            cy="36"
            r="1.9"
            fill="#1c1c1f"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.4"
          />
          <line
            x1={20 + i * 8.2}
            y1="36"
            x2={20 + i * 8.2}
            y2="34.4"
            stroke="#cfd3d7"
            strokeWidth="0.5"
          />
        </g>
      ))}
      {/* ON/OFF toggle (far right) */}
      <rect
        x="51.5"
        y="34.4"
        width="2.4"
        height="3.2"
        rx="0.4"
        fill="#0e0e10"
      />
    </g>
  );
}

// Mesa Mark IIC head: mirrors the combo's busy control panel — knob row + 5-band
// graphic-EQ block + channel toggles + red LED, over a black jute grille.
export function AmpHeadMesa({
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
  const mknobs = [12, 15.4, 18.8, 22.2, 25.6];
  const eqX = [31.5, 33.5, 35.5, 37.5, 39.5];
  const eqNub = [27, 25.6, 27.8, 26, 26.6];
  const sw = [43, 45.8, 48.6];
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
        k="mh"
      />
      <rect
        x="8"
        y="22.4"
        width="48"
        height="8"
        rx="1.2"
        fill="rgba(13,13,16,0.97)"
        stroke={edge}
        strokeWidth="0.4"
      />
      {mknobs.map((cx, i) => (
        <circle
          key={`k${String(i)}`}
          cx={cx}
          cy="26.4"
          r="1.1"
          fill="#cfd3d7"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      ))}
      <rect
        x="30"
        y="23.4"
        width="11.4"
        height="6"
        rx="0.8"
        fill="#39434f"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.3"
      />
      {eqX.map((x, i) => (
        <g key={`e${String(i)}`}>
          <line
            x1={x}
            y1="24.1"
            x2={x}
            y2="28.8"
            stroke="#1b232c"
            strokeWidth="0.4"
          />
          <rect
            x={x - 0.85}
            y={eqNub[i]}
            width="1.7"
            height="0.65"
            rx="0.25"
            fill="#b3bac2"
          />
        </g>
      ))}
      {sw.map((x, i) => (
        <g key={`s${String(i)}`}>
          <rect
            x={x - 0.7}
            y="25.4"
            width="1.4"
            height="1.3"
            rx="0.35"
            fill="#3a3a3e"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.25"
          />
          <line
            x1={x}
            y1="25.5"
            x2={x}
            y2="23.6"
            stroke="#d2d6da"
            strokeWidth="0.7"
            strokeLinecap="round"
          />
        </g>
      ))}
      <circle
        cx="53.5"
        cy="26.4"
        r="0.85"
        fill="#d24a3a"
        stroke="rgba(60,8,4,0.55)"
        strokeWidth="0.25"
      />
      <GrilleCloth
        x={8}
        y={31.4}
        w={48}
        h={8.6}
        rx={1.4}
        tone="mesa"
        uid={uid + "h"}
      />
    </g>
  );
}
