#!/usr/bin/env python3
"""
Tone Master Pro Preset Toolkit — decode, encode, inspect, diff, and analyze .preset files.

Usage:
    python3 preset_toolkit.py decode  <file.preset> [--output file.json] [--pretty]
    python3 preset_toolkit.py encode  <file.json>   [--output file.preset]
    python3 preset_toolkit.py inspect <file.preset>
    python3 preset_toolkit.py diff    <a.preset> <b.preset>
    python3 preset_toolkit.py effects <file.preset>
"""
import argparse
import json
import sys

XOR_KEY = bytes([0x4A, 0x4C, 0x44])  # "JLD"


# ---------------------------------------------------------------------------
# Core: XOR encode/decode (self-inverse)
# ---------------------------------------------------------------------------

def xor_cipher(data):
    """Apply the 3-byte repeating XOR cipher. Same operation encodes and decodes."""
    return bytes(b ^ XOR_KEY[i % 3] for i, b in enumerate(data))


def read_preset(path):
    """Read a .preset file and return parsed JSON."""
    with open(path, 'rb') as f:
        raw = f.read()
    decoded = xor_cipher(raw)
    try:
        text = decoded.decode('utf-8')
    except UnicodeDecodeError:
        print(f"Error: decoded data is not valid UTF-8. The XOR key may have changed.", file=sys.stderr)
        print(f"Key used: {' '.join(f'0x{b:02X}' for b in XOR_KEY)} (ASCII: {XOR_KEY.decode('ascii')})", file=sys.stderr)
        sys.exit(1)
    try:
        return json.loads(text)
    except json.JSONDecodeError as e:
        print(f"Error: decoded data is not valid JSON: {e}", file=sys.stderr)
        print(f"First 200 chars: {text[:200]}", file=sys.stderr)
        sys.exit(1)


def write_preset(path, data):
    """Write a JSON object as a .preset file."""
    text = json.dumps(data, separators=(',', ':'))
    encoded = xor_cipher(text.encode('utf-8'))
    with open(path, 'wb') as f:
        f.write(encoded)


# ---------------------------------------------------------------------------
# Helpers: extract info from preset JSON
# ---------------------------------------------------------------------------

def get_effects_in_slot(nodes, slot_name):
    """Extract effects from a guitarNodes/micNodes slot (audioGraph array format)."""
    slot = nodes.get(slot_name, [])
    effects = []
    for node in slot:
        if node.get('nodeType') != 'dspUnit':
            continue
        params = node.get('dspUnitParameters', {})
        effects.append({
            'fenderId': node.get('FenderId', '?'),
            'slot': slot_name,
            'bypass': params.get('bypass', False),
            'outputLevel': params.get('outputLevel'),
            'gain': params.get('gain'),
        })
    return effects


def get_all_effects(audio_graph):
    """Get all effects from audioGraph."""
    effects = []
    for slot in ('G1', 'G2', 'G3', 'G4', 'G5', 'G6', 'G7'):
        effects.extend(get_effects_in_slot(audio_graph.get('guitarNodes', {}), slot))
    for slot in ('M1', 'M2', 'M3', 'M4'):
        effects.extend(get_effects_in_slot(audio_graph.get('micNodes', {}), slot))
    return effects


def resolve_scene(audio_graph, scene):
    """Resolve a scene's effective state by merging its diff onto audioGraph defaults.

    Returns a list of effects with their effective parameters for this scene.
    """
    base_effects = get_all_effects(audio_graph)

    # Build override map from scene diff
    overrides = {}
    for nodes_key in ('guitarNodes', 'micNodes'):
        scene_nodes = scene.get(nodes_key, {})
        for slot_name, slot_overrides in scene_nodes.items():
            if not isinstance(slot_overrides, dict):
                continue
            for fender_id, node_override in slot_overrides.items():
                param_overrides = node_override.get('dspUnitParameters', {})
                overrides[(slot_name, fender_id)] = param_overrides

    # Apply overrides to base effects
    resolved = []
    for eff in base_effects:
        key = (eff['slot'], eff['fenderId'])
        merged = dict(eff)
        if key in overrides:
            for param, val in overrides[key].items():
                merged[param] = val
        resolved.append(merged)
    return resolved


def is_amp(fender_id):
    """Heuristic: identify amp-related blocks (heads, combos, cab IRs) by naming patterns."""
    amp_keywords = ('Reverb', 'Combo', 'Marshall', 'JCM', 'Plexi', 'Twin', 'Deluxe',
                    'Bassman', 'Champ', 'Princeton', 'Vibrolux', 'Super', 'Bandmaster',
                    'Orange', 'Rockerverb', 'Mesa', 'Rectifier', 'Bogner', 'Friedman',
                    'Soldano', 'Diezel', 'EVH', 'Engl', 'Matchless', 'Vox', 'AC30',
                    'AC15', 'JTM45', 'JVM', 'SilverJubilee', 'AFD100',
                    'NoFx', 'CabIR')
    return any(kw.lower() in fender_id.lower() for kw in amp_keywords)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

