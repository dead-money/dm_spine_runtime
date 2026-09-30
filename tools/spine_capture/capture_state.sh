#!/usr/bin/env bash
# Scripted AnimationState captures into tests/fixtures/state/<name>.json.
# See `spine_capture --state` for the script commands.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/build/spine_capture"
RUNTIME_ROOT="$(cd "$HERE/../.." && pwd)"
EXAMPLES="${SPINE_EXAMPLES:-$RUNTIME_ROOT/../spine-runtimes/examples}"
FIXTURES="$RUNTIME_ROOT/tests/fixtures/state"

[[ -x "$BIN" ]] || { echo "spine_capture not built. Run: make" >&2; exit 1; }
mkdir -p "$FIXTURES"

# name rig skel script
ROWS=(
    "crossfade spineboy spineboy-pro mix:0.2;set:0:walk:1;step:20;dump;set:0:run:1;step:6;dump;step:20;dump"
    "additive spineboy spineboy-pro set:0:walk:1;set:1:aim:1;additive:1:1;alpha:1:0.5;step:10;dump;step:10;dump"
    "queue-empty spineboy spineboy-pro mix:0.15;set:0:idle:1;add:0:jump:0:0.3;step:30;dump;step:30;dump;empty:0:0.25;step:10;dump;step:20;dump"
    "two-tracks spineboy spineboy-pro mix:0.3;set:0:run:1;step:5;set:1:shoot:0;step:5;dump;set:0:walk:1;interp:0:smooth;step:8;dump;step:12;dump"
    "attachments spineboy spineboy-pro mix:0.2;set:0:walk:1;step:10;set:0:death:0;step:10;dump;step:40;dump;set:0:idle:1;step:5;dump;step:30;dump"
    "hold raptor raptor-pro mix:0.25;set:0:walk:1;step:15;set:0:jump:0;step:8;dump;step:20;dump;addempty:0:0.3:0;step:90;dump"
    "deform stretchyman stretchyman-pro mix:0.3;set:0:idle:1;step:10;set:0:sneak:1;step:8;dump;step:20;dump"
    "sliders diamond diamond-pro mix:0.2;set:0:rotation:1;step:10;dump;set:0:idle-rotating:1;step:6;dump;step:20;dump"
)

for row in "${ROWS[@]}"; do
    read -r name rig skel script <<< "$row"
    export_dir="$EXAMPLES/$rig/export"
    rel_atlas="${export_dir#$RUNTIME_ROOT/../}/$rig.atlas"
    rel_skel="${export_dir#$RUNTIME_ROOT/../}/$skel.skel"
    (cd "$RUNTIME_ROOT/.." && "$BIN" --state "$rel_atlas" "$rel_skel" "$FIXTURES/$name.json" "$script")
    echo "  ok    tests/fixtures/state/$name.json"
done
