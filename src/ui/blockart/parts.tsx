// src/ui/blockart/parts.tsx — barrel re-exporting the procedural-SVG COMPONENTS
// of the block-art engine, split across ./partsCloth (weave/grille/chassis/speaker)
// and ./partsPanel (alu gradient/knob/silverface panel/EVH accent/corner protectors)
// to keep each file ≤500 lines. Shared data/helpers/types live in ./shared.
export { WeavePattern, GrilleCloth, ChassisBody, Speaker } from "./partsCloth";
export {
  AluGradient,
  SkirtedKnob,
  SilverfacePanel,
  EvhAccent,
  CornerProtectors,
} from "./partsPanel";
