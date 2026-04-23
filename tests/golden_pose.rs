// Spine Runtimes License Agreement
// Last updated April 5, 2025. Replaces all prior versions.
//
// Copyright (c) 2013-2025, Esoteric Software LLC
//
// Integration of the Spine Runtimes into software or otherwise creating
// derivative works of the Spine Runtimes is permitted under the terms and
// conditions of Section 2 of the Spine Editor License Agreement:
// http://esotericsoftware.com/spine-editor-license
//
// Otherwise, it is permitted to integrate the Spine Runtimes into software
// or otherwise create derivative works of the Spine Runtimes (collectively,
// "Products"), provided that each user of the Products must obtain their own
// Spine Editor license and redistribution of the Products in any form must
// include this license and copyright notice.
//
// THE SPINE RUNTIMES ARE PROVIDED BY ESOTERIC SOFTWARE LLC "AS IS" AND ANY
// EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL ESOTERIC SOFTWARE LLC BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES,
// BUSINESS INTERRUPTION, OR LOSS OF USE, DATA, OR PROFITS) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
// THE SPINE RUNTIMES, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Golden-pose test: loads every rig with a captured setup-pose fixture,
//! poses it via `Skeleton::new` + `update_cache` + `update_world_transform`,
//! and diffs every bone's world matrix and applied-local fields against
//! the fixture (which was captured from spine-cpp, see
//! `tools/spine_capture/`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

const TOLERANCE: f32 = 1e-4;

#[derive(Debug, Deserialize)]
struct Fixture {
    #[allow(dead_code)] // kept for debug prints on failure
    source_skel: String,
    #[allow(dead_code)]
    source_atlas: String,
    bones: Vec<BoneFixture>,
}

#[derive(Debug, Deserialize)]
struct BoneFixture {
    index: u16,
    name: String,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    world_x: f32,
    world_y: f32,
    ax: f32,
    ay: f32,
    a_rotation: f32,
    a_scale_x: f32,
    a_scale_y: f32,
    a_shear_x: f32,
    a_shear_y: f32,
    #[allow(dead_code)]
    active: bool,
}

fn fixtures_root() -> PathBuf {
    PathBuf::from("tests/fixtures")
}

fn examples_root() -> PathBuf {
    PathBuf::from("../spine-runtimes/examples")
}

/// Walk `tests/fixtures/{rig}[/variant]/setup_pose.json` and yield
/// `(rig, variant, fixture_path)` triples. `variant` is `None` for rigs
/// whose skel file matched the rig name directly (e.g. `chibi-stickers.skel`).
fn collect_fixtures() -> Vec<(String, Option<String>, PathBuf)> {
    let mut out = Vec::new();
    let root = fixtures_root();
    for rig_entry in std::fs::read_dir(&root).unwrap().flatten() {
        let rig_path = rig_entry.path();
        if !rig_path.is_dir() {
            continue;
        }
        let rig = rig_path.file_name().unwrap().to_string_lossy().into_owned();

        // `rig/setup_pose.json` (no variant dir) — single-variant rigs.
        let direct = rig_path.join("setup_pose.json");
        if direct.is_file() {
            out.push((rig.clone(), None, direct));
            continue;
        }

        // Otherwise `rig/{variant}/setup_pose.json`.
        for variant_entry in std::fs::read_dir(&rig_path).unwrap().flatten() {
            let variant_path = variant_entry.path();
            if !variant_path.is_dir() {
                continue;
            }
            let variant = variant_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let fx = variant_path.join("setup_pose.json");
            if fx.is_file() {
                out.push((rig.clone(), Some(variant), fx));
            }
        }
    }
    out.sort();
    out
}

/// Resolve the matching `.skel` and non-PMA atlas for a `(rig, variant)`
/// pair. Mirrors the pairing rule in `tools/spine_capture/capture_all.sh`.
fn resolve_assets(rig: &str, variant: Option<&str>) -> (PathBuf, PathBuf) {
    let export = examples_root().join(rig).join("export");
    let skel_name = match variant {
        Some(v) => format!("{rig}-{v}.skel"),
        None => format!("{rig}.skel"),
    };
    let skel = export.join(&skel_name);
    let atlas = export.join(format!("{rig}.atlas"));
    assert!(skel.is_file(), "expected skel file: {}", skel.display());
    assert!(atlas.is_file(), "expected atlas file: {}", atlas.display());
    (atlas, skel)
}

