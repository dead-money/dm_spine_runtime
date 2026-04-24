#!/usr/bin/env bash
# Walk every rig in spine-runtimes/examples/ and dump its setup-pose JSON
# into tests/fixtures/{rig}/{variant}/setup_pose.json.
#
# Atlas pairing rule: each rig has one non-PMA .atlas (<rig>.atlas) and
# one or more .skel variants (<rig>-<variant>.skel). We always use the
# non-PMA atlas because setup-pose math is pixel-independent and the PMA
# atlas just sets a blend-mode flag we don't read.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/build/spine_capture"
RUNTIME_ROOT="$(cd "$HERE/../.." && pwd)"
EXAMPLES="$RUNTIME_ROOT/../spine-runtimes/examples"
FIXTURES="$RUNTIME_ROOT/tests/fixtures"

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

    # Variant is whatever follows "<rig>-" in the skel basename, else empty.
    if [[ "$skel_base" == "$rig" ]]; then
        variant=""
    else
        variant="${skel_base#${rig}-}"
    fi

    # Match the non-PMA atlas. Fallback to any *.atlas without -pma.
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
        echo "  SKIP  $rig/$skel_base (no non-PMA atlas)" >&2
        missing_atlas=$((missing_atlas + 1))
        continue
    fi

    if [[ -z "$variant" ]]; then
        out_dir="$FIXTURES/$rig"
    else
        out_dir="$FIXTURES/$rig/$variant"
    fi
    mkdir -p "$out_dir"
    out="$out_dir/setup_pose.json"

    # Pass paths relative to the workspace root so the fixture's
    # `source_skel` / `source_atlas` provenance fields don't leak
    # absolute filesystem paths.
    rel_atlas="${atlas#$RUNTIME_ROOT/../}"
    rel_skel="${skel#$RUNTIME_ROOT/../}"
    if (cd "$RUNTIME_ROOT/.." && "$BIN" "$rel_atlas" "$rel_skel" "$out"); then
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
