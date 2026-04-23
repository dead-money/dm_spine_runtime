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

use crate::animation::curve::{
    bezier_value, curve_value1, curve_value2, relative_value, scale_value, search,
};
use crate::animation::{
    BEZIER_SIZE, CURVE_BEZIER, CURVE_LINEAR, CURVE_STEPPED, Event, MixBlend, MixDirection,
};
use crate::data::{AnimationEvent, BoneId, CurveFrames, Inherit, SlotId, Timeline};
use crate::math::Color;
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

            // --- Slot timelines (Phase 3c) ---
            Timeline::Rgba { slot, curves } => {
                apply_rgba(skeleton, *slot, curves, time, alpha, blend);
            }
            Timeline::Rgb { slot, curves } => {
                apply_rgb(skeleton, *slot, curves, time, alpha, blend);
            }
            Timeline::Alpha { slot, curves } => {
                apply_alpha(skeleton, *slot, curves, time, alpha, blend);
            }
            Timeline::Rgba2 { slot, curves } => {
                apply_rgba2(skeleton, *slot, curves, time, alpha, blend);
            }
            Timeline::Rgb2 { slot, curves } => {
                apply_rgb2(skeleton, *slot, curves, time, alpha, blend);
            }
            Timeline::Attachment {
                slot,
                frames,
                names,
            } => {
                apply_attachment(skeleton, *slot, frames, names, time, blend, direction);
            }

            // --- Skeleton-wide timelines (Phase 3c) ---
            Timeline::DrawOrder {
                frames,
                draw_orders,
            } => {
                apply_draw_order(skeleton, frames, draw_orders, time, blend, direction);
            }
            Timeline::Event {
                frames,
                events: keyframes,
            } => {
                apply_event(frames, keyframes, last_time, time, events);
            }

            // --- Deform / Sequence and all constraint timelines land later ---
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

// ---------------------------------------------------------------------------
// Slot timelines
// ---------------------------------------------------------------------------

/// `true` iff the slot's bone is currently active. spine-cpp's slot applies
/// always gate on `slot->_bone._active`; we derive the bone via data since
/// our `Slot` runtime doesn't cache a bone reference.
fn slot_bone_active(skeleton: &Skeleton, slot_id: SlotId) -> bool {
    let bone_id = skeleton.data.slots[slot_id.index()].bone;
    skeleton.bones[bone_id.index()].active
}

/// Sample N channel values (not including time) at `time` from a colour
/// timeline with stride `1 + N`. Returns values in an array ordered R, G,
/// B, A, ... per spine-cpp's `R=1, G=2, B=3, A=4` offset convention.
///
/// Handles LINEAR, STEPPED, and BEZIER curve types. Used by Rgba, Rgb,
/// Rgba2, Rgb2.
fn sample_color_channels<const N: usize>(frames: &[f32], curves: &[f32], time: f32) -> [f32; N] {
    let entries = N + 1;
    let i = search(frames, time, entries);
    let curve_type = curves[i / entries] as i32;
    let mut out = [0.0_f32; N];
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            for (k, o) in out.iter_mut().enumerate() {
                *o = frames[i + 1 + k];
            }
            let t = (time - before) / (frames[i + entries] - before);
            for (k, o) in out.iter_mut().enumerate() {
                *o += (frames[i + entries + 1 + k] - *o) * t;
            }
        }
        CURVE_STEPPED => {
            for (k, o) in out.iter_mut().enumerate() {
                *o = frames[i + 1 + k];
            }
        }
        _ => {
            let bezier_0 = (curve_type - CURVE_BEZIER) as usize;
            for (k, o) in out.iter_mut().enumerate() {
                *o = bezier_value(
                    frames,
                    curves,
                    time,
                    i,
                    1 + k,
                    bezier_0 + k * BEZIER_SIZE,
                    entries,
                );
            }
        }
    }
    out
}

/// Clamped element-wise add, for `Color::add` parity. spine-cpp's
/// `Color::add` clamps each channel into `[0, 1]`; our [`Color::set`]
/// already does this via the constructor.
fn color_add(c: &mut Color, r: f32, g: f32, b: f32, a: f32) {
    let (cr, cg, cb, ca) = (c.r, c.g, c.b, c.a);
    *c = Color::new(cr + r, cg + g, cb + b, ca + a);
}

/// Blend timeline RGBA values `(r, g, b, a)` into `c` with `alpha`.
/// Equivalent to `color_add(c, (r - c.r) * alpha, ...)` but without the
/// aliasing hazards of holding `&mut c` while reading `c`.
fn color_lerp_toward(c: &mut Color, r: f32, g: f32, b: f32, a: f32, alpha: f32) {
    let dr = (r - c.r) * alpha;
    let dg = (g - c.g) * alpha;
    let db = (b - c.b) * alpha;
    let da = (a - c.a) * alpha;
    color_add(c, dr, dg, db, da);
}

