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

//! Smoke-test: for every example rig and every animation in it, apply the
//! animation at several time points via `AnimationState` + the full pose
//! pipeline, and check nothing panics (array-index OOB, division by zero,
//! NaN propagation, etc.). Value correctness is Phase 3f's golden-diff
//! test — this one just keeps regressions to the integration plumbing
//! visible.

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::animation::AnimationState;
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};

fn examples_dir() -> PathBuf {
    PathBuf::from("../spine-runtimes/examples")
}

#[test]
fn all_animations_apply_without_panic() {
    let root = examples_dir();
    assert!(root.exists(), "missing examples dir: {}", root.display());

    let mut rigs_seen = 0usize;
    let mut animations_exercised = 0usize;

    for rig_entry in std::fs::read_dir(&root).unwrap().flatten() {
        let export = rig_entry.path().join("export");
        if !export.is_dir() {
            continue;
        }
        let rig = rig_entry.file_name().to_string_lossy().into_owned();

        // Pair each .skel with the matching non-PMA .atlas.
        let atlas_path = export.join(format!("{rig}.atlas"));
        if !atlas_path.exists() {
            continue;
        }
        let Ok(atlas_src) = std::fs::read_to_string(&atlas_path) else {
            continue;
        };
        let Ok(atlas) = Atlas::parse(&atlas_src) else {
            continue;
        };

        let mut exercised_here = false;
        for skel_entry in std::fs::read_dir(&export).unwrap().flatten() {
            let skel_path = skel_entry.path();
            if skel_path.extension().and_then(|s| s.to_str()) != Some("skel") {
                continue;
            }
            let bytes = std::fs::read(&skel_path).unwrap();
            let mut loader = AtlasAttachmentLoader::new(&atlas);
            let Ok(data) = SkeletonBinary::with_loader(&mut loader).read(&bytes) else {
                continue;
            };
            let data = Arc::new(data);

            for anim_idx in 0..data.animations.len() {
                let duration = data.animations[anim_idx].duration;
                let mut sk = Skeleton::new(Arc::clone(&data));
                sk.update_cache();
                let mut state = AnimationState::new(Arc::clone(&data));
                let _ =
                    state.set_animation(dm_spine_runtime::data::AnimationId(anim_idx as u16), true);

                for t in [0.0_f32, 0.1, 0.333, 0.666, 0.999].iter().copied() {
                    let time = if duration > 0.0 { t * duration } else { 0.0 };
                    state.update(time - state.track().unwrap().time);
                    sk.set_to_setup_pose();
                    let mut events = Vec::new();
                    state.apply(&mut sk, &mut events);
                    sk.update_world_transform(Physics::None);

                    // Light sanity checks: bone world matrices are finite.
                    for (i, bone) in sk.bones.iter().enumerate() {
                        assert!(
                            bone.a.is_finite()
                                && bone.b.is_finite()
                                && bone.c.is_finite()
                                && bone.d.is_finite()
                                && bone.world_x.is_finite()
                                && bone.world_y.is_finite(),
                            "rig={rig} anim={} bone={i} produced non-finite world transform at t={time}",
                            data.animations[anim_idx].name,
                        );
                    }
                }

                animations_exercised += 1;
                exercised_here = true;
            }
        }

        if exercised_here {
            rigs_seen += 1;
        }
    }

    assert!(rigs_seen >= 15, "exercised only {rigs_seen} rigs");
    assert!(
        animations_exercised >= 30,
        "exercised only {animations_exercised} animations"
    );
}
