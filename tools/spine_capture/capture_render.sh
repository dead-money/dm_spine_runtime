#!/usr/bin/env bash
# Dump the spine-cpp SkeletonRenderer output for every example rig at
# setup pose. Fixtures land at tests/fixtures/render/{rig}/{variant}.json.
#
# Mirrors capture_all.sh's atlas-pairing rules (use non-PMA atlases).

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/build/spine_capture"
RUNTIME_ROOT="$(cd "$HERE/../.." && pwd)"
EXAMPLES="$RUNTIME_ROOT/../spine-runtimes/examples"
FIXTURES="$RUNTIME_ROOT/tests/fixtures/render"

if [[ ! -x "$BIN" ]]; then
    echo "spine_capture binary not built. Run: make" >&2
    exit 1
fi
if [[ ! -d "$EXAMPLES" ]]; then
    echo "examples dir not found: $EXAMPLES" >&2
    exit 1
fi

mkdir -p "$FIXTURES"

captured=0
failed=0
missing_atlas=0

while IFS= read -r -d '' skel; do
    export_dir="$(dirname "$skel")"
    rig="$(basename "$(dirname "$export_dir")")"
    skel_base="$(basename "$skel" .skel)"

    if [[ "$skel_base" == "$rig" ]]; then
        variant=""
    else
        variant="${skel_base#${rig}-}"
    fi

    atlas="$export_dir/$rig.atlas"
    if [[ ! -f "$atlas" ]]; then
        atlas=""
        for a in "$export_dir"/*.atlas; do
            [[ -f "$a" ]] || continue
            [[ "$a" == *-pma.atlas ]] && continue
            atlas="$a"
            break
        done
    fi
    if [[ -z "$atlas" ]]; then
        missing_atlas=$((missing_atlas + 1))
        continue
    fi

    if [[ -z "$variant" ]]; then
        out_dir="$FIXTURES/$rig"
        out="$FIXTURES/$rig.json"
    else
        out_dir="$FIXTURES/$rig"
        out="$out_dir/$variant.json"
    fi
    mkdir -p "$(dirname "$out")"

    rel_atlas="${atlas#$RUNTIME_ROOT/../}"
    rel_skel="${skel#$RUNTIME_ROOT/../}"
    if (cd "$RUNTIME_ROOT/.." && "$BIN" --render "$rel_atlas" "$rel_skel" "$out"); then
        captured=$((captured + 1))
        rel="${out#$RUNTIME_ROOT/}"
        echo "  ok    $rel"
    else
        failed=$((failed + 1))
        echo "  FAIL  $rig/$skel_base" >&2
    fi
done < <(find "$EXAMPLES" -mindepth 3 -maxdepth 3 -name '*.skel' -print0 | sort -z)

echo
echo "captured: $captured"
echo "failed:   $failed"
echo "no-atlas: $missing_atlas"

if (( failed > 0 )); then
    exit 1
fi
