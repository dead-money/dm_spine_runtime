#!/usr/bin/env bash
# Runs `cargo bench --bench frame`, then times spine-cpp on the same rig,
# animation and frame count, and prints ns/frame side by side.
# Honors SPINE_EXAMPLES, HOMMLET_SPINE_ASSETS (hommlet's Assets/Spine),
# SPINE_BENCH_FRAMES and SPINE_BENCH_RIG.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BIN="$HERE/build/spine_capture"
[[ -x "$BIN" ]] || { echo "spine_capture not built. Run: make" >&2; exit 1; }

rust_lines="$(cd "$ROOT" && cargo bench --quiet --bench frame 2>/dev/null)"
[[ -n "$rust_lines" ]] || { echo "no rigs benched (see cargo bench --bench frame)" >&2; exit 1; }

cpp_lines=""
while IFS= read -r line; do
    mapfile -t f < <(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print("\n".join([d["atlas"], d["skel"], d["animation"], str(d["frames"]), d["skin"]]))' "$line")
    if [[ -n "${f[4]}" ]]; then
        cpp_lines+="$("$BIN" --bench "${f[0]}" "${f[1]}" "${f[2]}" "${f[3]}" "${f[4]}")"$'\n'
    else
        cpp_lines+="$("$BIN" --bench "${f[0]}" "${f[1]}" "${f[2]}" "${f[3]}")"$'\n'
    fi
done <<< "$rust_lines"

python3 - "$rust_lines" "$cpp_lines" <<'PY'
import json, os, sys
rust = [json.loads(l) for l in sys.argv[1].splitlines() if l.strip()]
cpp = [json.loads(l) for l in sys.argv[2].splitlines() if l.strip()]
print(f"{'rig':<28}{'anim':<16}{'phase':<8}{'rust ns':>10}{'cpp ns':>10}{'ratio':>8}")
for r, c in zip(rust, cpp):
    rig = os.path.basename(r["skel"])
    for k in ("anim", "world", "render"):
        rv, cv = r[k + "_ns"], c[k + "_ns"]
        print(f"{rig:<28}{r['animation'][:15]:<16}{k:<8}{rv:>10.0f}{cv:>10.0f}{rv / cv if cv else 0:>8.2f}")
    rt = sum(r[k + "_ns"] for k in ("anim", "world", "render"))
    ct = sum(c[k + "_ns"] for k in ("anim", "world", "render"))
    print(f"{rig:<28}{'':<16}{'total':<8}{rt:>10.0f}{ct:>10.0f}{rt / ct if ct else 0:>8.2f}")
PY
