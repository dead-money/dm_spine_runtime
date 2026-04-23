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

//! Multi-track integration smoke tests. Loads a real example rig and
//! drives `AnimationState` through crossfades, queued animations, and
//! empty-animation fades. Asserts no panics and that lifecycle events
//! fire in the expected order; bit-for-bit value correctness sits in
//! `tests/golden_animation.rs` via spine-cpp-captured fixtures.

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::animation::{AnimationState, AnimationStateData, EventType};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};

fn load_spineboy_pro() -> Arc<dm_spine_runtime::data::SkeletonData> {
    let examples = PathBuf::from("../spine-runtimes/examples/spineboy/export");
    let atlas_src = std::fs::read_to_string(examples.join("spineboy.atlas")).unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(examples.join("spineboy-pro.skel")).unwrap();
    Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    )
}

fn find_anim(
    data: &dm_spine_runtime::data::SkeletonData,
    name: &str,
) -> dm_spine_runtime::data::AnimationId {
    dm_spine_runtime::data::AnimationId(
        data.animations
            .iter()
            .position(|a| a.name == name)
            .unwrap_or_else(|| panic!("no animation `{name}` in spineboy-pro")) as u16,
    )
}

#[test]
fn crossfade_walk_to_run_fires_start_interrupt_end() {
    let data = load_spineboy_pro();
    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();

    let mut state_data = AnimationStateData::new(Arc::clone(&data));
    state_data.set_default_mix(0.2);
    let state_data = Arc::new(state_data);
    let mut state = AnimationState::new(state_data);

    let walk = find_anim(&data, "walk");
    let run = find_anim(&data, "run");

    state.set_animation(0, walk, true);
    assert!(
        state
            .drain_events()
            .iter()
            .any(|e| e.kind == EventType::Start),
        "walk should emit Start"
    );

    // Run walk for a while so it's applied, then switch to run mid-cycle.
    let mut events = Vec::new();
    for _ in 0..10 {
        state.update(0.033);
        state.apply(&mut sk, &mut events);
    }
    // Drain any events walk may have queued (Complete if it looped, etc.).
    let _ = state.drain_events();

    state.set_animation(0, run, true);
    let kinds: Vec<EventType> = state.drain_events().iter().map(|e| e.kind).collect();
    assert!(
        kinds.contains(&EventType::Interrupt),
        "walk entry should Interrupt on set_animation, got {kinds:?}"
    );
    assert!(
        kinds.contains(&EventType::Start),
        "run should Start on set_animation, got {kinds:?}"
    );

    // Drive the crossfade until the mix completes (≥ 0.2s + some extra
    // for the deferred clear) and the walk's End event fires.
    let mut saw_end = false;
    for _ in 0..30 {
        state.update(0.033);
        state.apply(&mut sk, &mut events);
        if state
            .drain_events()
            .iter()
            .any(|e| e.kind == EventType::End)
        {
            saw_end = true;
            break;
        }
    }
    assert!(
        saw_end,
        "mixing-from walk entry should fire End once mixing completes"
    );

    // And every bone transform must still be finite.
    sk.update_world_transform(Physics::None);
    for bone in &sk.bones {
        assert!(
            bone.a.is_finite()
                && bone.b.is_finite()
                && bone.c.is_finite()
                && bone.d.is_finite()
                && bone.world_x.is_finite()
                && bone.world_y.is_finite(),
            "non-finite bone after crossfade"
        );
    }
}

#[test]
fn queued_animation_promotes_then_fires_start() {
    let data = load_spineboy_pro();
    let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
    let mut state = AnimationState::new(state_data);

    let walk = find_anim(&data, "walk");
    let idle = find_anim(&data, "idle");

    state.set_animation(0, walk, false);
    state.add_animation(0, idle, true, 0.5); // queued 0.5s after walk starts

    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    let mut events = Vec::new();

    let mut promoted = false;
    for _ in 0..30 {
        state.update(0.05);
        state.apply(&mut sk, &mut events);
        if state.current(0).map(|e| e.animation) == Some(idle) {
            promoted = true;
            break;
        }
    }
    assert!(promoted, "queued idle should promote after 0.5s delay");
}

#[test]
fn set_empty_animations_fades_all_tracks_to_setup() {
    let data = load_spineboy_pro();
    let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
    let mut state = AnimationState::new(state_data);

    let walk = find_anim(&data, "walk");
    state.set_animation(0, walk, true);

    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    let mut events = Vec::new();
    state.update(0.1);
    state.apply(&mut sk, &mut events);

    state.set_empty_animations(0.15);
    // After fade + a few update ticks, the track should be cleared.
    let mut cleared = false;
    for _ in 0..10 {
        state.update(0.05);
        state.apply(&mut sk, &mut events);
        if state.current(0).is_none() {
            cleared = true;
            break;
        }
    }
    assert!(
        cleared,
        "set_empty_animations should clear all tracks within mix_duration + a tick"
    );
}
