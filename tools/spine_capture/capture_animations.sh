#!/usr/bin/env bash
# Captures animation samples into
# tests/fixtures/animations/<rig>-<variant>/<anim>/t<ms>.json. The set is kept
# small: enough timeline variety to catch regressions without bloating the
# fixture tree.
#
# Each animation is sampled at 0, 25, 50, 75 and 99% of its duration, applied
# from the setup pose (see `spine_capture --anim`).

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/build/spine_capture"
RUNTIME_ROOT="$(cd "$HERE/../.." && pwd)"
EXAMPLES="${SPINE_EXAMPLES:-$RUNTIME_ROOT/../spine-runtimes/examples}"
FIXTURES="$RUNTIME_ROOT/tests/fixtures/animations"

if [[ ! -x "$BIN" ]]; then
    echo "spine_capture binary not built. Run: make" >&2
    exit 1
fi

# rig variant animation duration_seconds
# Durations must match each animation's duration in the export.
declare -a ROWS=(
    "spineboy pro walk 1.0"
    "spineboy pro run 0.6666667"
    "spineboy pro idle 1.6666667"
    "spineboy pro jump 1.3333334"
    "raptor pro walk 1.2666668"
    "raptor pro roar 2.1333334"
    "stretchyman pro sneak 1.8"
    "diamond pro rotation 2.0"
    "diamond pro size-changing-rotation 6.0"
)

mkdir -p "$FIXTURES"
captured=0
failed=0

for row in "${ROWS[@]}"; do
    # shellcheck disable=SC2206
    parts=( $row )
    rig="${parts[0]}"
    variant="${parts[1]}"
    anim="${parts[2]}"
    duration="${parts[3]}"

    export_dir="$EXAMPLES/$rig/export"
    atlas="$export_dir/$rig.atlas"
    skel="$export_dir/$rig-$variant.skel"
    if [[ ! -f "$atlas" || ! -f "$skel" ]]; then
        echo "  SKIP  $rig-$variant/$anim (missing atlas or skel)" >&2
        continue
    fi

    out_dir="$FIXTURES/$rig-$variant/$anim"
    mkdir -p "$out_dir"

    # Integer-millisecond filenames so the ordering on disk reflects time.
    for idx in 0.00 0.25 0.50 0.75 0.99; do
        time="$(awk "BEGIN { printf \"%.6f\", $idx * $duration }")"
        ms="$(awk "BEGIN { printf \"%04d\", $idx * $duration * 1000 }")"
        out="$out_dir/t${ms}.json"
        rel_atlas="${atlas#$RUNTIME_ROOT/../}"
        rel_skel="${skel#$RUNTIME_ROOT/../}"
        if (cd "$RUNTIME_ROOT/.." && "$BIN" --anim "$rel_atlas" "$rel_skel" "$out" "$anim" "$time"); then
            rel="${out#$RUNTIME_ROOT/}"
            echo "  ok    $rel (t=${time}s)"
            captured=$((captured + 1))
        else
            echo "  FAIL  $rig-$variant/$anim t=$time" >&2
            failed=$((failed + 1))
        fi
    done
done

echo
echo "captured: $captured"
echo "failed:   $failed"
if (( failed > 0 )); then
    exit 1
fi