/// Same as [`color_lerp_toward`] but leaves alpha untouched (RGB-only
/// timelines).
fn color_lerp_rgb_toward(c: &mut Color, r: f32, g: f32, b: f32, alpha: f32) {
    let dr = (r - c.r) * alpha;
    let dg = (g - c.g) * alpha;
    let db = (b - c.b) * alpha;
    color_add(c, dr, dg, db, 0.0);
}

#[allow(clippy::float_cmp)] // `alpha == 1.0` is a tag check, not imprecise equality
fn apply_rgba(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    let setup = skeleton.data.slots[slot_id.index()].color;
    let slot = &mut skeleton.slots[slot_id.index()];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                slot.color = setup;
            }
            MixBlend::First => {
                color_lerp_toward(&mut slot.color, setup.r, setup.g, setup.b, setup.a, alpha);
            }
            _ => {}
        }
        return;
    }

    let [r, g, b, a] = sample_color_channels::<4>(&curves.frames, &curves.curves, time);
    if alpha == 1.0 {
        slot.color = Color::new(r, g, b, a);
    } else {
        if blend == MixBlend::Setup {
            slot.color = setup;
        }
        color_lerp_toward(&mut slot.color, r, g, b, a, alpha);
    }
}

#[allow(clippy::float_cmp)]
fn apply_rgb(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    let setup = skeleton.data.slots[slot_id.index()].color;
    let slot = &mut skeleton.slots[slot_id.index()];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                slot.color = setup;
            }
            MixBlend::First => {
                color_lerp_toward(&mut slot.color, setup.r, setup.g, setup.b, setup.a, alpha);
            }
            _ => {}
        }
        return;
    }

    let [r, g, b] = sample_color_channels::<3>(&curves.frames, &curves.curves, time);
    if alpha == 1.0 {
        slot.color = Color::new(r, g, b, slot.color.a);
    } else {
        if blend == MixBlend::Setup {
            slot.color = Color::new(setup.r, setup.g, setup.b, slot.color.a);
        }
        color_lerp_rgb_toward(&mut slot.color, r, g, b, alpha);
    }
}

#[allow(clippy::float_cmp)]
fn apply_alpha(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    let setup = skeleton.data.slots[slot_id.index()].color;
    let slot = &mut skeleton.slots[slot_id.index()];

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => slot.color.a = setup.a,
            MixBlend::First => slot.color.a += (setup.a - slot.color.a) * alpha,
            _ => {}
        }
        return;
    }

    let a = curve_value1(&curves.frames, &curves.curves, time);
    if alpha == 1.0 {
        slot.color.a = a;
    } else {
        if blend == MixBlend::Setup {
            slot.color.a = setup.a;
        }
        slot.color.a += (a - slot.color.a) * alpha;
    }
}

#[allow(clippy::float_cmp)]
fn apply_rgba2(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    // Dark-color slots that weren't exported with a dark colour skip the
    // timeline — matches spine-cpp's `_hasDarkColor` gate on RGBA2Timeline.
    let setup_light = skeleton.data.slots[slot_id.index()].color;
    let Some(setup_dark) = skeleton.data.slots[slot_id.index()].dark_color else {
        return;
    };
    let slot = &mut skeleton.slots[slot_id.index()];
    let Some(dark) = slot.dark_color.as_mut() else {
        return;
    };

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                slot.color = setup_light;
                *dark = setup_dark;
            }
            MixBlend::First => {
                color_lerp_toward(
                    &mut slot.color,
                    setup_light.r,
                    setup_light.g,
                    setup_light.b,
                    setup_light.a,
                    alpha,
                );
                color_lerp_rgb_toward(dark, setup_dark.r, setup_dark.g, setup_dark.b, alpha);
            }
            _ => {}
        }
        return;
    }

    // stride 8: [t, r, g, b, a, r2, g2, b2]
    let [r, g, b, a, r2, g2, b2] = sample_color_channels::<7>(&curves.frames, &curves.curves, time);
    if alpha == 1.0 {
        slot.color = Color::new(r, g, b, a);
        *dark = Color::new(r2, g2, b2, dark.a);
    } else {
        if blend == MixBlend::Setup {
            slot.color = setup_light;
            *dark = Color::new(setup_dark.r, setup_dark.g, setup_dark.b, dark.a);
        }
        color_lerp_toward(&mut slot.color, r, g, b, a, alpha);
        color_lerp_rgb_toward(dark, r2, g2, b2, alpha);
    }
}