def cmd_decode(args):
    data = read_preset(args.file)
    if args.pretty:
        text = json.dumps(data, indent=2)
    else:
        text = json.dumps(data)
    if args.output:
        with open(args.output, 'w') as f:
            f.write(text)
        print(f"Decoded to {args.output}")
    else:
        print(text)


def cmd_encode(args):
    with open(args.file, 'r') as f:
        data = json.load(f)
    output = args.output
    if not output:
        if args.file.endswith('.json'):
            output = args.file[:-5] + '.preset'
        else:
            print("Error: cannot infer output path. Use --output to specify.", file=sys.stderr)
            sys.exit(1)
    write_preset(output, data)
    print(f"Encoded to {output}")


def cmd_inspect(args):
    data = read_preset(args.file)
    ag = data.get('audioGraph', {})
    scenes = data.get('scenes', [])
    info = data.get('info', {})

    # Header
    name = info.get('displayName', '(unnamed)')
    version = info.get('version', '?')
    product = info.get('product_id', '?')
    print(f"Preset: {name}")
    print(f"  Version: {version}  Product: {product}")
    print(f"  Template: {ag.get('template', '?')}")
    print(f"  Preset Level: {ag.get('presetLevel', '?')}")
    print(f"  BPM: {data.get('bpm', '?')}")
    print(f"  Spillover: {ag.get('spillover', '?')}")
    print()

    # Default signal chain
    effects = get_all_effects(ag)
    active = [e for e in effects if not e['bypass']]
    print(f"Signal Chain (default): {len(active)} active / {len(effects)} total")
    for e in effects:
        status = "  " if not e['bypass'] else "x "
        level = f"  out={e['outputLevel']}" if e['outputLevel'] is not None else ""
        gain = f"  gain={e['gain']}" if e['gain'] is not None else ""
        amp_tag = " [AMP]" if is_amp(e['fenderId']) else ""
        print(f"  {status}{e['slot']}: {e['fenderId']}{amp_tag}{level}{gain}")
    print()

    # Scenes
    if scenes:
        print(f"Scenes ({len(scenes)}):")
        for i, scene in enumerate(scenes):
            resolved = resolve_scene(ag, scene)
            active_resolved = [e for e in resolved if not e['bypass']]
            amps = [e for e in active_resolved if is_amp(e['fenderId'])]

            amp_info = ""
            if amps:
                amp_names = ', '.join(a['fenderId'].replace('ACD_', '') for a in amps)
                amp_levels = ', '.join(
                    f"out={a['outputLevel']}" for a in amps if a['outputLevel'] is not None
                )
                amp_info = f"  amp: {amp_names}"
                if amp_levels:
                    amp_info += f" ({amp_levels})"

            # Count overrides in this scene
            override_count = 0
            for nodes_key in ('guitarNodes', 'micNodes'):
                for slot in scene.get(nodes_key, {}).values():
                    if isinstance(slot, dict):
                        override_count += len(slot)

            print(f"  [{i}] {scene.get('sceneName', '(unnamed)')}"
                  f"  ({override_count} overrides, {len(active_resolved)} active){amp_info}")
    else:
        print("No scenes")


