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

//! Scripted `AnimationState` scenarios diffed against spine-cpp captures
//! (`tools/spine_capture/capture_state.sh`): bone poses, slot attachments
//! and colors, draw order, and the lifecycle event sequence.

mod common;

use std::sync::Arc;

use dm_spine_runtime::animation::{
    AnimationState, AnimationStateData, EMPTY_ANIMATION_ID, EventType, Interpolation,
};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::{BoneId, SkeletonData};
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

const TOLERANCE: f32 = 1e-3;

#[derive(Deserialize)]
struct Fixture {
    source_skel: String,
    source_atlas: String,
    script: String,
    frames: Vec<Frame>,
    events: Vec<String>,
}

#[derive(Deserialize)]
struct Frame {
    bones: Vec<BoneFixture>,
    slots: Vec<SlotFixture>,
    draw_order: Vec<u16>,
}

#[derive(Deserialize)]
struct BoneFixture {
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
}

#[derive(Deserialize)]
struct SlotFixture {
    attachment: Option<String>,
    color: [f32; 4],
    sequence_index: i32,
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOLERANCE || (a - b).abs() <= TOLERANCE * a.abs().max(b.abs())
}

fn load(fx: &Fixture) -> Arc<SkeletonData> {
    let atlas =
        Atlas::parse(&std::fs::read_to_string(common::example_path(&fx.source_atlas)).unwrap())
            .unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(common::example_path(&fx.source_skel)).unwrap();
    Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    )
}

/// Runs the script, returning `(frames, events)` in the fixture's shape.
fn run(data: &Arc<SkeletonData>, script: &str) -> (Vec<Skeleton>, Vec<String>) {
    let mut skeleton = Skeleton::new(Arc::clone(data));
    skeleton.setup_pose();
    let mut state_data = AnimationStateData::new(Arc::clone(data));
    let mut state: Option<AnimationState> = None;
    let mut frames = Vec::new();
    let mut events = Vec::new();
    let mut keyframes = Vec::new();
    let anim_name = |id: dm_spine_runtime::data::AnimationId| {
        if id == EMPTY_ANIMATION_ID {
            "<empty>".to_string()
        } else {
            data.animations[id.index()].name.clone()
        }
    };
    for cmd in script.split(';').filter(|c| !c.is_empty()) {
        let f: Vec<&str> = cmd.split(':').collect();
        if f[0] == "mix" {
            state_data.set_default_mix(f[1].parse().unwrap());
            continue;
        }
        let state = state.get_or_insert_with(|| AnimationState::new(Arc::new(state_data.clone())));
        let track = || f[1].parse::<usize>().unwrap();
        match f[0] {
            "set" => {
                state
                    .set_animation_by_name(track(), f[2], f[3] == "1")
                    .unwrap();
            }
            "add" => {
                state
                    .add_animation_by_name(track(), f[2], f[3] == "1", f[4].parse().unwrap())
                    .unwrap();
            }
            "empty" => {
                state.set_empty_animation(track(), f[2].parse().unwrap());
            }
            "addempty" => {
                state.add_empty_animation(track(), f[2].parse().unwrap(), f[3].parse().unwrap());
            }
            "additive" | "alpha" | "interp" => {
                let id = state.track(track()).unwrap();
                let e = state.entry_mut(id).unwrap();
                match f[0] {
                    "additive" => e.additive = f[2] == "1",
                    "alpha" => e.alpha = f[2].parse().unwrap(),
                    _ => {
                        e.mix_interpolation = match f[2] {
                            "smooth" => Interpolation::Smooth,
                            "slowFast" => Interpolation::SlowFast,
                            "fastSlow" => Interpolation::FastSlow,
                            "circle" => Interpolation::Circle,
                            _ => Interpolation::Linear,
                        }
                    }
                }
            }
            "step" => {
                for _ in 0..f[1].parse::<usize>().unwrap() {
                    state.update(1.0 / 60.0);
                    state.apply(&mut skeleton, &mut keyframes);
                    skeleton.update_world_transform(Physics::None);
                }
            }
            "dump" => {
                let mut snap = skeleton.clone();
                for i in 0..snap.bones.len() {
                    snap.validate_local_transform(BoneId(i as u16));
                }
                frames.push(snap);
            }
            other => panic!("unknown command {other}"),
        }
        for e in state.drain_events() {
            let kind = match e.kind {
                EventType::Start => "start",
                EventType::Interrupt => "interrupt",
                EventType::End => "end",
                EventType::Complete => "complete",
                EventType::Dispose => "dispose",
                EventType::Event => "event",
            };
            let mut s = format!("{kind}:{}:{}", e.track_index, anim_name(e.animation));
            if let Some(ev) = e.event {
                s.push(':');
                s.push_str(&data.events[ev.data.index()].name);
            }
            events.push(s);
        }
    }
    (frames, events)
}

