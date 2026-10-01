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

use serde::Deserialize;
use spine_runtime::animation::{
    AnimationState, AnimationStateData, EMPTY_ANIMATION_ID, EventType, Interpolation,
};
use spine_runtime::atlas::Atlas;
use spine_runtime::data::{BoneId, SkeletonData};
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::skeleton::{Physics, Skeleton};

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
    load_rig(&fx.source_skel, &fx.source_atlas)
}

fn load_rig(skel: &str, atlas: &str) -> Arc<SkeletonData> {
    let atlas =
        Atlas::parse(&std::fs::read_to_string(common::example_path(atlas)).unwrap()).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(common::example_path(skel)).unwrap();
    Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    )
}

/// Every live entry's fields, reached from the tracks.
fn trace_entries(state: &AnimationState, trace: &mut Vec<String>) {
    for &track in state.tracks() {
        let mut from = track;
        while let Some(id) = from {
            let e = state.entry(id).unwrap();
            trace.push(format!("{id:?} {e:?}"));
            from = e.mixing_from;
        }
        let mut next = track.and_then(|id| state.entry(id).unwrap().next);
        while let Some(id) = next {
            let e = state.entry(id).unwrap();
            trace.push(format!("{id:?} {e:?}"));
            next = e.next;
        }
    }
}

/// How [`run`] applies each step.
struct Mode {
    /// Runs [`AnimationState::pose`] between every update, apply and command.
    pose: bool,
    /// Starts every full apply from a fresh skeleton in its setup pose, so
    /// the pose doesn't depend on which steps were full.
    reset: bool,
    /// Whether the step with this index, counted across the script, is a full
    /// apply. The last step before each command is always full.
    full: fn(usize) -> bool,
    /// Non-full steps call [`AnimationState::apply_events`] if set; otherwise
    /// they apply and then reset every entry's rotation directions.
    events_only: bool,
}

const FULL: Mode = Mode {
    pose: false,
    reset: false,
    full: |_| true,
    events_only: false,
};

fn reset_rotation_directions(state: &mut AnimationState) {
    for track in state.tracks().to_vec() {
        let mut from = track;
        while let Some(id) = from {
            let e = state.entry_mut(id).unwrap();
            e.reset_rotation_directions();
            from = e.mixing_from;
        }
    }
}

