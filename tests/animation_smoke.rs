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

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use spine_runtime::animation::{AnimationState, AnimationStateData};
use spine_runtime::atlas::Atlas;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::skeleton::{Physics, Skeleton};

fn examples_dir() -> PathBuf {
    common::examples_root()
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
                let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
                let mut state = AnimationState::new(state_data);
                let _ =
                    state.set_animation(0, spine_runtime::data::AnimationId(anim_idx as u16), true);

                for t in [0.0_f32, 0.1, 0.333, 0.666, 0.999].iter().copied() {
                    let time = if duration > 0.0 { t * duration } else { 0.0 };
                    let entry = state.track(0).unwrap();
                    state.update(time - state.entry(entry).unwrap().track_time);
                    sk.setup_pose();
                    let mut events = Vec::new();
                    state.apply(&mut sk, &mut events);
                    sk.update_world_transform(Physics::None);

                    // Light sanity checks: bone world matrices are finite.
                    for (i, bone) in sk.bones.iter().enumerate() {
                        let bone = bone.applied();
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

    eprintln!("exercised {animations_exercised} animations on {rigs_seen} rigs");
    assert!(rigs_seen >= 15, "exercised only {rigs_seen} rigs");
    assert!(
        animations_exercised >= 30,
        "exercised only {animations_exercised} animations"
    );
}

/// What a save file keeps per queued entry on a track.
struct SavedEntry {
    animation: Option<String>,
    looping: bool,
    track_time: f32,
    time_scale: f32,
    mix_duration: f32,
    delay: f32,
}

/// A track's queue saved while no crossfade is in progress and restored
/// into a fresh state plays on exactly as the original does.
#[test]
fn saved_track_queue_restores_playback() {
    let dir = examples_dir().join("spineboy/export");
    let atlas =
        Atlas::parse(&std::fs::read_to_string(dir.join("spineboy.atlas")).unwrap()).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let data = Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&std::fs::read(dir.join("spineboy-pro.skel")).unwrap())
            .unwrap(),
    );
    let mut state_data = AnimationStateData::new(Arc::clone(&data));
    state_data.set_default_mix(0.2);
    let state_data = Arc::new(state_data);

    let mut skeleton = Skeleton::new(Arc::clone(&data));
    let mut state = AnimationState::new(Arc::clone(&state_data));
    let walk = state.set_animation_by_name(0, "walk", true).unwrap();
    state.entry_mut(walk).unwrap().time_scale = 1.5;
    state.add_animation_by_name(0, "run", true, 1.2).unwrap();
    state.add_empty_animation(0, 0.3, 0.8);
    let mut events = Vec::new();
    let step = |state: &mut AnimationState, skeleton: &mut Skeleton, events: &mut Vec<_>| {
        state.update(1.0 / 30.0);
        state.apply(skeleton, events);
        skeleton.update_world_transform(Physics::Update);
    };
    for _ in 0..12 {
        step(&mut state, &mut skeleton, &mut events);
    }

    let mut saved = Vec::new();
    let mut next = state.track(0);
    while let Some(id) = next {
        let e = state.entry(id).unwrap();
        assert!(e.mixing_from.is_none());
        saved.push(SavedEntry {
            animation: (!e.is_empty_animation())
                .then(|| data.animations[e.animation.index()].name.clone()),
            looping: e.looping,
            track_time: e.track_time,
            time_scale: e.time_scale,
            mix_duration: e.mix_duration,
            delay: e.delay,
        });
        next = e.next;
    }
    assert_eq!(saved.len(), 3);

    let mut restored = AnimationState::new(Arc::clone(&state_data));
    for (i, s) in saved.iter().enumerate() {
        let id = match (&s.animation, i) {
            (Some(name), 0) => restored.set_animation_by_name(0, name, s.looping).unwrap(),
            (None, 0) => restored.set_empty_animation(0, s.mix_duration),
            (Some(name), _) => restored
                .add_animation_by_name(0, name, s.looping, s.delay)
                .unwrap(),
            (None, _) => restored.add_empty_animation(0, s.mix_duration, s.delay),
        };
        let e = restored.entry_mut(id).unwrap();
        if i == 0 {
            e.track_time = s.track_time;
            let time = e.animation_time();
            e.set_animation_last(time);
        }
        e.time_scale = s.time_scale;
        e.mix_duration = s.mix_duration;
    }

    let mut restored_skeleton = skeleton.clone();
    for frame in 0..90 {
        step(&mut state, &mut skeleton, &mut events);
        step(&mut restored, &mut restored_skeleton, &mut events);
        for (a, b) in skeleton.bones.iter().zip(&restored_skeleton.bones) {
            let (a, b) = (a.applied(), b.applied());
            assert_eq!(
                (a.world_x, a.world_y, a.a, a.b, a.c, a.d),
                (b.world_x, b.world_y, b.a, b.b, b.c, b.d),
                "frame {frame}"
            );
        }
        for (a, b) in skeleton.slots.iter().zip(&restored_skeleton.slots) {
            assert_eq!(a.applied(), b.applied(), "frame {frame}");
        }
    }
}
