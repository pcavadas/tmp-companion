// src/ui/blockart/ampsCombo.tsx — the amp COMBO bodies (silverface '68 Custom, EVH
// 5150 III, Bassbreaker, Mesa Mark IIC, Roland JC, tweed/Blues Jr clean-front, and
// the Fender/British default combo), split from ./amps so each file stays ≤500
// lines. Dispatched by AmpBody's `g === "combo"` case. Shared helpers in ./shared.
import { comboLivery, evhAccentColor, type PedalTone } from "./shared";
import {
  GrilleCloth,
  ChassisBody,
  AluGradient,
  SilverfacePanel,
  EvhAccent,
} from "./parts";
import { AmpComboMesa, AmpComboRoland } from "./ampsComboMesaRoland";

export function AmpComboBody({
  c,
  t,
  lab,
  uid,
  edge,
}: {
  c: PedalTone;
  t: string;
  lab: string;
  uid: string;
  edge: string;
}) {
  // '68 Custom silverface combo (ref: 68-custom-deluxe-reverb-combo.svg):
  // black tolex, brushed-alu control panel (skirted Fender knobs + blue
  // silkscreen + red pilot), and a polished-alu TRIM BEZEL framing the
  // silver-turquoise sparkle grille.
  if (t === "silverface") {
    const gid = "sfa" + uid;
    return (
      <g>
        <defs>
          <AluGradient id={gid} />
        </defs>
        {/* black tolex cabinet */}
        <rect
          x="7"
          y="7"
          width="50"
          height="50"
          rx="3"
          fill={c.body}
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* brushed-alu control panel */}
        <SilverfacePanel x={9} y={9} w={46} gid={gid} nKnobs={6} />
        {/* polished-alu trim bezel */}
        <rect
          x="9"
          y="17.6"
          width="46"
          height="37.4"
          rx="2"
          fill={`url(#${gid})`}
          stroke="rgba(0,0,0,0.35)"
          strokeWidth="0.4"
        />
        <rect
          x="9.4"
          y="18"
          width="45.2"
          height="0.7"
          rx="0.35"
          fill="#dfe3e6"
          opacity="0.6"
        />
        {/* sparkle grille inset (base + sf-spark pattern via GrilleCloth) */}
        <GrilleCloth
          x={10.9}
          y={19.5}
          w={42.2}
          h={33.6}
          rx={1}
          tone={t}
          uid={uid + "a"}
        />
      </g>
    );
  }
  // EVH 5150 III combo (evhmodern): near-black boxy cabinet, EVH dark grille,
  // a white chicken-head knob strip, and a channel jewel coloured by the voicing
  // (green/blue/red). A
  // '65 Twin-15-style combo (lab contains 15 / TWIN15) gets ONE big 15"
  // speaker hint behind the cloth instead of the default 1×12.
  if (t === "evhmodern") {
    const acc = evhAccentColor(lab);
    const big15 = /(^|[^0-9])15([^0-9]|$)|TWIN ?15/i.test(lab || "");
    const spkR = big15 ? 15 : 12;
    return (
      <g>
        {/* near-black cabinet */}
        <rect
          x="7"
          y="7"
          width="50"
          height="50"
          rx="3"
          fill={c.body}
          stroke={edge}
          strokeWidth="0.7"
        />
        <rect
          x="7.4"
          y="7.4"
          width="49.2"
          height="0.8"
          rx="0.4"
          fill="#34343a"
          opacity="0.6"
        />
        {/* EVH dark grille (base + evh-grille pattern via GrilleCloth) */}
        <GrilleCloth
          x={9}
          y={18.5}
          w={46}
          h={34.5}
          rx={1.5}
          tone={t}
          uid={uid + "a"}
        />
        {/* single speaker hint behind the cloth */}
        <g opacity="0.38">
          <circle
            cx="32"
            cy="36"
            r={spkR}
            fill="none"
            stroke="#3a3833"
            strokeWidth="0.75"
          />
          <circle
            cx="32"
            cy="36"
            r={spkR * 0.62}
            fill="none"
            stroke="#3a3833"
            strokeWidth="0.5"
          />
          <circle cx="32" cy="36" r={spkR * 0.26} fill="#3a3833" />
        </g>
        {/* EVH mark centered on the grille */}
        <EvhAccent cx={32} cy={36} s={7} stroke="#eef0f2" />
        {/* white chicken-head knob strip on the upper black face */}
        <rect x="11" y="9.4" width="42" height="6.6" rx="1.1" fill="#1c1c1e" />
        {[0, 1, 2, 3, 4].map((i) => (
          <g key={i} transform={`translate(${String(15 + i * 6.6)},12.7)`}>
            <circle
              r="2"
              fill="#eef0f2"
              stroke="rgba(0,0,0,0.4)"
              strokeWidth="0.4"
            />
            {/* chicken-head beak pointing ~12 o'clock */}
            <path d="M0 0 L0 -2.6 L0.95 -1.4 Z" fill="#1a1a1c" />
          </g>
        ))}
        {/* channel jewel (voicing accent) */}
        <circle
          cx="49.4"
          cy="12.7"
          r="1.25"
          fill={acc}
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.3"
        />
      </g>
    );
  }
  const L = comboLivery(t);
  const isTweed = t === "tweed";
  // Bassbreaker combo: charcoal grey-tweed tolex, a black grille, and a single
  // brushed-aluminium trim strip across the top of the grille. Controls live on
  // top, so the front carries no knobs (and no Fender badge — kept simple).
  if (/^BBRK/i.test(lab || "")) {
    return (
      <g>
        {/* charcoal grey tolex cabinet (near-solid — kept simple) */}
        <rect
          x="7"
          y="7"
          width="50"
          height="50"
          rx="3"
          fill="#2b2d31"
          stroke={edge}
          strokeWidth="0.7"
        />
        {/* black grille inset */}
        <GrilleCloth
          x={11}
          y={18}
          w={42}
          h={34}
          rx={1.6}
          tone="black"
          uid={uid + "a"}
        />
        {/* brushed-aluminium trim strip straddling the top of the grille */}
        <g>
          <rect
            x="10"
            y="15.2"
            width="44"
            height="2.9"
            rx="0.6"
            fill="#c2c6ca"
            stroke="rgba(0,0,0,0.4)"
            strokeWidth="0.3"
          />
          <rect
            x="10"
            y="15.4"
            width="44"
            height="0.7"
            rx="0.35"
            fill="#e6e9ec"
            opacity="0.7"
          />
          <rect
            x="10"
            y="17.3"
            width="44"
            height="0.55"
            rx="0.3"
            fill="rgba(0,0,0,0.3)"
          />
        </g>
      </g>
    );
  }
  if (t === "mesa") return <AmpComboMesa uid={uid} edge={edge} />;
  if (t === "roland") return <AmpComboRoland uid={uid} edge={edge} />;
  // The Blues Junior (regular, blackface) shares the LTD's clean-front cabinet —
  // controls live on a top panel, so the front is just a framed grille. Render
  // it with the tweed layout but in black-panel colours (black tolex + silver
  // cloth, both driven by its blackface tone).
  const cleanFront = isTweed || /^BJR$/i.test(lab || "");
  if (cleanFront) {
    // Tweed combo (57 Deluxe, Blues Junior LTD, '59 Bassman …): lacquered
    // mustard-twill cabinet with a large oxblood grille inset behind a tweed
    // frame. Controls live on a rear/top chassis, so the FRONT face is clean
    // — no handle, badge or corner bolts. Frame margins vary by cabinet:
    //   • default (57 Deluxe, BJR LTD): tall — thick top/bottom, thin sides.
    //   • '59 Bassman (4×10): thin top/bottom to match the sides.
    //   • Bassman TV (1×15): thick sides to match top/bottom (square-ish).
    const L_ = lab || "";
    // chassis is 7..57; the grille sits inside a tweed frame. Margins (mx side,
    // mt top, mb bottom) vary by cabinet aspect:
    //   • default (57 Deluxe, BJR LTD): THIN left/right, THICK top/bottom.
    //   • '59 Bassman (4×10): thin & uniform all round (wide cabinet).
    //   • Bassman TV (1×15): top/bottom = left/right — a uniform square frame.
    let mx = 4,
      mt = 6,
      mb = 6;
    if (/BASSTV|BASSMAN TV/i.test(L_)) {
      mx = 8;
      mt = 8;
      mb = 8;
    } else if (/59B|59 BASSMAN/i.test(L_)) {
      mx = 4;
      mt = 4;
      mb = 4;
    }
    // Clean front: tweed amps + Hot Rod-series (Blues Jr) carry their controls
    // on a TOP chassis, so the front is just the cabinet + a framed grille — NO
    // faceplate, knobs, handle or badge. Blues Jr = the tweed layout in black
    // tolex (driven by its blackface tone).
    const gTop = 7 + mt,
      gBot = 57 - mb;
    return (
      <g>
        <ChassisBody
          x={7}
          y={7}
          w={50}
          h={50}
          rx={3}
          c={c}
          t={t}
          edge={edge}
          uid={uid}
          k="b"
        />
        <GrilleCloth
          x={7 + mx}
          y={gTop}
          w={50 - 2 * mx}
          h={gBot - gTop}
          rx={1.6}
          tone={t}
          uid={uid + "a"}
        />
      </g>
    );
  }
  const gy = L.panel ? 16.6 : 22,
    gh = L.panel ? 36.4 : 31;
  const isBrown = t === "brownface";
  const nKnobs = isBrown ? 4 : 5; // brownface '62 Princeton carries 4 controls
  return (
    <g>
      <ChassisBody
        x={7}
        y={7}
        w={50}
        h={50}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="b"
      />
      {L.panel ? (
        /* '65 blackface control panel: NORMAL + VIBRATO channel clusters with a
           divider, red pilot jewel on the right — matches the Deluxe Reverb. */
        <g>
          <rect
            x="9"
            y="9"
            width="46"
            height="7.6"
            rx="1.2"
            fill={L.panelFill}
            stroke={edge}
            strokeWidth="0.4"
          />
          {/* input jacks on the left */}
          {[0, 1].map((i) => (
            <circle
              key={`j${String(i)}`}
              cx={11.8 + i * 2.6}
              cy="12.8"
              r="0.95"
              fill="rgba(0,0,0,0.55)"
              stroke={L.pknob}
              strokeWidth="0.35"
            />
          ))}
          {/* single continuous row of control knobs */}
          {Array.from({ length: nKnobs }, (_, i) => (
            <circle
              key={`k${String(i)}`}
              cx={19 + i * 4}
              cy="12.8"
              r="1.3"
              fill={L.pknob}
              stroke="rgba(0,0,0,0.4)"
              strokeWidth="0.3"
            />
          ))}
          {/* right indicator: brownface round badge / blackface pilot jewel */}
          {!L.noJewel &&
            (isBrown ? (
              <g>
                <circle
                  cx="51.6"
                  cy="12.8"
                  r="1.45"
                  fill="#c9ccd0"
                  stroke="rgba(0,0,0,0.4)"
                  strokeWidth="0.3"
                />
                <circle cx="51.6" cy="12.8" r="0.85" fill="#c0392b" />
              </g>
            ) : (
              <circle
                cx="51.6"
                cy="12.8"
                r="1.25"
                fill="#d24a3a"
                stroke="rgba(60,8,4,0.6)"
                strokeWidth="0.3"
              />
            ))}
        </g>
      ) : (
        /* British / other combos: clean front — a simple neutral badge on the
           top tolex (no brand wave/script letterforms). */
        <g>
          <g opacity="0.95">
            <rect
              x={L.lx}
              y={12}
              width={L.lw}
              height={5.6}
              rx={1.3}
              fill={L.logo}
            />
            <rect
              x={(L.lx ?? 0) + 1.4}
              y={13.6}
              width={(L.lw ?? 0) - 2.8}
              height={1}
              rx={0.4}
              fill={c.body}
              opacity={0.3}
            />
          </g>
          <rect
            x={9}
            y={gy - 1.7}
            width={46}
            height={L.pipeH}
            rx={0.7}
            fill={L.pipe}
            opacity="0.92"
          />
        </g>
      )}
      {/* opaque grille cloth (speakers sit hidden behind the cloth) — the
         blackface/blonde identity reads from the black panel + silver grille;
         the front grille stays clean (no brand mark), matching the refs */}
      <GrilleCloth
        x={9}
        y={gy}
        w={46}
        h={gh}
        rx={1.4}
        tone={t}
        uid={uid + "a"}
      />
    </g>
  );
}