/// Runs the script, returning the skeleton at each `dump`, the lifecycle
/// events in the fixture's string form, and a trace of every drained event,
/// keyframe event and entry state.
fn run(
    data: &Arc<SkeletonData>,
    script: &str,
    mode: &Mode,
) -> (Vec<Skeleton>, Vec<String>, Vec<String>) {
    let pose = mode.pose;
    let mut skeleton = Skeleton::new(Arc::clone(data));
    skeleton.setup_pose();
    let fresh = skeleton.clone();
    let mut state_data = AnimationStateData::new(Arc::clone(data));
    let mut state: Option<AnimationState> = None;
    let mut frames = Vec::new();
    let mut events = Vec::new();
    let mut keyframes = Vec::new();
    let mut trace = Vec::new();
    let mut step = 0;
    let anim_name = |id: spine_runtime::data::AnimationId| {
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
            "additive" | "alpha" | "interp" | "reverse" | "threshold" | "shortest" => {
                let id = state.track(track()).unwrap();
                let e = state.entry_mut(id).unwrap();
                match f[0] {
                    "additive" => e.additive = f[2] == "1",
                    "alpha" => e.alpha = f[2].parse().unwrap(),
                    "reverse" => e.reverse = f[2] == "1",
                    "threshold" => e.event_threshold = f[2].parse().unwrap(),
                    "shortest" => e.shortest_rotation = f[2] == "1",
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
                let count = f[1].parse::<usize>().unwrap();
                for i in 0..count {
                    state.update(1.0 / 60.0);
                    if pose {
                        state.pose(&mut skeleton);
                    }
                    if i + 1 == count || (mode.full)(step) {
                        if mode.reset {
                            skeleton.clone_from(&fresh);
                        }
                        state.apply(&mut skeleton, &mut keyframes);
                    } else if mode.events_only {
                        state.apply_events(&mut keyframes);
                    } else {
                        state.apply(&mut skeleton, &mut keyframes);
                        reset_rotation_directions(state);
                    }
                    step += 1;
                    if pose {
                        state.pose(&mut skeleton);
                        state.pose(&mut skeleton);
                    }
                    skeleton.update_world_transform(Physics::None);
                    trace.extend(keyframes.drain(..).map(|k| format!("{k:?}")));
                    trace_entries(state, &mut trace);
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
        if pose {
            state.pose(&mut skeleton);
        }
        trace_entries(state, &mut trace);
        for e in state.drain_events() {
            trace.push(format!("{e:?}"));
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
    (frames, events, trace)
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
        let name = pose.attachment.map(|a| sk.attachment(a).name().to_string());
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
        let (frames, events, _) = run(&data, &fx.script, &FULL);
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

#[test]
fn pose_leaves_events_and_entries_unchanged() {
    let files = common::json_files(std::path::Path::new("tests/fixtures/state"));
    assert!(!files.is_empty(), "no state fixtures; run capture_state.sh");
    for path in &files {
        let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let label = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data = load(&fx);
        let (_, events, trace) = run(&data, &fx.script, &FULL);
        let (frames, posed_events, posed_trace) =
            run(&data, &fx.script, &Mode { pose: true, ..FULL });
        assert_eq!(events, posed_events, "{label}: events");
        assert_eq!(trace.len(), posed_trace.len(), "{label}: trace length");
        for (i, (want, got)) in trace.iter().zip(&posed_trace).enumerate() {
            assert_eq!(want, got, "{label}: trace #{i}");
        }
        for (i, (sk, want)) in frames.iter().zip(&fx.frames).enumerate() {
            if let Some(msg) = check_frame(&format!("{label}#{i}"), sk, want) {
                panic!("{msg}");
            }
        }
    }
}

const SPINEBOY: (&str, &str) = (
    "spineboy/export/spineboy-pro.skel",
    "spineboy/export/spineboy.atlas",
);

const RAPTOR: (&str, &str) = (
    "raptor/export/raptor-pro.skel",
    "raptor/export/raptor.atlas",
);

/// Event-heavy scripts beyond the fixtures', checked only against full
/// applies of themselves.
const EVENT_SCRIPTS: &[((&str, &str), &str)] = &[
    (SPINEBOY, "set:0:walk:1;step:90;dump;step:47;dump"),
    (
        SPINEBOY,
        "mix:0.4;set:0:walk:1;threshold:0:0.5;step:25;set:0:run:1;threshold:0:1;step:30;dump;\
         set:0:walk:1;step:40;dump",
    ),
    (
        SPINEBOY,
        "mix:0.2;set:0:run:1;add:0:jump:0:0.5;add:0:walk:1:0;add:0:idle:1:1.5;step:200;dump",
    ),
    (
        SPINEBOY,
        "add:0:walk:1:0.25;step:30;addempty:0:0.3:0.5;add:0:run:1:0.2;step:80;dump;\
         empty:0:0;step:5;dump",
    ),
    (
        SPINEBOY,
        "mix:0.3;set:0:walk:1;reverse:0:1;threshold:0:1;step:50;set:0:run:1;reverse:0:1;step:40;\
         dump;set:0:jump:0;step:70;dump",
    ),
    (
        SPINEBOY,
        "set:0:idle:1;set:1:walk:1;alpha:1:0.6;step:70;dump;empty:1:0.4;step:30;dump",
    ),
    (
        SPINEBOY,
        "mix:0.5;set:0:walk:1;shortest:0:1;step:20;set:0:death:0;step:40;dump;set:0:run:1;\
         step:40;dump",
    ),
    (
        RAPTOR,
        "mix:0.4;set:0:roar:1;set:1:jump:1;step:20;set:1:walk:1;step:30;dump;step:30;dump",
    ),
];

/// Full applies on these steps, `apply_events` on the rest.
const PATTERNS: &[fn(usize) -> bool] = &[
    |s| s % 2 == 0,
    |s| s % 2 == 1,
    |s| s % 5 != 2,
    |s| s % 7 == 3,
    |_| false,
];

fn pose_diff(want: &Skeleton, got: &Skeleton) -> Option<String> {
    for (i, (w, g)) in want.bones.iter().zip(&got.bones).enumerate() {
        if w.applied() != g.applied() {
            return Some(format!(
                "bone {i}: want {:?} got {:?}",
                w.applied(),
                g.applied()
            ));
        }
    }
    for (i, (w, g)) in want.slots.iter().zip(&got.slots).enumerate() {
        if w.applied() != g.applied() {
            return Some(format!(
                "slot {i}: want {:?} got {:?}",
                w.applied(),
                g.applied()
            ));
        }
    }
    if want.draw_order.applied() != got.draw_order.applied() {
        return Some("draw order differs".to_string());
    }
    let (w, g) = (
        format!("{:?}", want.constraints),
        format!("{:?}", got.constraints),
    );
    (w != g).then(|| format!("constraints: want {w} got {g}"))
}

/// Interleaving `apply_events` with full applies must give the same events,
/// entries and poses as full applies alone, given the rotation directions
/// `apply_events` documents resetting.
#[test]
fn apply_events_matches_full_apply() {
    let mut cases = Vec::new();
    for path in common::json_files(std::path::Path::new("tests/fixtures/state")) {
        let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let label = path.file_stem().unwrap().to_string_lossy().into_owned();
        cases.push((label, load(&fx), fx.script));
    }
    for (i, ((skel, atlas), script)) in EVENT_SCRIPTS.iter().enumerate() {
        cases.push((
            format!("events-{i}"),
            load_rig(skel, atlas),
            (*script).to_string(),
        ));
    }
    assert!(cases.len() > EVENT_SCRIPTS.len(), "no state fixtures");

    for (label, data, script) in &cases {
        for (p, &full) in PATTERNS.iter().enumerate() {
            let label = format!("{label} pattern {p}");
            let reference = Mode {
                reset: true,
                full,
                ..FULL
            };
            let (frames, events, trace) = run(data, script, &reference);
            let mode = Mode {
                events_only: true,
                ..reference
            };
            let (got_frames, got_events, got_trace) = run(data, script, &mode);
            assert_eq!(events, got_events, "{label}: events");
            assert_eq!(trace.len(), got_trace.len(), "{label}: trace length");
            for (i, (want, got)) in trace.iter().zip(&got_trace).enumerate() {
                assert_eq!(want, got, "{label}: trace #{i}");
            }
            for (i, (want, got)) in frames.iter().zip(&got_frames).enumerate() {
                if let Some(msg) = pose_diff(want, got) {
                    panic!("{label}: dump #{i}: {msg}");
                }
            }
        }
    }
}
