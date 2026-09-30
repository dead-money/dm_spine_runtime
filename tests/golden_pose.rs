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

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

const TOLERANCE: f32 = 1e-4;

#[derive(Debug, Deserialize)]
struct Fixture {
    source_skel: String,
    source_atlas: String,
    update_cache: Vec<String>,
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

/// Every `setup_pose.json` under the fixtures root, labelled by its
/// directory relative to that root.
fn collect_fixtures() -> Vec<(String, PathBuf)> {
    let root = fixtures_root();
    common::json_files(&root)
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "setup_pose.json"))
        .map(|p| {
            let label = p
                .parent()
                .and_then(|d| d.strip_prefix(&root).ok())
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            (label, p)
        })
        .collect()
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

/// Update cache as the capture prints it: `bone:<name>`, `ik:<name>`, ...
fn update_cache_names(sk: &Skeleton) -> Vec<String> {
    use dm_spine_runtime::data::ConstraintData;
    use dm_spine_runtime::skeleton::UpdateCacheEntry;
    let data = sk.data();
    sk.update_cache_entries()
        .iter()
        .map(|e| match e {
            UpdateCacheEntry::Bone(b) => format!("bone:{}", data.bones[b.index()].name),
            UpdateCacheEntry::Constraint(c) => {
                let kind = match &data.constraints[c.index()] {
                    ConstraintData::Ik(_) => "ik",
                    ConstraintData::Transform(_) => "transform",
                    ConstraintData::Path(_) => "path",
                    ConstraintData::Physics(_) => "physics",
                    ConstraintData::Slider(_) => "slider",
                };
                format!("{kind}:{}", data.constraints[c.index()].name())
            }
        })
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOLERANCE || (a - b).abs() <= TOLERANCE * a.abs().max(b.abs())
}

fn check_bone(
    rig_label: &str,
    expected: &BoneFixture,
    actual: &dm_spine_runtime::skeleton::BonePose,
) {
    let fields: [(&str, f32, f32); 13] = [
        ("a", expected.a, actual.a),
        ("b", expected.b, actual.b),
        ("c", expected.c, actual.c),
        ("d", expected.d, actual.d),
        ("world_x", expected.world_x, actual.world_x),
        ("world_y", expected.world_y, actual.world_y),
        ("ax", expected.ax, actual.x),
        ("ay", expected.ay, actual.y),
        ("a_rotation", expected.a_rotation, actual.rotation),
        ("a_scale_x", expected.a_scale_x, actual.scale_x),
        ("a_scale_y", expected.a_scale_y, actual.scale_y),
        ("a_shear_x", expected.a_shear_x, actual.shear_x),
        ("a_shear_y", expected.a_shear_y, actual.shear_y),
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

fn first_bone_mismatch(
    rig_label: &str,
    expected: &BoneFixture,
    actual: &dm_spine_runtime::skeleton::BonePose,
) -> Option<String> {
    let fields: [(&str, f32, f32); 13] = [
        ("a", expected.a, actual.a),
        ("b", expected.b, actual.b),
        ("c", expected.c, actual.c),
        ("d", expected.d, actual.d),
        ("world_x", expected.world_x, actual.world_x),
        ("world_y", expected.world_y, actual.world_y),
        ("ax", expected.ax, actual.x),
        ("ay", expected.ay, actual.y),
        ("a_rotation", expected.a_rotation, actual.rotation),
        ("a_scale_x", expected.a_scale_x, actual.scale_x),
        ("a_scale_y", expected.a_scale_y, actual.scale_y),
        ("a_shear_x", expected.a_shear_x, actual.shear_x),
        ("a_shear_y", expected.a_shear_y, actual.shear_y),
    ];
    for (label, want, got) in fields {
        if !close(want, got) {
            return Some(format!(
                "[{rig_label}] bone #{} ({}) .{label}: want {want} got {got} (Δ{:.4})",
                expected.index,
                expected.name,
                (want - got).abs(),
            ));
        }
    }
    let _ = check_bone; // keep helper visible for narrow-test debugging
    None
}

// Phase 5e: fixtures regenerated with the full constraint pipeline
// enabled via Skeleton::updateWorldTransform.
#[test]
fn setup_pose_matches_spine_cpp_on_every_captured_rig() {
    let fixtures = collect_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no fixtures found at {:?}",
        fixtures_root()
    );

    let mut checked = 0usize;
    for (rig_label, fx_path) in &fixtures {
        let fx_json = std::fs::read_to_string(fx_path).unwrap();
        let fx: Fixture = serde_json::from_str(&fx_json)
            .unwrap_or_else(|e| panic!("[{rig_label}] fixture parse failed: {e}"));

        let atlas_path = common::example_path(&fx.source_atlas);
        let skel_path = common::example_path(&fx.source_skel);
        let mut sk = load_skeleton(&atlas_path, &skel_path);
        sk.setup_pose();
        sk.update_world_transform(Physics::None);

        let cache = update_cache_names(&sk);
        assert_eq!(cache, fx.update_cache, "[{rig_label}] update cache order");
        assert_eq!(
            sk.bones.len(),
            fx.bones.len(),
            "[{rig_label}] bone count mismatch"
        );
        // The capture validates local transforms constraints left stale.
        for i in 0..sk.bones.len() {
            sk.validate_local_transform(dm_spine_runtime::data::BoneId(i as u16));
        }

        let mut mismatches = 0usize;
        let mut first_miss: Option<String> = None;
        for (i, expected) in fx.bones.iter().enumerate() {
            assert_eq!(
                expected.index as usize, i,
                "[{rig_label}] fixture bone order broken at index {i}"
            );
            if let Some(msg) = first_bone_mismatch(rig_label, expected, sk.bones[i].applied()) {
                mismatches += 1;
                if first_miss.is_none() {
                    first_miss = Some(msg);
                }
            }
        }
        if mismatches == 0 {
            checked += 1;
        } else {
            eprintln!(
                "  {} miss ({} bones): {}",
                rig_label,
                mismatches,
                first_miss.unwrap()
            );
        }
    }
    eprintln!(
        "\ngolden_pose: {} fixtures match, {} diverge",
        checked,
        fixtures.len() - checked
    );

    assert_eq!(checked, fixtures.len(), "every captured rig must match");
}
