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

//! `Timeline::apply` dispatch. One inherent method on [`Timeline`] that
//! matches the variant and delegates to a per-kind free function, mirroring
//! spine-cpp's per-subclass `apply` overrides.

#![allow(clippy::many_single_char_names)] // mirrors spine-cpp variable names

use crate::animation::curve::{curve_value2, relative_value, scale_value, search};
use crate::animation::{Event, MixBlend, MixDirection};
use crate::data::{BoneId, CurveFrames, Inherit, Timeline};
use crate::skeleton::Skeleton;

impl Timeline {
    /// Write this timeline's contribution to `skeleton` for the given time.
    ///
    /// `last_time` is the previous-frame time (only used by discrete event
    /// timelines — Phase 3c). `events` is the out-param for event firings.
    /// `alpha` scales the timeline's influence against the pose baseline
    /// selected by `blend`. `direction` matters for mixing crossfades.
    ///
    /// Ports the dispatch table at `spine::Animation::apply`, which in turn
    /// delegates to each subclass's override. All variants are handled
    /// incrementally as Phase 3 progresses — variants that haven't been
    /// ported yet are a no-op here.
    #[allow(clippy::too_many_arguments)] // matches spine-cpp's apply signature
    pub fn apply(
        &self,
        skeleton: &mut Skeleton,
        last_time: f32,
        time: f32,
        events: &mut Vec<Event>,
        alpha: f32,
        blend: MixBlend,
        direction: MixDirection,
    ) {
        let _ = (last_time, events); // unused until Phase 3c

        match self {
            // --- Bone timelines (Phase 3b) ---
            Timeline::Rotate { bone, curves } => {
                apply_rotate(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::Translate { bone, curves } => {
                apply_translate(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::TranslateX { bone, curves } => {
                apply_translate_x(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::TranslateY { bone, curves } => {
                apply_translate_y(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::Scale { bone, curves } => {
                apply_scale(skeleton, *bone, curves, time, alpha, blend, direction);
            }
            Timeline::ScaleX { bone, curves } => {
                apply_scale_x(skeleton, *bone, curves, time, alpha, blend, direction);
            }
            Timeline::ScaleY { bone, curves } => {
                apply_scale_y(skeleton, *bone, curves, time, alpha, blend, direction);
            }
            Timeline::Shear { bone, curves } => {
                apply_shear(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::ShearX { bone, curves } => {
                apply_shear_x(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::ShearY { bone, curves } => {
                apply_shear_y(skeleton, *bone, curves, time, alpha, blend);
            }
            Timeline::Inherit {
                bone,
                frames,
                inherits,
            } => {
                apply_inherit(skeleton, *bone, frames, inherits, time, blend, direction);
            }

            // --- Other timeline kinds land in Phase 3c / 3d ---
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up the runtime bone. Returns `None` when the bone is inactive
/// (timeline should skip it — matches every spine-cpp bone apply).
fn active_bone(skeleton: &mut Skeleton, bone_id: BoneId) -> Option<usize> {
    let idx = bone_id.index();
    if skeleton.bones[idx].active {
        Some(idx)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Rotate
// ---------------------------------------------------------------------------

fn apply_rotate(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].rotation;
    let bone = &mut skeleton.bones[idx];
    bone.rotation = relative_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        bone.rotation,
        setup,
    );
}

// ---------------------------------------------------------------------------
// Translate (and X/Y splits)
// ---------------------------------------------------------------------------

fn apply_translate(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup_x = skeleton.data.bones[idx].x;
    let setup_y = skeleton.data.bones[idx].y;
    let bone = &mut skeleton.bones[idx];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                bone.x = setup_x;
                bone.y = setup_y;
            }
            MixBlend::First => {
                bone.x += (setup_x - bone.x) * alpha;
                bone.y += (setup_y - bone.y) * alpha;
            }
            _ => {}
        }
        return;
    }

    let (x, y) = curve_value2(&curves.frames, &curves.curves, time);
    match blend {
        MixBlend::Setup => {
            bone.x = setup_x + x * alpha;
            bone.y = setup_y + y * alpha;
        }
        MixBlend::First | MixBlend::Replace => {
            bone.x += (setup_x + x - bone.x) * alpha;
            bone.y += (setup_y + y - bone.y) * alpha;
        }
        MixBlend::Add => {
            bone.x += x * alpha;
            bone.y += y * alpha;
        }
    }
}

fn apply_translate_x(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].x;
    let bone = &mut skeleton.bones[idx];
    bone.x = relative_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        bone.x,
        setup,
    );
}

fn apply_translate_y(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].y;
    let bone = &mut skeleton.bones[idx];
    bone.y = relative_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        bone.y,
        setup,
    );
}

// ---------------------------------------------------------------------------
// Scale (and X/Y splits)
// ---------------------------------------------------------------------------

/// Literal port of `ScaleTimeline::apply` — the scale mix rules are
/// intricate enough (sign-handling on abs magnitude, setup-vs-current pose
/// baselines, direction-aware sign source) that factoring would muddy the
/// diff against the reference.
#[allow(clippy::too_many_arguments, clippy::float_cmp)]
fn apply_scale(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let data_scale_x = skeleton.data.bones[idx].scale_x;
    let data_scale_y = skeleton.data.bones[idx].scale_y;
    let bone = &mut skeleton.bones[idx];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                bone.scale_x = data_scale_x;
                bone.scale_y = data_scale_y;
            }
            MixBlend::First => {
                bone.scale_x += (data_scale_x - bone.scale_x) * alpha;
                bone.scale_y += (data_scale_y - bone.scale_y) * alpha;
            }
            _ => {}
        }
        return;
    }

    let (mut x, mut y) = curve_value2(&curves.frames, &curves.curves, time);
    x *= data_scale_x;
    y *= data_scale_y;

    if alpha == 1.0 {
        if blend == MixBlend::Add {
            bone.scale_x += x - data_scale_x;
            bone.scale_y += y - data_scale_y;
        } else {
            bone.scale_x = x;
            bone.scale_y = y;
        }
        return;
    }

    if direction == MixDirection::Out {
        match blend {
            MixBlend::Setup => {
                let bx = data_scale_x;
                let by = data_scale_y;
                bone.scale_x = bx + (x.abs() * bx.signum() - bx) * alpha;
                bone.scale_y = by + (y.abs() * by.signum() - by) * alpha;
            }
            MixBlend::First | MixBlend::Replace => {
                let bx = bone.scale_x;
                let by = bone.scale_y;
                bone.scale_x = bx + (x.abs() * bx.signum() - bx) * alpha;
                bone.scale_y = by + (y.abs() * by.signum() - by) * alpha;
            }
            MixBlend::Add => {
                bone.scale_x += (x - data_scale_x) * alpha;
                bone.scale_y += (y - data_scale_y) * alpha;
            }
        }
    } else {
        match blend {
            MixBlend::Setup => {
                let bx = data_scale_x.abs() * x.signum();
                let by = data_scale_y.abs() * y.signum();
                bone.scale_x = bx + (x - bx) * alpha;
                bone.scale_y = by + (y - by) * alpha;
            }
            MixBlend::First | MixBlend::Replace => {
                let bx = bone.scale_x.abs() * x.signum();
                let by = bone.scale_y.abs() * y.signum();
                bone.scale_x = bx + (x - bx) * alpha;
                bone.scale_y = by + (y - by) * alpha;
            }
            MixBlend::Add => {
                bone.scale_x += (x - data_scale_x) * alpha;
                bone.scale_y += (y - data_scale_y) * alpha;
            }
        }
    }
}

fn apply_scale_x(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].scale_x;
    let bone = &mut skeleton.bones[idx];
    bone.scale_x = scale_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        direction,
        bone.scale_x,
        setup,
    );
}

fn apply_scale_y(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].scale_y;
    let bone = &mut skeleton.bones[idx];
    bone.scale_y = scale_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        direction,
        bone.scale_y,
        setup,
    );
}

// ---------------------------------------------------------------------------
// Shear (and X/Y splits)
// ---------------------------------------------------------------------------

fn apply_shear(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup_x = skeleton.data.bones[idx].shear_x;
    let setup_y = skeleton.data.bones[idx].shear_y;
    let bone = &mut skeleton.bones[idx];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                bone.shear_x = setup_x;
                bone.shear_y = setup_y;
            }
            MixBlend::First => {
                bone.shear_x += (setup_x - bone.shear_x) * alpha;
                bone.shear_y += (setup_y - bone.shear_y) * alpha;
            }
            _ => {}
        }
        return;
    }

    let (x, y) = curve_value2(&curves.frames, &curves.curves, time);
    match blend {
        MixBlend::Setup => {
            bone.shear_x = setup_x + x * alpha;
            bone.shear_y = setup_y + y * alpha;
        }
        MixBlend::First | MixBlend::Replace => {
            bone.shear_x += (setup_x + x - bone.shear_x) * alpha;
            bone.shear_y += (setup_y + y - bone.shear_y) * alpha;
        }
        MixBlend::Add => {
            bone.shear_x += x * alpha;
            bone.shear_y += y * alpha;
        }
    }
}