#[allow(clippy::float_cmp)]
fn apply_rgb2(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    curves: &CurveFrames,
    time: f32,
    alpha: f32,
    blend: MixBlend,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    let setup_light = skeleton.data.slots[slot_id.index()].color;
    let Some(setup_dark) = skeleton.data.slots[slot_id.index()].dark_color else {
        return;
    };
    let slot = &mut skeleton.slots[slot_id.index()];
    let Some(dark) = slot.dark_color.as_mut() else {
        return;
    };

    if time < curves.frames[0] {
        match blend {
            MixBlend::Setup => {
                slot.color = Color::new(setup_light.r, setup_light.g, setup_light.b, slot.color.a);
                *dark = Color::new(setup_dark.r, setup_dark.g, setup_dark.b, dark.a);
            }
            MixBlend::First => {
                color_lerp_rgb_toward(
                    &mut slot.color,
                    setup_light.r,
                    setup_light.g,
                    setup_light.b,
                    alpha,
                );
                color_lerp_rgb_toward(dark, setup_dark.r, setup_dark.g, setup_dark.b, alpha);
            }
            _ => {}
        }
        return;
    }

    // stride 7: [t, r, g, b, r2, g2, b2]
    let [r, g, b, r2, g2, b2] = sample_color_channels::<6>(&curves.frames, &curves.curves, time);
    if alpha == 1.0 {
        slot.color = Color::new(r, g, b, slot.color.a);
        *dark = Color::new(r2, g2, b2, dark.a);
    } else {
        if blend == MixBlend::Setup {
            slot.color = Color::new(setup_light.r, setup_light.g, setup_light.b, slot.color.a);
            *dark = Color::new(setup_dark.r, setup_dark.g, setup_dark.b, dark.a);
        }
        color_lerp_rgb_toward(&mut slot.color, r, g, b, alpha);
        color_lerp_rgb_toward(dark, r2, g2, b2, alpha);
    }
}

fn apply_attachment(
    skeleton: &mut Skeleton,
    slot_id: SlotId,
    frames: &[f32],
    names: &[Option<String>],
    time: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    if !slot_bone_active(skeleton, slot_id) {
        return;
    }
    let setup_name = skeleton.data.slots[slot_id.index()].attachment_name.clone();

    // Mixing out: only reset to setup-pose attachment on Setup blend.
    if direction == MixDirection::Out {
        if blend == MixBlend::Setup {
            let att = setup_name
                .as_deref()
                .and_then(|n| skeleton.get_attachment(slot_id, n));
            skeleton.slots[slot_id.index()].attachment = att;
        }
        return;
    }

    if time < frames[0] {
        if blend == MixBlend::Setup || blend == MixBlend::First {
            let att = setup_name
                .as_deref()
                .and_then(|n| skeleton.get_attachment(slot_id, n));
            skeleton.slots[slot_id.index()].attachment = att;
        }
        return;
    }

    // Stepped lookup (attachment name is discrete). Frames is just times, so
    // step = 1; names[i] is the attachment for frame i.
    let i = search(frames, time, 1);
    let att = names[i]
        .as_deref()
        .and_then(|n| skeleton.get_attachment(slot_id, n));
    skeleton.slots[slot_id.index()].attachment = att;
}

// ---------------------------------------------------------------------------
// Skeleton-wide timelines
// ---------------------------------------------------------------------------

fn apply_draw_order(
    skeleton: &mut Skeleton,
    frames: &[f32],
    draw_orders: &[Option<Vec<SlotId>>],
    time: f32,
    blend: MixBlend,
    direction: MixDirection,
) {
    let slot_count = skeleton.slots.len();

    // Mixing out on Setup blend: restore identity permutation. Otherwise
    // leave draw_order alone.
    if direction == MixDirection::Out {
        if blend == MixBlend::Setup {
            skeleton.draw_order.clear();
            skeleton
                .draw_order
                .extend((0..slot_count).map(|i| SlotId(i as u16)));
        }
        return;
    }

    if time < frames[0] {
        if blend == MixBlend::Setup || blend == MixBlend::First {
            skeleton.draw_order.clear();
            skeleton
                .draw_order
                .extend((0..slot_count).map(|i| SlotId(i as u16)));
        }
        return;
    }

    let i = search(frames, time, 1);
    match &draw_orders[i] {
        None => {
            // A None frame means "restore identity order".
            skeleton.draw_order.clear();
            skeleton
                .draw_order
                .extend((0..slot_count).map(|i| SlotId(i as u16)));
        }
        Some(order) => {
            skeleton.draw_order.clear();
            skeleton.draw_order.extend_from_slice(order);
        }
    }
}