def cmd_diff(args):
    a = read_preset(args.file_a)
    b = read_preset(args.file_b)

    name_a = a.get('info', {}).get('displayName', args.file_a)
    name_b = b.get('info', {}).get('displayName', args.file_b)
    print(f"Comparing: {name_a} vs {name_b}")
    print()

    # Top-level scalar diffs
    for key in ('bpm', 'expAutoOffTime'):
        va, vb = a.get(key), b.get(key)
        if va != vb:
            print(f"  {key}: {va} -> {vb}")

    # audioGraph diffs
    ag_a, ag_b = a.get('audioGraph', {}), b.get('audioGraph', {})
    for key in ('presetLevel', 'template', 'spillover'):
        va, vb = ag_a.get(key), ag_b.get(key)
        if va != vb:
            print(f"  audioGraph.{key}: {va} -> {vb}")

    # Effect diffs
    effects_a = {(e['slot'], e['fenderId']): e for e in get_all_effects(ag_a)}
    effects_b = {(e['slot'], e['fenderId']): e for e in get_all_effects(ag_b)}

    keys_a, keys_b = set(effects_a.keys()), set(effects_b.keys())
    added = keys_b - keys_a
    removed = keys_a - keys_b
    common = keys_a & keys_b

    if added:
        print(f"\n  Effects added ({len(added)}):")
        for k in sorted(added):
            print(f"    + {k[0]}: {k[1]}")
    if removed:
        print(f"\n  Effects removed ({len(removed)}):")
        for k in sorted(removed):
            print(f"    - {k[0]}: {k[1]}")

    changed = []
    for k in sorted(common):
        ea, eb = effects_a[k], effects_b[k]
        diffs = []
        for param in ('bypass', 'outputLevel', 'gain'):
            if ea.get(param) != eb.get(param):
                diffs.append(f"{param}: {ea.get(param)} -> {eb.get(param)}")
        if diffs:
            changed.append((k, diffs))

    if changed:
        print(f"\n  Effects changed ({len(changed)}):")
        for k, diffs in changed:
            print(f"    ~ {k[0]}: {k[1]}")
            for d in diffs:
                print(f"      {d}")

    # Scene diffs
    scenes_a = a.get('scenes', [])
    scenes_b = b.get('scenes', [])
    names_a = [s.get('sceneName', f'scene_{i}') for i, s in enumerate(scenes_a)]
    names_b = [s.get('sceneName', f'scene_{i}') for i, s in enumerate(scenes_b)]

    if names_a != names_b:
        print(f"\n  Scenes: {names_a} -> {names_b}")
    elif len(scenes_a) > 0:
        scene_changes = 0
        for i, (sa, sb) in enumerate(zip(scenes_a, scenes_b)):
            if sa != sb:
                scene_changes += 1
        if scene_changes:
            print(f"\n  Scenes: {scene_changes}/{len(scenes_a)} scenes differ")

    if not any([added, removed, changed]) and names_a == names_b:
        print("\n  No structural differences found")


def cmd_effects(args):
    data = read_preset(args.file)
    ag = data.get('audioGraph', {})
    effects = get_all_effects(ag)

    print(f"Effects in {args.file} ({len(effects)} total):")
    print()
    print(f"{'Slot':<5} {'FenderId':<40} {'Bypass':<8} {'Output':<10} {'Gain':<10}")
    print("-" * 73)
    for e in effects:
        bypass = "OFF" if e['bypass'] else "ON"
        out = f"{e['outputLevel']:.2f}" if e['outputLevel'] is not None else "-"
        gain = f"{e['gain']:.2f}" if e['gain'] is not None else "-"
        amp = " *" if is_amp(e['fenderId']) else ""
        print(f"{e['slot']:<5} {e['fenderId']:<40} {bypass:<8} {out:<10} {gain:<10}{amp}")

    # Also show unique FenderIds across all scenes
    scenes = data.get('scenes', [])
    scene_ids = set()
    for scene in scenes:
        for nodes_key in ('guitarNodes', 'micNodes'):
            for slot_overrides in scene.get(nodes_key, {}).values():
                if isinstance(slot_overrides, dict):
                    scene_ids.update(slot_overrides.keys())

    base_ids = {e['fenderId'] for e in effects}
    if scene_ids - base_ids:
        print(f"\nAdditional FenderIds in scene overrides (not in base chain):")
        for fid in sorted(scene_ids - base_ids):
            print(f"  {fid}")

    print(f"\n* = identified as amp head")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Tone Master Pro Preset Toolkit",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="XOR key: 0x4A 0x4C 0x44 (ASCII: JLD)")
    sub = parser.add_subparsers(dest='command', required=True)

    p_decode = sub.add_parser('decode', help='Decode .preset to JSON')
    p_decode.add_argument('file', help='Input .preset file')
    p_decode.add_argument('--output', '-o', help='Output JSON file (default: stdout)')
    p_decode.add_argument('--pretty', '-p', action='store_true', help='Pretty-print JSON')

    p_encode = sub.add_parser('encode', help='Encode JSON to .preset')
    p_encode.add_argument('file', help='Input JSON file')
    p_encode.add_argument('--output', '-o', help='Output .preset file')

    p_inspect = sub.add_parser('inspect', help='Inspect preset structure')
    p_inspect.add_argument('file', help='Input .preset file')

    p_diff = sub.add_parser('diff', help='Diff two presets')
    p_diff.add_argument('file_a', help='First .preset file')
    p_diff.add_argument('file_b', help='Second .preset file')

    p_effects = sub.add_parser('effects', help='List effects in preset')
    p_effects.add_argument('file', help='Input .preset file')

    args = parser.parse_args()
    {'decode': cmd_decode, 'encode': cmd_encode, 'inspect': cmd_inspect,
     'diff': cmd_diff, 'effects': cmd_effects}[args.command](args)


if __name__ == "__main__":
    main()