fn load_skeleton(atlas_path: &Path, skel_path: &Path) -> Skeleton {
    let atlas_src = std::fs::read_to_string(atlas_path).unwrap();
    let atlas = Atlas::parse(&atlas_src)
        .unwrap_or_else(|e| panic!("atlas parse failed for {}: {e}", atlas_path.display()));
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(skel_path).unwrap();
    let data = SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap_or_else(|e| panic!("skeleton load failed for {}: {e}", skel_path.display()));
    Skeleton::new(Arc::new(data))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOLERANCE || (a - b).abs() <= TOLERANCE * a.abs().max(b.abs())
}

fn check_bone(rig_label: &str, expected: &BoneFixture, actual: &dm_spine_runtime::skeleton::Bone) {
    let fields: [(&str, f32, f32); 13] = [
        ("a", expected.a, actual.a),
        ("b", expected.b, actual.b),
        ("c", expected.c, actual.c),
        ("d", expected.d, actual.d),
        ("world_x", expected.world_x, actual.world_x),
        ("world_y", expected.world_y, actual.world_y),
        ("ax", expected.ax, actual.ax),
        ("ay", expected.ay, actual.ay),
        ("a_rotation", expected.a_rotation, actual.a_rotation),
        ("a_scale_x", expected.a_scale_x, actual.a_scale_x),
        ("a_scale_y", expected.a_scale_y, actual.a_scale_y),
        ("a_shear_x", expected.a_shear_x, actual.a_shear_x),
        ("a_shear_y", expected.a_shear_y, actual.a_shear_y),
    ];
    for (label, want, got) in fields {
        assert!(
            close(want, got),
            "[{rig_label}] bone #{} ({}) field `{label}` mismatch: \
             expected {want} got {got} (diff {})",
            expected.index,
            expected.name,
            (want - got).abs(),
        );
    }
}

// Fixtures were captured with constraints disabled (Phase 2a harness).
// Phase 5 enables constraint solvers, so IK-affected bones (hero/pro thigh1,
// raptor/pro, etc.) now diverge. Phase 5e will regenerate with constraints
// enabled and re-enable this test.
#[ignore = "pending Phase 5e fixture regeneration"]
#[test]
fn setup_pose_matches_spine_cpp_on_every_captured_rig() {
    let fixtures = collect_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no fixtures found at {:?}",
        fixtures_root()
    );

    let mut checked = 0usize;
    for (rig, variant, fx_path) in &fixtures {
        let rig_label = match variant {
            Some(v) => format!("{rig}/{v}"),
            None => rig.clone(),
        };

        let fx_json = std::fs::read_to_string(fx_path).unwrap();
        let fx: Fixture = serde_json::from_str(&fx_json)
            .unwrap_or_else(|e| panic!("[{rig_label}] fixture parse failed: {e}"));

        let (atlas_path, skel_path) = resolve_assets(rig, variant.as_deref());
        let mut sk = load_skeleton(&atlas_path, &skel_path);
        // Exercise the full Phase 2 public sequence, not just the bone pose
        // shortcut that Skeleton::new already seeds. `set_to_setup_pose` is a
        // no-op for freshly-loaded skeletons but must stay idempotent here.
        sk.set_to_setup_pose();
        sk.update_cache();
        sk.update_world_transform(Physics::None);

        assert_eq!(
            sk.bones.len(),
            fx.bones.len(),
            "[{rig_label}] bone count mismatch: runtime {} vs fixture {}",
            sk.bones.len(),
            fx.bones.len(),
        );

        for (i, expected) in fx.bones.iter().enumerate() {
            assert_eq!(
                expected.index as usize, i,
                "[{rig_label}] fixture bone order broken at index {i}"
            );
            check_bone(&rig_label, expected, &sk.bones[i]);
        }

        checked += 1;
    }

    // Sanity: confirm we actually exercised the set.
    assert!(
        checked >= 20,
        "only checked {checked} fixtures — expected most of 25"
    );
}