#[allow(clippy::float_cmp)] // matching spine-cpp's `frames[i - 1] != frameTime` equality check verbatim
fn apply_event(
    frames: &[f32],
    keyframes: &[AnimationEvent],
    last_time: f32,
    time: f32,
    events: &mut Vec<Event>,
) {
    let frame_count = frames.len();
    if frame_count == 0 {
        return;
    }

    let (mut last_time, time) = if last_time > time {
        // Looped back: fire every event after last_time (to infinity),
        // then re-enter with last_time = -1 so the caller-visible range
        // [0, time] fires on the wrap.
        apply_event(frames, keyframes, last_time, f32::MAX, events);
        (-1.0_f32, time)
    } else if last_time >= frames[frame_count - 1] {
        return;
    } else {
        (last_time, time)
    };

    if time < frames[0] {
        return;
    }

    let mut i: usize = if last_time < frames[0] {
        0
    } else {
        // spine-cpp: Animation::search(frames, lastTime) + 1, then walk back
        // to fire every event keyed at the same time as the one we landed on.
        let mut i = search(frames, last_time, 1) + 1;
        let frame_time = frames[i.min(frame_count - 1)];
        while i > 0 && frames[i - 1] == frame_time {
            i -= 1;
        }
        i
    };

    while i < frame_count && time >= frames[i] {
        let k = &keyframes[i];
        events.push(Event {
            data: k.event,
            time: k.time,
            int_value: k.int_value,
            float_value: k.float_value,
            string_value: k.string_value.clone(),
            volume: k.volume,
            balance: k.balance,
        });
        i += 1;
    }
    let _ = &mut last_time; // silences unused_mut after the `if lastTime > time` branch
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

    /// Rgba timeline writes linear-interpolated colour at alpha=1 with
    /// Setup blend.
    #[test]
    fn rgba_timeline_setup_blend_writes_interpolated_color() {
        use crate::data::SlotData;
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.slots
            .push(SlotData::new(crate::data::SlotId(0), "body", BoneId(0)));
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        // Linear from (0, 0, 0, 0) at t=0 to (1, 0.5, 0.25, 0.75) at t=1.
        let curves = CurveFrames {
            frames: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5, 0.25, 0.75],
            curves: vec![CURVE_LINEAR as f32, CURVE_LINEAR as f32],
        };
        let tl = Timeline::Rgba {
            slot: crate::data::SlotId(0),
            curves,
        };

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
        let c = sk.slots[0].color;
        assert!((c.r - 0.5).abs() < 1e-6);
        assert!((c.g - 0.25).abs() < 1e-6);
        assert!((c.b - 0.125).abs() < 1e-6);
        assert!((c.a - 0.375).abs() < 1e-6);
    }

    /// `DrawOrder` timeline writes a permutation frame's order.
    #[test]
    fn draw_order_timeline_applies_permutation() {
        use crate::data::SlotData;
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            sd.slots.push(SlotData::new(
                crate::data::SlotId(i as u16),
                *name,
                BoneId(0),
            ));
        }
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        let sid = crate::data::SlotId;
        let tl = Timeline::DrawOrder {
            frames: vec![0.0, 1.0],
            draw_orders: vec![
                Some(vec![sid(2), sid(0), sid(1)]), // swap at t=0
                None,                               // identity at t=1
            ],
        };

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
        assert_eq!(sk.draw_order, vec![sid(2), sid(0), sid(1)]);

        // Time past the second frame restores identity via None sentinel.
        tl.apply(
            &mut sk,
            0.5,
            2.0,
            &mut events,
            1.0,
            MixBlend::Setup,
            MixDirection::In,
        );
        assert_eq!(sk.draw_order, vec![sid(0), sid(1), sid(2)]);
    }

    /// Event timeline fires every event keyed between (`last_time`, time].
    #[test]
    fn event_timeline_fires_events_in_window() {
        use crate::data::{AnimationEvent, EventData, EventId};

        let mut sd = SkeletonData::default();
        sd.events.push(EventData::new(EventId(0), "step"));
        sd.events.push(EventData::new(EventId(1), "clap"));
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        let tl = Timeline::Event {
            frames: vec![0.5, 1.0, 1.5],
            events: vec![
                AnimationEvent {
                    time: 0.5,
                    event: EventId(0),
                    int_value: 1,
                    float_value: 0.0,
                    string_value: None,
                    volume: 1.0,
                    balance: 0.0,
                },
                AnimationEvent {
                    time: 1.0,
                    event: EventId(1),
                    int_value: 2,
                    float_value: 0.0,
                    string_value: None,
                    volume: 1.0,
                    balance: 0.0,
                },
                AnimationEvent {
                    time: 1.5,
                    event: EventId(0),
                    int_value: 3,
                    float_value: 0.0,
                    string_value: None,
                    volume: 1.0,
                    balance: 0.0,
                },
            ],
        };

        // Window (0.0, 1.2] should fire events at 0.5 and 1.0 but not 1.5.
        let mut events = Vec::new();
        tl.apply(
            &mut sk,
            0.0,
            1.2,
            &mut events,
            1.0,
            MixBlend::Replace,
            MixDirection::In,
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].int_value, 1);
        assert_eq!(events[1].int_value, 2);
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