fn apply_shear_x(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].shear_x;
    let bone = &mut skeleton.bones[idx];
    bone.shear_x = relative_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        bone.shear_x,
        setup,
    );
}

fn apply_shear_y(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].shear_y;
    let bone = &mut skeleton.bones[idx];
    bone.shear_y = relative_value(
        &curves.frames,
        &curves.curves,
        time,
        alpha,
        blend,
        bone.shear_y,
        setup,
    );
}

// ---------------------------------------------------------------------------
// Inherit timeline (4.2)
// ---------------------------------------------------------------------------

fn apply_inherit(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    frames: &[f32],
    inherits: &[Inherit],
    time: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    let Some(idx) = active_bone(skeleton, bone_id) else {
        return;
    };
    let setup = skeleton.data.bones[idx].inherit;
    let bone = &mut skeleton.bones[idx];

    // Mixing out of an animation: only reset on Setup blend; otherwise
    // preserve whatever state we're at.
    if direction == MixDirection::Out {
        if blend == MixBlend::Setup {
            bone.inherit = setup;
        }
        return;
    }

    if time < frames[0] {
        if blend == MixBlend::Setup || blend == MixBlend::First {
            bone.inherit = setup;
        }
        return;
    }

    // Stepped lookup — no interpolation (you can't tween between enum values).
    // spine-cpp uses `Animation::search(_frames, time, ENTRIES) + INHERIT`
    // where ENTRIES = 2, INHERIT = 1; our storage splits frames (times only)
    // and inherits into sibling vecs so the index maps through step = 1.
    let i = search(frames, time, 1);
    bone.inherit = inherits[i];
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::animation::CURVE_LINEAR;
    use crate::data::{BoneData, SkeletonData};
    use std::sync::Arc;

    /// Build a one-bone skeleton and return its Skeleton + a helper that
    /// always yields `&mut skeleton.bones[0]`. Bones default to setup pose,
    /// but we tweak `setup_rotation` via `root_rotation`.
    fn one_bone(root_rotation: f32) -> Skeleton {
        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.rotation = root_rotation;
        sd.bones.push(root);
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();
        sk
    }

    #[test]
    fn rotate_setup_blend_overwrites_to_setup_plus_value() {
        // Linear ramp from 5 at t=0 to 15 at t=1. Setup rotation = 10.
        let curves = CurveFrames {
            frames: vec![0.0, 5.0, 1.0, 15.0],
            curves: vec![CURVE_LINEAR as f32, CURVE_LINEAR as f32],
        };
        let tl = Timeline::Rotate {
            bone: BoneId(0),
            curves,
        };

        let mut sk = one_bone(10.0);
        sk.bones[0].rotation = 999.0; // clobber to ensure Setup overwrites
        let mut events = Vec::new();
        tl.apply(
            &mut sk,
            0.0,
            0.5,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        // time 0.5 on the ramp = 10 (timeline value). Setup + value * alpha
        // = 10 + 10 * 1.0 = 20.
        assert!((sk.bones[0].rotation - 20.0).abs() < 1e-6);
    }

    #[test]
    fn translate_before_first_frame_setup_returns_data_values() {
        let curves = CurveFrames {
            frames: vec![1.0, 3.0, 4.0, 2.0, 6.0, 8.0], // first frame t=1
            curves: vec![CURVE_LINEAR as f32, CURVE_LINEAR as f32],
        };
        let tl = Timeline::Translate {
            bone: BoneId(0),
            curves,
        };
        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.x = 7.0;
        root.y = 9.0;
        sd.bones.push(root);
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();
        sk.bones[0].x = 100.0;
        sk.bones[0].y = 200.0;

        let mut events = Vec::new();
        tl.apply(
            &mut sk,
            0.0,
            0.0, // before first frame (t=1)
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.bones[0].x, 7.0);
        assert_eq!(sk.bones[0].y, 9.0);
    }

    #[test]
    fn scale_alpha_one_setup_replaces_with_curve_times_setup() {
        // Scale curve value at t=0.5 is 2.0 (linear from 2→2). Setup scale = 3.
        // Expected: bone.scale_x = 2.0 * 3.0 = 6.0.
        let curves = CurveFrames {
            frames: vec![0.0, 2.0, 2.0, 1.0, 2.0, 2.0],
            curves: vec![CURVE_LINEAR as f32, CURVE_LINEAR as f32],
        };
        let tl = Timeline::Scale {
            bone: BoneId(0),
            curves,
        };

        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.scale_x = 3.0;
        root.scale_y = 3.0;
        sd.bones.push(root);
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        let mut events = Vec::new();
        tl.apply(
            &mut sk,
            0.0,
            0.5,
            &mut events,
            1.0, // alpha = 1 replaces directly
            MixBlend::Setup,
            MixDirection::In,
        );
        assert!((sk.bones[0].scale_x - 6.0).abs() < 1e-6);
        assert!((sk.bones[0].scale_y - 6.0).abs() < 1e-6);
    }

    #[test]
    fn inactive_bone_is_skipped() {
        let curves = CurveFrames {
            frames: vec![0.0, 42.0, 1.0, 99.0],
            curves: vec![CURVE_LINEAR as f32, CURVE_LINEAR as f32],
        };
        let tl = Timeline::Rotate {
            bone: BoneId(0),
            curves,
        };

        let mut sk = one_bone(0.0);
        sk.bones[0].active = false;
        sk.bones[0].rotation = 7.7;
        let mut events = Vec::new();
        tl.apply(
            &mut sk,
            0.0,
            0.5,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.bones[0].rotation, 7.7);
    }

    #[test]
    fn inherit_timeline_steps_to_last_keyframe() {
        let tl = Timeline::Inherit {
            bone: BoneId(0),
            frames: vec![0.0, 1.0, 2.0],
            inherits: vec![
                Inherit::OnlyTranslation,
                Inherit::NoScale,
                Inherit::NoScaleOrReflection,
            ],
        };

        let mut sk = one_bone(0.0);
        assert_eq!(sk.bones[0].inherit, Inherit::Normal);

        let mut events = Vec::new();
        // At t=0.5 → frame 0 → OnlyTranslation.
        tl.apply(
            &mut sk,
            0.0,
            0.5,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.bones[0].inherit, Inherit::OnlyTranslation);
        // At t=1.5 → frame 1 → NoScale.
        tl.apply(
            &mut sk,
            0.0,
            1.5,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.bones[0].inherit, Inherit::NoScale);
        // At t=5.0 (past last) → stays at last frame (NoScaleOrReflection).
        tl.apply(
            &mut sk,
            0.0,
            5.0,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.bones[0].inherit, Inherit::NoScaleOrReflection);
    }
}
