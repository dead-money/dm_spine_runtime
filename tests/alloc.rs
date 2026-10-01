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

//! Steady-state frames must not allocate: update, apply, world transform and
//! render run with reused buffers once warmed up.

// A counting global allocator needs `unsafe`; the library itself has none.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use spine_runtime::animation::{AnimationState, AnimationStateData};
use spine_runtime::atlas::Atlas;
use spine_runtime::data::SlotId;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::render::{RenderOptions, SkeletonRenderer};
use spine_runtime::skeleton::{Physics, RegionGeometry, Skeleton, SkeletonBounds};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations_per_loop(rig: &str, skel: &str, anim: &str, physics: Physics) -> usize {
    let dir = common::examples_root().join(rig).join("export");
    let atlas =
        Atlas::parse(&std::fs::read_to_string(dir.join(format!("{rig}.atlas"))).unwrap()).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(dir.join(format!("{skel}.skel"))).unwrap();
    let data = Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    );
    let mut skeleton = Skeleton::new(Arc::clone(&data));
    let mut state = AnimationState::new(Arc::new(AnimationStateData::new(Arc::clone(&data))));
    state.set_animation_by_name(0, anim, true).unwrap();
    let mut renderer = SkeletonRenderer::new();
    let mut merged_renderer = SkeletonRenderer::with_options(RenderOptions {
        vertex_ids: true,
        merge_colors: true,
    });
    let mut bounds = SkeletonBounds::new();
    let mut geometry = RegionGeometry::new();
    let mut events = Vec::with_capacity(64);
    let mut state_events = Vec::with_capacity(64);
    let duration = data
        .animations
        .iter()
        .find(|a| a.name == anim)
        .unwrap()
        .duration;
    let frames = (duration * 60.0).ceil() as usize;
    let mut run = |n: usize| {
        for _ in 0..n {
            state.update(1.0 / 60.0);
            events.clear();
            state.apply(&mut skeleton, &mut events);
            skeleton.update(1.0 / 60.0);
            skeleton.update_world_transform(physics);
            renderer.render(&skeleton);
            merged_renderer.render(&skeleton);
            bounds.update(&skeleton, true);
            for slot in 0..skeleton.slots.len() {
                if geometry.update(&skeleton, SlotId(slot as u16)) {
                    let _ = geometry.map(0.5, 0.5);
                }
            }
            state_events.clear();
            state.drain_events_into(&mut state_events);
        }
    };
    // Warm up until every buffer has reached its peak size; the loop's phase
    // drifts against the 60 Hz step, so give it several loops.
    run(frames * 6);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    run(frames);
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

#[test]
fn steady_state_frames_do_not_allocate() {
    for (rig, skel, anim, physics) in [
        ("spineboy", "spineboy-pro", "walk", Physics::None),
        ("raptor", "raptor-pro", "walk", Physics::None),
        ("stretchyman", "stretchyman-pro", "sneak", Physics::None),
        ("diamond", "diamond-pro", "rotation", Physics::None),
        (
            "celestial-circus",
            "celestial-circus-pro",
            "swing",
            Physics::Update,
        ),
        ("coin", "coin-pro", "animation", Physics::None),
    ] {
        let n = allocations_per_loop(rig, skel, anim, physics);
        assert_eq!(
            n, 0,
            "{rig}/{anim}: {n} allocations in one loop of steady-state frames"
        );
    }
}
