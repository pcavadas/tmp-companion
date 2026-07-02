// Amps (heads / combos / stacks) — block-art renderer(s) split from the BlockArt engine.
// Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
import {
  type PedalTone,
  clothFor,
  ptrColor,
  comboLivery,
  evhAccentColor,
} from "./shared";
import {
  GrilleCloth,
  ChassisBody,
  Speaker,
  AluGradient,
  SilverfacePanel,
  SkirtedKnob,
  EvhAccent,
} from "./parts";

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
  const cl = clothFor(t);
  if (g === "combo") {
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
          <rect
            x="11"
            y="9.4"
            width="42"
            height="6.6"
            rx="1.1"
            fill="#1c1c1e"
          />
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
    // Marksman (Mesa Mark IIC+) combo: black tolex, a busy black control panel
    // (knob row + 5-band graphic-EQ slider block + toggle switches & red LED),
    // a black jute grille — minimal flat sketch (no bolts/badge).
    if (t === "mesa") {
      const knobs = [14.5, 18, 21.5, 25, 28.5];
      const eqX = [33, 35, 37, 39, 41],
        eqNub = [13.8, 12.3, 14.6, 12.7, 13.4];
      const sw = [45.6, 48.4, 51.2];
      return (
        <g>
          <rect
            x="7"
            y="7"
            width="50"
            height="50"
            rx="3"
            fill="#111114"
            stroke={edge}
            strokeWidth="0.7"
          />
          {/* black jute grille */}
          <GrilleCloth
            x={11}
            y={19}
            w={42}
            h={34}
            rx={1.6}
            tone="mesa"
            uid={uid + "a"}
          />
          {/* control panel */}
          <rect
            x="9"
            y="9"
            width="46"
            height="8.4"
            rx="1.2"
            fill="rgba(13,13,16,0.97)"
            stroke={edge}
            strokeWidth="0.4"
          />
          {/* input jacks (stacked, far left) */}
          {[11.2, 14.2].map((cy, i) => (
            <circle
              key={`j${String(i)}`}
              cx="11"
              cy={cy}
              r="0.7"
              fill="rgba(0,0,0,0.6)"
              stroke="#9aa0a6"
              strokeWidth="0.3"
            />
          ))}
          {/* knob row */}
          {knobs.map((cx, i) => (
            <circle
              key={`k${String(i)}`}
              cx={cx}
              cy="13"
              r="1.15"
              fill="#cfd3d7"
              stroke="rgba(0,0,0,0.45)"
              strokeWidth="0.3"
            />
          ))}
          {/* 5-band graphic EQ block */}
          <rect
            x="31.4"
            y="10.3"
            width="11.4"
            height="6.6"
            rx="0.8"
            fill="#39434f"
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.3"
          />
          {eqX.map((x, i) => (
            <g key={`eq${String(i)}`}>
              <line
                x1={x}
                y1="11.1"
                x2={x}
                y2="16.2"
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
          {/* toggle switches + red LED */}
          {sw.map((x, i) => (
            <g key={`sw${String(i)}`}>
              <rect
                x={x - 0.7}
                y="13.1"
                width="1.4"
                height="1.3"
                rx="0.35"
                fill="#3a3a3e"
                stroke="rgba(0,0,0,0.45)"
                strokeWidth="0.25"
              />
              <line
                x1={x}
                y1="13.2"
                x2={x}
                y2="11"
                stroke="#d2d6da"
                strokeWidth="0.7"
                strokeLinecap="round"
              />
              <circle cx={x} cy="10.9" r="0.5" fill="#e8ebee" />
            </g>
          ))}
          <circle
            cx="53.7"
            cy="12.7"
            r="0.85"
            fill="#d24a3a"
            stroke="rgba(60,8,4,0.55)"
            strokeWidth="0.25"
          />
        </g>
      );
    }
    // JC Clean (Roland JC-120) combo: black cabinet, a wide control panel (input +
    // bright switch, 6 knobs, a khaki chorus sub-panel of 3 knobs, a right section
    // with red LED + rocker), a dark-charcoal grid grille, and a neutral badge.
    if (t === "roland") {
      const k6 = [16, 19, 22, 25, 28, 31];
      const k3 = [35.5, 38.5, 41.5];
      return (
        <g>
          <rect
            x="7"
            y="7"
            width="50"
            height="50"
            rx="3"
            fill="#121316"
            stroke={edge}
            strokeWidth="0.7"
          />
          {/* dark-charcoal grid grille */}
          <GrilleCloth
            x={11}
            y={19.5}
            w={42}
            h={33}
            rx={1.6}
            tone="jc"
            uid={uid + "a"}
          />
          {/* JC badge, top-left of the grille */}
          <g>
            <rect
              x="14"
              y="23"
              width="11"
              height="6.4"
              rx="0.8"
              fill="rgba(13,14,17,0.5)"
              stroke="#9aa0a6"
              strokeWidth="0.45"
            />
            <rect
              x="16.4"
              y="24.4"
              width="2"
              height="3.4"
              rx="0.4"
              fill="#c7cbcf"
              opacity="0.8"
            />
            <rect
              x="19.4"
              y="24.4"
              width="2"
              height="3.4"
              rx="0.4"
              fill="#c7cbcf"
              opacity="0.8"
            />
          </g>
          {/* control panel */}
          <rect
            x="9"
            y="9"
            width="46"
            height="8.4"
            rx="1.2"
            fill="#2b3338"
            stroke={edge}
            strokeWidth="0.4"
          />
          {/* input jack + bright toggle */}
          <circle
            cx="11"
            cy="13"
            r="0.8"
            fill="rgba(0,0,0,0.6)"
            stroke="#9aa0a6"
            strokeWidth="0.3"
          />
          <g>
            <rect
              x="12.9"
              y="13.1"
              width="1.1"
              height="1.2"
              rx="0.3"
              fill="#1d2326"
              stroke="rgba(0,0,0,0.4)"
              strokeWidth="0.25"
            />
            <line
              x1="13.45"
              y1="13.2"
              x2="13.45"
              y2="11.4"
              stroke="#d2d6da"
              strokeWidth="0.6"
              strokeLinecap="round"
            />
          </g>
          {/* 6 main knobs */}
          {k6.map((cx, i) => (
            <circle
              key={`k${String(i)}`}
              cx={cx}
              cy="13"
              r="1.05"
              fill="#cfd3d7"
              stroke="rgba(0,0,0,0.45)"
              strokeWidth="0.3"
            />
          ))}
          {/* khaki chorus sub-panel + 3 knobs */}
          <rect
            x="33.5"
            y="10"
            width="10"
            height="6.8"
            rx="0.7"
            fill="#8a7f4e"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.3"
          />
          {k3.map((cx, i) => (
            <circle
              key={`c${String(i)}`}
              cx={cx}
              cy="13"
              r="0.95"
              fill="#23231f"
              stroke="rgba(0,0,0,0.45)"
              strokeWidth="0.3"
            />
          ))}
          {/* right section: red LED + rocker switch */}
          <rect
            x="44.5"
            y="10"
            width="9.5"
            height="6.8"
            rx="0.7"
            fill="#15191c"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.3"
          />
          <circle
            cx="46.9"
            cy="13"
            r="0.75"
            fill="#d24a3a"
            stroke="rgba(60,8,4,0.55)"
            strokeWidth="0.25"
          />
          <rect
            x="49.6"
            y="11.6"
            width="2.2"
            height="2.8"
            rx="0.4"
            fill="#3a3e42"
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.25"
          />
        </g>
      );
    }
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
  if (g === "stack") {
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
  // ====== PLAIN AMP HEAD ======================================================
  // Heads are drawn as a WIDE / SHORT box (chassis x5..59 w54 · y20..42 h22) —
  // real amp-head proportions, the same width as a CabBody (x5..59 w54). The
  // half-stack icon simply stacks this head over a cab at one shared scale, so
  // editing a head here updates the Amp Heads view AND the half stack together.
  // tweed amps AND the Hot-Rod series (Blues Jr) carry their controls on a rear/top
  // chassis, so the head front is just tolex + a big grille filling the face — no
  // faceplate, knobs or handle (a Hot Rod head reads like a tweed but in
  // black tolex). The grille cloth follows the tone (tweed = oxblood, Blues Jr's
  // blackface = silver Fender), matching its own combo.
  if (t === "tweed" || /^BJR/i.test(lab)) {
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
        <GrilleCloth
          x={9}
          y={24}
          w={46}
          h={14}
          rx={2}
          tone={t}
          uid={uid + "h"}
        />
      </g>
    );
  }
  // '68 Custom silverface head: black tolex head chassis carrying the SAME
  // brushed-alu panel as the combo (skirted knobs + blue silkscreen + red
  // pilot), with a polished-alu trim bezel over a short sparkle grille below.
  if (t === "silverface") {
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
  if (t === "evhmodern") {
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
  // Orange heads: cream picture-frame border, white control panel with the brand
  // bar + red square up top, and a lower orange control band with black knobs.
  // The cream frame is a FILLED rect with the white panel inset by an EQUAL
  // margin on all four sides, so the visible cream border is uniform width all
  // around (a stroked frame + mismatched inner panel reads as uneven borders).
  if (t === "orange") {
    const frame = "#efe9da";
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
          k="oh"
        />
        {/* filled cream picture-frame */}
        <rect
          x="7.5"
          y="22"
          width="49"
          height="18"
          rx="1.8"
          fill={frame}
          stroke="rgba(0,0,0,0.18)"
          strokeWidth="0.4"
        />
        {/* white panel, inset 2.2 on every side → uniform cream border */}
        <rect
          x="9.7"
          y="24.2"
          width="44.6"
          height="13.6"
          rx="0.9"
          fill="#f9f7f1"
        />
        <rect
          x="12"
          y="25.4"
          width="18.5"
          height="3.3"
          rx="0.6"
          fill="#16120c"
        />
        <rect
          x="49.4"
          y="25.2"
          width="3.8"
          height="3.8"
          rx="0.6"
          fill="#c33b1e"
          opacity="0.85"
        />
        <rect
          x="12"
          y="30.6"
          width="40"
          height="6.1"
          rx="0.9"
          fill={c.body}
          stroke="rgba(0,0,0,0.25)"
          strokeWidth="0.4"
        />
        {[0, 1, 2, 3, 4, 5].map((i) => (
          <g key={i}>
            <circle
              cx={15.6 + i * 6.3}
              cy={33.6}
              r="1.9"
              fill="#15110f"
              stroke="rgba(0,0,0,0.4)"
              strokeWidth="0.4"
            />
            <line
              x1={15.6 + i * 6.3}
              y1={33.6}
              x2={15.6 + i * 6.3}
              y2={32}
              stroke="#e8e8e8"
              strokeWidth="0.55"
              strokeLinecap="round"
            />
          </g>
        ))}
      </g>
    );
  }
  // Gallien-Krueger solid-state bass head: black steel chassis with a finned
  // heatsink top, a silver control strip packed with small knobs, and the white
  // GK wordmark across the lower face.
  if (t === "gk") {
    const sil = c.panel;
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
          k="gk"
        />
        {/* finned heatsink top */}
        <path
          d="M8 20 h48 a3 3 0 0 1 3 3 v2.4 h-54 v-2.4 a3 3 0 0 1 3 -3 z"
          fill="#202327"
        />
        {Array.from({ length: 14 }).map((_, i) => (
          <line
            key={i}
            x1={10 + i * 3.05}
            y1={21}
            x2={10 + i * 3.05}
            y2={25}
            stroke="#0c0d0f"
            strokeWidth="0.55"
          />
        ))}
        {/* silver control strip */}
        <rect
          x="7"
          y="27"
          width="50"
          height="8"
          rx="1"
          fill={sil}
          stroke="rgba(0,0,0,0.32)"
          strokeWidth="0.4"
        />
        <circle
          cx="11"
          cy="31"
          r="1.5"
          fill="#16161a"
          stroke="rgba(0,0,0,0.45)"
          strokeWidth="0.4"
        />
        {Array.from({ length: 10 }).map((_, i) => (
          <circle
            key={i}
            cx={16 + i * 4.05}
            cy={31}
            r="1.45"
            fill="#16161a"
            stroke="rgba(0,0,0,0.45)"
            strokeWidth="0.3"
          />
        ))}
        {/* abstract logo plate + wordmark suggestion on the lower black face */}
        <rect
          x="8.5"
          y="37"
          width="6"
          height="3.6"
          rx="1"
          fill="none"
          stroke="#e8eaec"
          strokeWidth="0.6"
        />
        <rect
          x="10"
          y="38.45"
          width="3"
          height="0.9"
          rx="0.45"
          fill="#e8eaec"
          opacity="0.85"
        />
        <rect
          x="17"
          y="38.2"
          width="24"
          height="1.5"
          rx="0.6"
          fill="#e8eaec"
          opacity="0.85"
        />
      </g>
    );
  }
  // Mesa/Boogie Dual Rectifier: black tolex, diamond-plate steel upper panel with
  // a central chrome MESA badge flanked by vent louvers, and a black lower control
  // strip with a long row of knobs + the red power LED.
  if (t === "recto") {
    const dp = "dp" + uid;
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
          k="rc"
        />
        <defs>
          <pattern id={dp} patternUnits="userSpaceOnUse" width="6" height="6">
            <rect width="6" height="6" fill="#cdd0d2" />
            <g strokeLinecap="round" stroke="#8d9094" strokeWidth="1">
              <line x1="1" y1="2.3" x2="2.7" y2="0.6" />
              <line x1="3.3" y1="5.4" x2="5" y2="3.7" />
              <line x1="3.3" y1="0.6" x2="5" y2="2.3" />
              <line x1="1" y1="3.7" x2="2.7" y2="5.4" />
            </g>
          </pattern>
        </defs>
        {/* diamond-plate upper panel */}
        <rect
          x="7"
          y="22.5"
          width="50"
          height="9"
          rx="1"
          fill={`url(#${dp})`}
          stroke="rgba(0,0,0,0.4)"
          strokeWidth="0.5"
        />
        {/* vent louvers */}
        {[12, 38].map((vx, k) => (
          <g key={k}>
            <rect
              x={vx}
              y="24.6"
              width="10"
              height="4.8"
              rx="0.5"
              fill="#16161a"
            />
            {[0, 1, 2, 3].map((i) => (
              <line
                key={i}
                x1={vx + 1.4 + i * 2.2}
                y1="25.1"
                x2={vx + 1.4 + i * 2.2}
                y2="28.9"
                stroke="#aeb1b4"
                strokeWidth="0.6"
              />
            ))}
          </g>
        ))}
        {/* central chrome logo badge (abstract — no brand wordmark) */}
        <rect
          x="25.5"
          y="24"
          width="13"
          height="6"
          rx="1"
          fill="#16161a"
          stroke="#cdd0d2"
          strokeWidth="0.4"
        />
        <rect
          x="28.5"
          y="26.55"
          width="7"
          height="0.9"
          rx="0.45"
          fill="#e8eaec"
          opacity="0.9"
        />
        {/* lower black control strip */}
        <rect x="7" y="33" width="50" height="6" rx="1" fill="#0e0e10" />
        <circle cx="11" cy="36" r="1.2" fill="#d23b2b" />
        {Array.from({ length: 11 }).map((_, i) => (
          <circle
            key={i}
            cx={16 + i * 3.7}
            cy="36"
            r="1.35"
            fill="#1c1c1f"
            stroke="#5a5a5e"
            strokeWidth="0.3"
          />
        ))}
      </g>
    );
  }
  // Ampeg SVT (Blue Line): black tolex, silver control panel with chrome knobs +
  // black rocker switches and a blue jewel, and the dark silver-fleck Ampeg grille
  // below with the script badge.
  if (t === "svt") {
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
          k="svt"
        />
        {/* silver blue-line control panel across the TOP (ref) */}
        <rect
          x="7"
          y="21.5"
          width="50"
          height="7"
          rx="1"
          fill={c.panel}
          stroke="rgba(0,0,0,0.3)"
          strokeWidth="0.4"
        />
        {/* blue silkscreen line */}
        <rect
          x="9"
          y="22.4"
          width="46"
          height="0.5"
          rx="0.25"
          fill="#3f78c0"
          opacity="0.6"
        />
        <circle cx="10.5" cy="25.2" r="1.2" fill={c.jewel} />
        {[0, 1, 2, 3, 4].map((i) => (
          <g key={i}>
            <circle
              cx={18 + i * 5.2}
              cy="25.2"
              r="1.7"
              fill="#cfd3d7"
              stroke="rgba(0,0,0,0.45)"
              strokeWidth="0.4"
            />
            <line
              x1={18 + i * 5.2}
              y1="25.2"
              x2={18 + i * 5.2}
              y2="23.8"
              stroke="rgba(0,0,0,0.5)"
              strokeWidth="0.5"
            />
          </g>
        ))}
        {/* two rocker switches on the right of the panel */}
        {[46, 50].map((rx, i) => (
          <rect
            key={i}
            x={rx}
            y="23.8"
            width="3"
            height="2.8"
            rx="0.4"
            fill="#0e0e10"
          />
        ))}
        {/* tall silver-fleck Ampeg grille filling the lower face + script badge */}
        <GrilleCloth
          x={7}
          y={30}
          w={50}
          h={10}
          rx={1.3}
          tone="svt"
          uid={uid + "g"}
        />
        <ellipse
          cx="20"
          cy="35"
          rx="11"
          ry="2.6"
          fill="#15161a"
          opacity="0.82"
        />
        <rect
          x="15"
          y="34.55"
          width="10"
          height="0.9"
          rx="0.45"
          fill="#cfd3d7"
          opacity="0.85"
        />
      </g>
    );
  }
  // Ampeg B-15 Portaflex: the exposed amp chassis — a perforated black tube cage
  // up top, a silver control plate with a row of knobs + jacks below.
  if (t === "b15") {
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
  if (t === "mesa") {
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
  // Vox AC30 head + Fender Bassbreaker head: NO front control panel — the head is
  // just the cabinet grille (controls live on a top chassis). Vox keeps its diamond
  // grille; Bassbreaker shows a black grille with the combo's brushed-alu trim strip
  // across the top.
  if (t === "vox" || /^BBRK/i.test(lab)) {
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
  if (t === "friedman") {
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
