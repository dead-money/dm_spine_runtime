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

//! Diffs Rust-computed animation samples against spine-cpp fixtures from
//! `tools/spine_capture/capture_animations.sh`. The capture harness applies
//! each animation at a specific time using `Animation::apply` + bones-only
//! `Bone::updateWorldTransform` in spine-cpp; the Rust side does the same
//! through `AnimationState` + `Skeleton::update_world_transform`.
//!
//! Phase 3 stubs constraint solvers, so any animation that would rely on an
//! IK or transform constraint running at evaluation time will diverge.
//! The fixtures don't exercise those paths yet.

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

const TOLERANCE: f32 = 1e-3; // animations integrate accumulated trig, 1e-3 is spine-cpp convention

#[derive(Debug, Deserialize)]
struct Fixture {
    animation: String,
    time: f32,
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
    PathBuf::from("tests/fixtures/animations")
}

fn examples_root() -> PathBuf {
    PathBuf::from("../spine-runtimes/examples")
}

/// `(rig, variant, animation, [sample_path, …])` — one entry per
/// animation, with all time-sample paths grouped together.
fn collect_fixture_samples() -> Vec<(String, String, String, Vec<PathBuf>)> {
    let mut out = Vec::new();
    let root = fixtures_root();
    if !root.is_dir() {
        return out;
    }
    for rig_entry in std::fs::read_dir(&root).unwrap().flatten() {
        let rig_dir = rig_entry.path();
        if !rig_dir.is_dir() {
            continue;
        }
        // Dir name is "rig-variant".
        let rigvar = rig_dir.file_name().unwrap().to_string_lossy().into_owned();
        let Some((rig, variant)) = rigvar.split_once('-') else {
            panic!("fixture dir not in rig-variant form: {rigvar}")
        };
        let (rig, variant) = (rig.to_string(), variant.to_string());

        for anim_entry in std::fs::read_dir(&rig_dir).unwrap().flatten() {
            let anim_dir = anim_entry.path();
            if !anim_dir.is_dir() {
                continue;
            }
            let anim_name = anim_dir.file_name().unwrap().to_string_lossy().into_owned();
            let mut samples: Vec<PathBuf> = std::fs::read_dir(&anim_dir)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
                .collect();
            samples.sort();
            if !samples.is_empty() {
                out.push((rig.clone(), variant.clone(), anim_name, samples));
            }
        }
    }
    out.sort();
    out
}

fn load_skeleton(rig: &str, variant: &str) -> Arc<dm_spine_runtime::data::SkeletonData> {
    let export = examples_root().join(rig).join("export");
    let atlas_path = export.join(format!("{rig}.atlas"));
    let skel_path = export.join(format!("{rig}-{variant}.skel"));
    let atlas_src = std::fs::read_to_string(&atlas_path).unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(&skel_path).unwrap();
    Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    )
}

fn close(a: f32, b: f32) -> bool {
    let diff = (a - b).abs();
    diff <= TOLERANCE || diff <= TOLERANCE * a.abs().max(b.abs())
}

fn check_bone(label: &str, expected: &BoneFixture, actual: &dm_spine_runtime::skeleton::Bone) {
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
    for (name, want, got) in fields {
        assert!(
            close(want, got),
            "[{label}] bone #{} ({}) field `{name}` mismatch: \
             expected {want} got {got} (diff {})",
            expected.index,
            expected.name,
            (want - got).abs(),
        );
    }
}

#[test]
fn animation_samples_match_spine_cpp() {
    let groups = collect_fixture_samples();
    assert!(
        !groups.is_empty(),
        "no animation fixtures found; run tools/spine_capture/capture_animations.sh"
    );

    let mut checked = 0usize;
    for (rig, variant, anim_name, samples) in &groups {
        let data = load_skeleton(rig, variant);
        let anim_id = match data.animations.iter().position(|a| a.name == *anim_name) {
            Some(i) => dm_spine_runtime::data::AnimationId(i as u16),
            None => panic!("no animation `{anim_name}` in {rig}-{variant}"),
        };

        for fx_path in samples {
            let fx: Fixture =
                serde_json::from_str(&std::fs::read_to_string(fx_path).unwrap()).unwrap();
            assert_eq!(fx.animation, *anim_name);

            let mut sk = Skeleton::new(Arc::clone(&data));
            sk.update_cache();
            sk.set_to_setup_pose();

            let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
            let mut state = AnimationState::new(state_data);
            state.set_animation(0, anim_id, false);
            // Jump to `fx.time` in one step (update advances by delta).
            state.update(fx.time);

            let mut events = Vec::new();
            state.apply(&mut sk, &mut events);
            sk.update_world_transform(Physics::None);

            let label = format!("{rig}-{variant}/{anim_name}@{:.4}s", fx.time);
            assert_eq!(
                sk.bones.len(),
                fx.bones.len(),
                "[{label}] bone count mismatch"
            );
            for (i, expected) in fx.bones.iter().enumerate() {
                assert_eq!(expected.index as usize, i);
                check_bone(&label, expected, &sk.bones[i]);
            }
            checked += 1;
        }
    }

    assert!(checked >= 20, "only checked {checked} samples");
}