fn check_frame(label: &str, sk: &Skeleton, fx: &Frame) -> Option<String> {
    let data = sk.data();
    for (i, (bone, want)) in sk.bones.iter().zip(&fx.bones).enumerate() {
        let p = bone.applied();
        let fields = [
            ("a", want.a, p.a),
            ("b", want.b, p.b),
            ("c", want.c, p.c),
            ("d", want.d, p.d),
            ("world_x", want.world_x, p.world_x),
            ("world_y", want.world_y, p.world_y),
            ("ax", want.ax, p.x),
            ("ay", want.ay, p.y),
            ("a_rotation", want.a_rotation, p.rotation),
            ("a_scale_x", want.a_scale_x, p.scale_x),
            ("a_scale_y", want.a_scale_y, p.scale_y),
            ("a_shear_x", want.a_shear_x, p.shear_x),
            ("a_shear_y", want.a_shear_y, p.shear_y),
        ];
        for (field, w, g) in fields {
            if !close(w, g) {
                return Some(format!(
                    "{label}: bone {i} ({}) .{field}: want {w} got {g}",
                    want.name
                ));
            }
        }
    }
    for (i, (slot, want)) in sk.slots.iter().zip(&fx.slots).enumerate() {
        let pose = slot.applied();
        let name = pose
            .attachment
            .map(|a| data.attachments[a.index()].name().to_string());
        if name != want.attachment {
            return Some(format!(
                "{label}: slot {i} ({}) attachment: want {:?} got {name:?}",
                data.slots[i].name, want.attachment
            ));
        }
        let c = pose.color;
        if !([c.r, c.g, c.b, c.a]
            .iter()
            .zip(&want.color)
            .all(|(g, w)| close(*w, *g)))
        {
            return Some(format!(
                "{label}: slot {i} color: want {:?} got {c:?}",
                want.color
            ));
        }
        if pose.sequence_index != want.sequence_index {
            return Some(format!(
                "{label}: slot {i} sequence index: want {} got {}",
                want.sequence_index, pose.sequence_index
            ));
        }
    }
    let order: Vec<u16> = sk.draw_order.applied().iter().map(|s| s.0).collect();
    if order != fx.draw_order {
        return Some(format!("{label}: draw order differs"));
    }
    None
}

#[test]
fn animation_state_scenarios_match_spine_cpp() {
    let root = std::path::Path::new("tests/fixtures/state");
    let files = common::json_files(root);
    assert!(!files.is_empty(), "no state fixtures; run capture_state.sh");
    let mut failures = Vec::new();
    for path in &files {
        let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let label = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data = load(&fx);
        let (frames, events) = run(&data, &fx.script);
        if events != fx.events {
            failures.push(format!(
                "{label}: events\n    want {:?}\n    got  {events:?}",
                fx.events
            ));
        }
        assert_eq!(frames.len(), fx.frames.len(), "{label}: dump count");
        for (i, (sk, want)) in frames.iter().zip(&fx.frames).enumerate() {
            if let Some(msg) = check_frame(&format!("{label}#{i}"), sk, want) {
                failures.push(msg);
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
