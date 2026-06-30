// Speaker cabs — block-art renderer(s) split from the BlockArt engine.
// Shared data/helpers (tones, cloth, chassis, speakers) live in ./shared.
import { PEDAL_TONES, CAB_GRID, CLOTH, clothFor, type PedalTone } from "./shared";
import {
  GrilleCloth,
  ChassisBody,
  Speaker,
  AluGradient,
  EvhAccent,
} from "./parts";

export function CabBody({
  c,
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
  const edge = "rgba(0,0,0,0.42)";
  // Modern metal cabs (Mesa Rectifier / Diezel / Bogner closed-back) wear a black
  // diamond-mesh metal grille — but ONLY the cab, not the combo that shares the
  // tone, so it can't live in TONE_CLOTH. Override the cloth here in CabBody.
  const metalCab = t === "mesa" || t === "boutique";
  let cl = metalCab ? CLOTH.metaldiamond : clothFor(t);
  // Per-block grille overrides the lineage tone can't express (matched on the
  // normalized short label):
  if (lab.includes("BOG"))
    cl = CLOTH.saltpep; // Bogner — Marshall-style salt&pepper grille (not the Mesa diamond)
  else if (lab.includes("MES V30"))
    cl = CLOTH.blackjute; // Rectifier Trad — plain black grille (≠ HBBSClosed's diamond)
  else if (lab.includes("SWR 210"))
    cl = CLOTH.whitegrid; // SWR Redhead — white grid lines on a dark baffle
  else if (lab.includes("M4 JV30"))
    cl = CLOTH.silver; // Marshall 1960A V30 — silver grille
  const grids: Partial<Record<string, [number, number, number]>> = CAB_GRID;
  const [cols, rows] = grids[g] ?? CAB_GRID.cab1;
  // ALL cabs share ONE outer box, head-matching width (x5..59 = the AmpBody head),
  // so a cab always stacks flush under a head and the Cabinets view reads as a
  // consistent set. The speaker config (1x12…8x10) shows ONLY as the faint interior
  // hint — never as the silhouette.
  const chX = 5,
    chW = 54,
    chY = 6,
    chH = 52;
  // Most cabs frame the grille evenly at 4px; the Ampeg B-15 flip-top has a
  // visibly WIDE dark border between cab edge and grille — match the ref.
  const FRAME = t === "fliptop" ? 7 : 4;
  const baffleX = chX + FRAME,
    baffleY = chY + FRAME;
  const baffleW = chW - FRAME * 2,
    baffleH = chH - FRAME * 2;
  const cw = baffleW / cols,
    ch = baffleH / rows;
  const r = Math.min(cw, ch) * 0.36;

  // EVH 5150 III cab (evhmodern): near-black cabinet + EVH dark grille, dark
  // corner protectors, and the abstract white geometric corner accent. The 1×12
  // case is translated coord-for-coord from 1x12-evh-5150-g12h.svg.
  if (t === "evhmodern" || t === "evhblack") {
    // the 5150 1x12 ships in IVORY tolex (white cabinet, same dark grille + mark);
    // the 4x12 stays black.
    const evhBody = lab.includes("5150 112") ? "#e7e2d6" : c.body;
    return (
      <g>
        <rect
          x={chX}
          y={chY}
          width={chW}
          height={chH}
          rx="2.5"
          fill={evhBody}
          stroke="rgba(0,0,0,0.42)"
          strokeWidth="0.7"
        />
        {/* EVH dark grille (base + evh-grille pattern via GrilleCloth) */}
        <GrilleCloth
          x={baffleX}
          y={baffleY}
          w={baffleW}
          h={baffleH}
          rx={1.5}
          tone={t}
          uid={uid + "c"}
        />
        {/* speaker hint(s) behind the cloth */}
        <g opacity="0.38">
          {(() => {
            const out = [];
            for (let ry = 0; ry < rows; ry++)
              for (let cx = 0; cx < cols; cx++) {
                const x = baffleX + cw * cx + cw / 2,
                  y = baffleY + ch * ry + ch / 2;
                out.push(
                  <g key={`${String(cx)}-${String(ry)}`}>
                    <circle
                      cx={x}
                      cy={y}
                      r={r}
                      fill="none"
                      stroke="#3a3833"
                      strokeWidth="0.75"
                    />
                    <circle
                      cx={x}
                      cy={y}
                      r={r * 0.62}
                      fill="none"
                      stroke="#3a3833"
                      strokeWidth="0.5"
                    />
                    <circle cx={x} cy={y} r={r * 0.26} fill="#3a3833" />
                  </g>,
                );
              }
            return out;
          })()}
        </g>
        {/* abstract white EVH mark, centered H+V on the grille */}
        <EvhAccent
          cx={baffleX + baffleW / 2}
          cy={baffleY + baffleH / 2}
          s={7}
          stroke="#eef0f2"
        />
      </g>
    );
  }

  const speakers = [];
  if (lab.includes("VK 3")) {
    // Vibro-King 3x10 — triangular baffle: two 10" at the bottom, one centred up top.
    const cxm = baffleX + baffleW / 2;
    const rr = Math.min(baffleW, baffleH) * 0.24;
    const pts: [number, number][] = [
      [cxm, baffleY + baffleH * 0.32],
      [baffleX + baffleW * 0.3, baffleY + baffleH * 0.7],
      [baffleX + baffleW * 0.7, baffleY + baffleH * 0.7],
    ];
    pts.forEach(([x, y], i) => {
      speakers.push(<Speaker key={`t${String(i)}`} x={x} y={y} r={rr} cl={cl} />);
    });
  } else {
    for (let ry = 0; ry < rows; ry++)
      for (let cx = 0; cx < cols; cx++) {
        const x = baffleX + cw * cx + cw / 2,
          y = baffleY + ch * ry + ch / 2;
        speakers.push(
          <Speaker
            key={`${String(cx)}-${String(ry)}`}
            x={x}
            y={y}
            r={r}
            cl={cl}
          />,
        );
      }
  }

  // '68 Custom silverface cab (silverface): black tolex + a polished-alu trim
  // bezel framing the silver-turquoise sparkle grille.
  if (t === "silverface") {
    const gid = "sfc" + uid;
    return (
      <g>
        <defs>
          <AluGradient id={gid} />
        </defs>
        <ChassisBody
          x={chX}
          y={chY}
          w={chW}
          h={chH}
          rx={3}
          c={c}
          t={t}
          edge={edge}
          uid={uid}
          k="cb"
        />
        {/* alu trim bezel */}
        <rect
          x={chX + 2}
          y={chY + 2}
          width={chW - 4}
          height={chH - 4}
          rx="2"
          fill={`url(#${gid})`}
          stroke="rgba(0,0,0,0.35)"
          strokeWidth="0.4"
        />
        {/* sparkle grille inset */}
        <GrilleCloth
          x={baffleX}
          y={baffleY}
          w={baffleW}
          h={baffleH}
          rx={1.2}
          tone={t}
          uid={uid + "c"}
        />
        <g opacity="0.4">{speakers}</g>
      </g>
    );
  }

  return (
    <g>
      <ChassisBody
        x={chX}
        y={chY}
        w={chW}
        h={chH}
        rx={3}
        c={c}
        t={t}
        edge={edge}
        uid={uid}
        k="cb"
      />
      <GrilleCloth
        x={baffleX}
        y={baffleY}
        w={baffleW}
        h={baffleH}
        rx={1.5}
        tone={t}
        cloth={cl}
        uid={uid + "c"}
      />
      {/* speakers sit behind opaque cloth on a real cab — keep them as a faint hint of the configuration */}
      <g opacity="0.4">{speakers}</g>
    </g>
  );
}

// JTM45 'Bluesbreaker' family — the trem combo, its head, AND the two matching
// 2x12 cabs all render through THIS one body, so they are byte-identical (the user
// requires the cab to look exactly like the combo). Clean front: a black levant
// tolex box + a grey gold-piped grille below a top tolex band (the logo area) — the
// controls live on the TOP chassis, so there is NO front faceplate / knob row.
export function BluesbreakerBody({ c, uid }: { c: PedalTone; uid: string }) {
  return (
    <g>
      <ChassisBody x={5} y={6} w={54} h={52} rx={3} c={c} t="bluesbreaker" uid={uid} k="bb" />
      <GrilleCloth x={9} y={16} w={46} h={36} rx={1.6} tone="bluesbreaker" uid={uid + "bb"} />
    </g>
  );
}

// Impulse Response / External Cab — a neutral cab with a bold stamp ("IR" or
// "EXT CAB"), so a user IR / external-cab routing reads as distinct from the
// stock speaker cabinets it sits beside. Both refs are the same dark square +
// big-speaker + bottom wordmark; we keep the IR stamp style and just swap text.
export function IRBody({ uid, label = "IR" }: { uid: string; label?: string }) {
  const c = PEDAL_TONES.ink;
  // size the stamp box + font to the word so "EXT CAB" fits on one line
  const wide = label.length > 3;
  const boxW = wide ? 40 : 23;
  const fontSize = wide ? 8 : 11;
  // centre the stamp on the cab's vertical midline (chassis y6..58 → cy 32), not
  // high on the face (was y21.5 → cy29). Box + text share that centre line.
  const boxH = 15;
  const cy = 32;
  return (
    <g>
      <CabBody c={c} t="ink" g="cab1" lab="" uid={uid} />
      <rect
        x={32 - boxW / 2}
        y={cy - boxH / 2}
        width={boxW}
        height={boxH}
        rx="3.2"
        fill="rgba(12,13,17,0.82)"
        stroke="rgba(255,255,255,0.3)"
        strokeWidth="0.7"
      />
      <text
        x="32"
        y={cy}
        textAnchor="middle"
        dominantBaseline="central"
        fontFamily="'JetBrains Mono','SF Mono',monospace"
        fontWeight="700"
        fontSize={fontSize}
        letterSpacing="1"
        fill="#f3efe6"
      >
        {label}
      </text>
    </g>
  );
}

// ============================================================================
// Extra form-factor bodies (treadle / round fuzz / rack / desk / screen)
// — same flat sketch language, drawn into the 0..64 viewBox.
// ============================================================================
