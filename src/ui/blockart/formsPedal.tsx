// src/ui/blockart/formsPedal.tsx — treadle + round-fuzz pedal form-factor
// renderers, split from ./forms so each file stays ≤500 lines. The rack/desk/
// screen/rockbox forms live in ./formsRack. Shared data/helpers live in ./shared.
import { lum, type PedalTone } from "./shared";

// Treadle / rocker pedal — wahs, whammy, volume pedal. Drawn TOP-DOWN like the
// Cry Baby ref: a portrait chassis with a small POSITION knob (or whammy LED) up
// top and a big ribbed rubber tread filling the lower body.
export function TreadleBody({
  c,
  g,
  lab: _lab,
}: {
  c: PedalTone;
  g: string;
  lab: string;
}) {
  const edge = "rgba(0,0,0,0.42)";
  const tread = "#1a1a1c"; // black rubber tread
  const ptr = lum(c.knob) > 0.55 ? "rgba(0,0,0,0.5)" : "#e8e8e8";
  const ribY = Array.from({ length: 11 }, (_, i) => 20.5 + i * 3.4);
  return (
    <g>
      {/* portrait treadle chassis */}
      <rect
        x="13"
        y="5"
        width="38"
        height="54"
        rx="4.5"
        fill={c.body}
        stroke={edge}
        strokeWidth="0.8"
      />
      {/* top control: POSITION knob (wah/volume) or red LED (whammy) */}
      {g === "whammy" ? (
        <circle
          cx="32"
          cy="11"
          r="2.4"
          fill={c.jewel ?? "#cf3a2e"}
          stroke="rgba(0,0,0,0.4)"
          strokeWidth="0.3"
        />
      ) : (
        <g>
          <circle
            cx="32"
            cy="11"
            r="3.1"
            fill={c.knob}
            stroke={edge}
            strokeWidth="0.5"
          />
          <line
            x1="32"
            y1="11"
            x2="32"
            y2="8.3"
            stroke={ptr}
            strokeWidth="0.7"
            strokeLinecap="round"
          />
        </g>
      )}
      {/* big ribbed rubber tread */}
      <rect
        x="16"
        y="17"
        width="32"
        height="40"
        rx="3"
        fill={tread}
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.4"
      />
      {ribY.map((y, i) => (
        <line
          key={i}
          x1="18"
          y1={y}
          x2="46"
          y2={y}
          stroke="rgba(255,255,255,0.10)"
          strokeWidth="0.9"
        />
      ))}
    </g>
  );
}

// Round Fuzz-Face — circular die-cast "smiley": IN/OUT jacks on top, two big
// knob eyes, and the signature triangular ribbed footswitch wedge ("mouth").
export function RoundBody({
  c,
  lab: _lab,
  uid,
}: {
  c: PedalTone;
  lab: string;
  uid: string;
}) {
  const edge = "rgba(0,0,0,0.45)";
  const cx = 32,
    cy = 30,
    r = 26;
  const clip = "ff" + uid;
  const apexY = cy - 3,
    halfW = 13;
  const yCorner = cy + Math.sqrt(r * r - halfW * halfW); // base corners sit on the chassis rim
  const wedge = `M${String(cx - halfW)} ${String(yCorner)} L${String(cx)} ${String(apexY)} L${String(cx + halfW)} ${String(yCorner)} A${String(r)} ${String(r)} 0 0 1 ${String(cx - halfW)} ${String(yCorner)} Z`;
  const ribs = [];
  for (let i = 1; i < 11; i++) {
    const t = i / 11,
      x = cx - halfW + t * halfW * 2;
    ribs.push(
      <line
        key={i}
        x1={x}
        y1={apexY}
        x2={x}
        y2={yCorner + 4}
        stroke="rgba(255,255,255,0.10)"
        strokeWidth="0.5"
      />,
    );
  }
  return (
    <g>
      {/* IN / OUT jack sockets poking out the top */}
      <rect
        x={cx - 13}
        y={cy - r - 2}
        width="7"
        height="4.5"
        rx="1"
        fill="rgba(0,0,0,0.45)"
        stroke={edge}
        strokeWidth="0.3"
      />
      <rect
        x={cx + 6}
        y={cy - r - 2}
        width="7"
        height="4.5"
        rx="1"
        fill="rgba(0,0,0,0.45)"
        stroke={edge}
        strokeWidth="0.3"
      />
      {/* round hammered chassis */}
      <circle
        cx={cx}
        cy={cy}
        r={r}
        fill={c.body}
        stroke={edge}
        strokeWidth="0.9"
      />
      <path
        d={`M${String(cx - r)} ${String(cy)} a${String(r)} ${String(r)} 0 0 0 ${String(2 * r)} 0`}
        fill="rgba(0,0,0,0.10)"
      />
      {/* triangular ribbed footswitch wedge */}
      <defs>
        <clipPath id={clip}>
          <path d={wedge} />
        </clipPath>
      </defs>
      <path
        d={wedge}
        fill="#15110f"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.4"
      />
      <g clipPath={`url(#${clip})`}>{ribs}</g>
      {/* footswitch button */}
      <circle
        cx={cx}
        cy={cy + 12.5}
        r="2.3"
        fill="#c9ccce"
        stroke="rgba(0,0,0,0.5)"
        strokeWidth="0.4"
      />
      {/* two knob 'eyes' */}
      {[-11.5, 11.5].map((dx, i) => (
        <g key={i}>
          <circle
            cx={cx + dx}
            cy={cy - 9}
            r="5"
            fill={c.knob}
            stroke="rgba(0,0,0,0.5)"
            strokeWidth="0.5"
          />
          <line
            x1={cx + dx}
            y1={cy - 9}
            x2={cx + dx}
            y2={cy - 13.6}
            stroke="#eaeaea"
            strokeWidth="0.8"
            strokeLinecap="round"
          />
        </g>
      ))}
    </g>
  );
}
