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

//! Applying an animation's timelines to a skeleton, through
//! [`Skeleton::apply_animation`].
//!
//! The mix parameters shared by the functions here and in
//! [`curve`][crate::animation::curve]:
//!
//! - `alpha`: weight of the timeline's value, 0 to 1.
//! - `from`: what the value mixes from before and between keys (see
//!   [`MixFrom`]).
//! - `add`: add the keyed value to the current one instead of mixing toward
//!   it. Only timelines that support it ([`Timeline::is_additive`]) honor it.
//! - `out`: the animation is mixing out. Timelines with discrete values
//!   (attachment, draw order, inherit, sequence, IK bend direction) stop
//!   applying keys and return to setup or keep the current value, per
//!   `from`. Scale timelines keep the sign of the value they mix from.
//! - `applied`: write the constrained (applied) pose rather than the
//!   unconstrained pose. Slider constraints apply animations this way;
//!   [`AnimationState`][crate::animation::AnimationState] does not.

#![allow(
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_range_loop
)]

use crate::animation::curve::{
    absolute_value, curve_value1, curve_value2, curve_values, relative_value, scale_value, search,
    sign,
};
use crate::animation::{BEZIER_SIZE, CURVE_BEZIER, CURVE_LINEAR, CURVE_STEPPED, Event, MixFrom};
use crate::data::skin::resolve;
use crate::data::{
    AnimationId, Attachment, AttachmentId, BoneId, ConstraintId, CurveFrames,
    PhysicsConstraintData, PhysicsProperty, SkeletonData, SkinKey, SlotId, Timeline,
};
use crate::math::Color;
use crate::skeleton::{Constraint, PhysicsConstraint, Skeleton, SlotPose};

impl Skeleton {
    /// Applies every timeline of `animation` at `time` (seconds). With
    /// `looping`, `time` and a positive `last_time` wrap by the duration.
    /// Events keyed in `(last_time, time]` are appended to `events`; `None`
    /// skips them. Pass a `last_time` of -1 to include events keyed at 0.
    /// The mix parameters are described in the [module docs](crate::animation::apply).
    ///
    /// # Panics
    ///
    /// If `animation` isn't an index into this skeleton's
    /// [`SkeletonData::animations`].
    pub fn apply_animation(
        &mut self,
        animation: AnimationId,
        mut last_time: f32,
        mut time: f32,
        looping: bool,
        mut events: Option<&mut Vec<Event>>,
        alpha: f32,
        from: MixFrom,
        add: bool,
        out: bool,
        applied: bool,
    ) {
        let sd = std::sync::Arc::clone(self.data());
        let anim = &sd.animations[animation.index()];
        if looping && anim.duration != 0.0 {
            time %= anim.duration;
            if last_time > 0.0 {
                last_time %= anim.duration;
            }
        }
        for t in &anim.timelines {
            apply_timeline(
                self,
                &sd,
                t,
                last_time,
                time,
                &mut events,
                alpha,
                from,
                add,
                out,
                applied,
            );
        }
    }
}

/// Applies one timeline. Parameters are as for [`Skeleton::apply_animation`].
pub(crate) fn apply_timeline(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    t: &Timeline,
    last_time: f32,
    time: f32,
    events: &mut Option<&mut Vec<Event>>,
    alpha: f32,
    from: MixFrom,
    add: bool,
    out: bool,
    applied: bool,
) {
    match t {
        Timeline::Rotate { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.rotation = relative_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                p.rotation,
                s.rotation,
            );
        }),
        Timeline::TranslateX { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.x = relative_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                p.x,
                s.x,
            );
        }),
        Timeline::TranslateY { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.y = relative_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                p.y,
                s.y,
            );
        }),
        Timeline::ScaleX { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.scale_x = scale_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                out,
                p.scale_x,
                s.scale_x,
            );
        }),
        Timeline::ScaleY { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.scale_y = scale_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                out,
                p.scale_y,
                s.scale_y,
            );
        }),
        Timeline::ShearX { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.shear_x = relative_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                p.shear_x,
                s.shear_x,
            );
        }),
        Timeline::ShearY { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            p.shear_y = relative_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                p.shear_y,
                s.shear_y,
            );
        }),
        Timeline::Translate { bone, curves } | Timeline::Shear { bone, curves } => {
            let shear = matches!(t, Timeline::Shear { .. });
            bone1(sk, sd, *bone, applied, |p, s| {
                let (px, py, sx, sy) = if shear {
                    (&mut p.shear_x, &mut p.shear_y, s.shear_x, s.shear_y)
                } else {
                    (&mut p.x, &mut p.y, s.x, s.y)
                };
                if time < curves.frames[0] {
                    match from {
                        MixFrom::Setup => {
                            *px = sx;
                            *py = sy;
                        }
                        MixFrom::First => {
                            *px += (sx - *px) * alpha;
                            *py += (sy - *py) * alpha;
                        }
                        MixFrom::Current => {}
                    }
                    return;
                }
                let (x, y) = curve_value2(&curves.frames, &curves.curves, time);
                if from == MixFrom::Setup {
                    *px = sx + x * alpha;
                    *py = sy + y * alpha;
                } else if add {
                    *px += x * alpha;
                    *py += y * alpha;
                } else {
                    *px += (sx + x - *px) * alpha;
                    *py += (sy + y - *py) * alpha;
                }
            });
        }
        Timeline::Scale { bone, curves } => bone1(sk, sd, *bone, applied, |p, s| {
            if time < curves.frames[0] {
                match from {
                    MixFrom::Setup => {
                        p.scale_x = s.scale_x;
                        p.scale_y = s.scale_y;
                    }
                    MixFrom::First => {
                        p.scale_x += (s.scale_x - p.scale_x) * alpha;
                        p.scale_y += (s.scale_y - p.scale_y) * alpha;
                    }
                    MixFrom::Current => {}
                }
                return;
            }
            let (mut x, mut y) = curve_value2(&curves.frames, &curves.curves, time);
            x *= s.scale_x;
            y *= s.scale_y;
            if alpha == 1.0 && !add {
                p.scale_x = x;
                p.scale_y = y;
                return;
            }
            let (mut bx, mut by) = if from == MixFrom::Setup {
                (s.scale_x, s.scale_y)
            } else {
                (p.scale_x, p.scale_y)
            };
            if add {
                p.scale_x = bx + (x - s.scale_x) * alpha;
                p.scale_y = by + (y - s.scale_y) * alpha;
            } else if out {
                p.scale_x = bx + (x.abs() * sign(bx) - bx) * alpha;
                p.scale_y = by + (y.abs() * sign(by) - by) * alpha;
            } else {
                bx = bx.abs() * sign(x);
                by = by.abs() * sign(y);
                p.scale_x = bx + (x - bx) * alpha;
                p.scale_y = by + (y - by) * alpha;
            }
        }),
        Timeline::Inherit {
            bone,
            frames,
            inherits,
        } => {
            let b = &mut sk.bones[bone.index()];
            if !b.active {
                return;
            }
            let setup = sd.bones[bone.index()].setup.inherit;
            let p = b.posed.select_mut(applied);
            if out || time < frames[0] {
                if from != MixFrom::Current {
                    p.inherit = setup;
                }
            } else {
                p.inherit = inherits[search(frames, time, 1)];
            }
        }

        Timeline::Rgba { slot, curves } => color_timeline(sk, sd, *slot, applied, |pose, setup| {
            if time < curves.frames[0] {
                match from {
                    MixFrom::Setup => set_color(&mut pose.color, setup.color),
                    MixFrom::First => {
                        let d = delta(setup.color, pose.color, alpha);
                        add_color(&mut pose.color, d);
                    }
                    MixFrom::Current => {}
                }
                return;
            }
            let (_, [r, g, b, a]) = curve_values::<4>(&curves.frames, &curves.curves, time, 5);
            let c = &mut pose.color;
            if alpha == 1.0 {
                set_color(c, Color::new(r, g, b, a));
            } else if from == MixFrom::Setup {
                let s = setup.color;
                set_color(
                    c,
                    Color::new(
                        s.r + (r - s.r) * alpha,
                        s.g + (g - s.g) * alpha,
                        s.b + (b - s.b) * alpha,
                        s.a + (a - s.a) * alpha,
                    ),
                );
            } else {
                let d = Color::new(
                    (r - c.r) * alpha,
                    (g - c.g) * alpha,
                    (b - c.b) * alpha,
                    (a - c.a) * alpha,
                );
                add_color(c, d);
            }
        }),
        Timeline::Rgb { slot, curves } => color_timeline(sk, sd, *slot, applied, |pose, setup| {
            let c = &mut pose.color;
            if time < curves.frames[0] {
                let s = setup.color;
                match from {
                    MixFrom::Setup => {
                        c.r = s.r;
                        c.g = s.g;
                        c.b = s.b;
                    }
                    MixFrom::First => {
                        c.r += (s.r - c.r) * alpha;
                        c.g += (s.g - c.g) * alpha;
                        c.b += (s.b - c.b) * alpha;
                    }
                    MixFrom::Current => {}
                }
                return;
            }
            let (_, [mut r, mut g, mut b]) =
                curve_values::<3>(&curves.frames, &curves.curves, time, 4);
            if alpha != 1.0 {
                let base = if from == MixFrom::Setup {
                    setup.color
                } else {
                    *c
                };
                r = base.r + (r - base.r) * alpha;
                g = base.g + (g - base.g) * alpha;
                b = base.b + (b - base.b) * alpha;
            }
            c.r = clamp01(r);
            c.g = clamp01(g);
            c.b = clamp01(b);
        }),
        Timeline::Alpha { slot, curves } => {
            color_timeline(sk, sd, *slot, applied, |pose, setup| {
                let c = &mut pose.color;
                if time < curves.frames[0] {
                    match from {
                        MixFrom::Setup => c.a = setup.color.a,
                        MixFrom::First => c.a += (setup.color.a - c.a) * alpha,
                        MixFrom::Current => {}
                    }
                    return;
                }
                let mut a = curve_value1(&curves.frames, &curves.curves, time);
                if alpha != 1.0 {
                    let base = if from == MixFrom::Setup {
                        setup.color.a
                    } else {
                        c.a
                    };
                    a = base + (a - base) * alpha;
                }
                c.a = clamp01(a);
            });
        }
        Timeline::Rgba2 { slot, curves } => {
            color_timeline(sk, sd, *slot, applied, |pose, setup| {
                let sl = setup.color;
                let sdk = setup.dark_color.unwrap_or(Color::new(0.0, 0.0, 0.0, 0.0));
                if time < curves.frames[0] {
                    match from {
                        MixFrom::Setup => {
                            set_color(&mut pose.color, sl);
                            pose.dark_color.r = sdk.r;
                            pose.dark_color.g = sdk.g;
                            pose.dark_color.b = sdk.b;
                        }
                        MixFrom::First => {
                            let dl = delta(sl, pose.color, alpha);
                            add_color(&mut pose.color, dl);
                            let d = &mut pose.dark_color;
                            d.r += (sdk.r - d.r) * alpha;
                            d.g += (sdk.g - d.g) * alpha;
                            d.b += (sdk.b - d.b) * alpha;
                        }
                        MixFrom::Current => {}
                    }
                    return;
                }
                let (_, [r, g, b, a, mut r2, mut g2, mut b2]) =
                    curve_values::<7>(&curves.frames, &curves.curves, time, 8);
                if alpha == 1.0 {
                    set_color(&mut pose.color, Color::new(r, g, b, a));
                } else if from == MixFrom::Setup {
                    set_color(
                        &mut pose.color,
                        Color::new(
                            sl.r + (r - sl.r) * alpha,
                            sl.g + (g - sl.g) * alpha,
                            sl.b + (b - sl.b) * alpha,
                            sl.a + (a - sl.a) * alpha,
                        ),
                    );
                    r2 = sdk.r + (r2 - sdk.r) * alpha;
                    g2 = sdk.g + (g2 - sdk.g) * alpha;
                    b2 = sdk.b + (b2 - sdk.b) * alpha;
                } else {
                    let l = pose.color;
                    add_color(
                        &mut pose.color,
                        Color::new(
                            (r - l.r) * alpha,
                            (g - l.g) * alpha,
                            (b - l.b) * alpha,
                            (a - l.a) * alpha,
                        ),
                    );
                    let d = pose.dark_color;
                    r2 = d.r + (r2 - d.r) * alpha;
                    g2 = d.g + (g2 - d.g) * alpha;
                    b2 = d.b + (b2 - d.b) * alpha;
                }
                pose.dark_color.r = clamp01(r2);
                pose.dark_color.g = clamp01(g2);
                pose.dark_color.b = clamp01(b2);
            });
        }
        Timeline::Rgb2 { slot, curves } => color_timeline(sk, sd, *slot, applied, |pose, setup| {
            let sl = setup.color;
            let sdk = setup.dark_color.unwrap_or(Color::new(0.0, 0.0, 0.0, 0.0));
            let (l, d) = (&mut pose.color, &mut pose.dark_color);
            if time < curves.frames[0] {
                match from {
                    MixFrom::Setup => {
                        l.r = sl.r;
                        l.g = sl.g;
                        l.b = sl.b;
                        d.r = sdk.r;
                        d.g = sdk.g;
                        d.b = sdk.b;
                    }
                    MixFrom::First => {
                        l.r += (sl.r - l.r) * alpha;
                        l.g += (sl.g - l.g) * alpha;
                        l.b += (sl.b - l.b) * alpha;
                        d.r += (sdk.r - d.r) * alpha;
                        d.g += (sdk.g - d.g) * alpha;
                        d.b += (sdk.b - d.b) * alpha;
                    }
                    MixFrom::Current => {}
                }
                return;
            }
            let (_, [mut r, mut g, mut b, mut r2, mut g2, mut b2]) =
                curve_values::<6>(&curves.frames, &curves.curves, time, 7);
            if alpha != 1.0 {
                let (bl, bd) = if from == MixFrom::Setup {
                    (sl, sdk)
                } else {
                    (*l, *d)
                };
                r = bl.r + (r - bl.r) * alpha;
                g = bl.g + (g - bl.g) * alpha;
                b = bl.b + (b - bl.b) * alpha;
                r2 = bd.r + (r2 - bd.r) * alpha;
                g2 = bd.g + (g2 - bd.g) * alpha;
                b2 = bd.b + (b2 - bd.b) * alpha;
            }
            l.r = clamp01(r);
            l.g = clamp01(g);
            l.b = clamp01(b);
            d.r = clamp01(r2);
            d.g = clamp01(g2);
            d.b = clamp01(b2);
        }),

        Timeline::Attachment {
            slot, frames, keys, ..
        } => {
            let si = slot.index();
            if !sk.bones[sk.slots[si].bone.index()].active {
                return;
            }
            let name = if out || time < frames[0] {
                if from == MixFrom::Current {
                    return;
                }
                sd.slots[si].attachment_key
            } else {
                keys[search(frames, time, 1)]
            };
            set_attachment_by_key(sk, *slot, name, applied);
        }

        Timeline::Deform {
            slot,
            attachment,
            curves,
            vertices,
        } => apply_deform(
            sk,
            sd,
            *slot,
            *attachment,
            curves,
            vertices,
            time,
            alpha,
            from,
            add,
            applied,
        ),

        Timeline::Sequence {
            slot,
            attachment,
            frames,
        } => apply_sequence(sk, sd, *slot, *attachment, frames, time, from, out, applied),

        Timeline::DrawOrder {
            frames,
            draw_orders,
        } => {
            let slot_count = sk.slots.len();
            let pose = sk.draw_order.select_mut(applied);
            let setup_order = |pose: &mut Vec<SlotId>| {
                pose.clear();
                pose.extend((0..slot_count).map(|i| SlotId(i as u16)));
            };
            if out || time < frames[0] {
                if from != MixFrom::Current {
                    setup_order(pose);
                }
                return;
            }
            match &draw_orders[search(frames, time, 1)] {
                None => setup_order(pose),
                Some(order) => pose.clone_from(order),
            }
        }

        Timeline::DrawOrderFolder {
            slots,
            frames,
            draw_orders,
        } => {
            let pose = sk.draw_order.select_mut(applied);
            let order = if out || time < frames[0] {
                if from == MixFrom::Current {
                    return;
                }
                None
            } else {
                draw_orders[search(frames, time, 1)].as_deref()
            };
            // Folder slots keep their positions in the pose; only which
            // folder slot sits in each position changes.
            let mut found = 0;
            for entry in pose.iter_mut() {
                if found == slots.len() {
                    break;
                }
                if slots.contains(entry) {
                    *entry = match order {
                        None => slots[found],
                        Some(order) => slots[order[found] as usize],
                    };
                    found += 1;
                }
            }
        }

        Timeline::Event {
            frames,
            events: keys,
        } => {
            let Some(events) = events.as_deref_mut() else {
                return;
            };
            fire_events(frames, keys, last_time, time, events);
        }

        Timeline::IkConstraint { constraint, curves } => {
            let Some(Constraint::Ik(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_ik()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            let frames = &curves.frames;
            if time < frames[0] {
                match from {
                    MixFrom::Setup => *pose = setup,
                    MixFrom::First => {
                        pose.mix += (setup.mix - pose.mix) * alpha;
                        pose.softness += (setup.softness - pose.softness) * alpha;
                        pose.bend_direction = setup.bend_direction;
                        pose.compress = setup.compress;
                        pose.stretch = setup.stretch;
                    }
                    MixFrom::Current => {}
                }
                return;
            }
            let (i, [mix, softness]) = curve_values::<2>(frames, &curves.curves, time, 6);
            let base = if from == MixFrom::Setup { setup } else { *pose };
            pose.mix = base.mix + (mix - base.mix) * alpha;
            pose.softness = base.softness + (softness - base.softness) * alpha;
            if out {
                if from == MixFrom::Setup {
                    pose.bend_direction = base.bend_direction;
                    pose.compress = base.compress;
                    pose.stretch = base.stretch;
                }
            } else {
                pose.bend_direction = frames[i + 3] as i32;
                pose.compress = frames[i + 4] != 0.0;
                pose.stretch = frames[i + 5] != 0.0;
            }
        }

        Timeline::TransformConstraint { constraint, curves } => {
            let Some(Constraint::Transform(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_transform()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            let mut v = [
                &mut pose.mix_rotate,
                &mut pose.mix_x,
                &mut pose.mix_y,
                &mut pose.mix_scale_x,
                &mut pose.mix_scale_y,
                &mut pose.mix_shear_y,
            ];
            let s = [
                setup.mix_rotate,
                setup.mix_x,
                setup.mix_y,
                setup.mix_scale_x,
                setup.mix_scale_y,
                setup.mix_shear_y,
            ];
            mix_channels(
                &mut v,
                &s,
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
            );
        }

        Timeline::PathConstraintPosition { constraint, curves } => {
            let Some(Constraint::Path(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_path()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            pose.position = absolute_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                pose.position,
                setup.position,
                None,
            );
        }
        Timeline::PathConstraintSpacing { constraint, curves } => {
            let Some(Constraint::Path(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_path()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            pose.spacing = absolute_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                false,
                pose.spacing,
                setup.spacing,
                None,
            );
        }
        Timeline::PathConstraintMix { constraint, curves } => {
            let Some(Constraint::Path(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_path()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            let mut v = [&mut pose.mix_rotate, &mut pose.mix_x, &mut pose.mix_y];
            let s = [setup.mix_rotate, setup.mix_x, setup.mix_y];
            mix_channels(
                &mut v,
                &s,
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
            );
        }

        Timeline::Physics {
            constraint,
            property,
            curves,
        } => {
            let add = add && t.is_additive();
            match constraint {
                None => {
                    let value = if time >= curves.frames[0] {
                        curve_value1(&curves.frames, &curves.curves, time)
                    } else {
                        0.0
                    };
                    for i in 0..sk.physics.len() {
                        let id = sk.physics[i];
                        let data = sd.constraints[id.index()].as_physics().expect("kind");
                        if !sk.constraints_active[id.index()] || !physics_global(data, *property) {
                            continue;
                        }
                        let Constraint::Physics(c) = &mut sk.constraints[id.index()] else {
                            continue;
                        };
                        let pose = c.posed.select_mut(applied);
                        let v = absolute_value(
                            &curves.frames,
                            &curves.curves,
                            time,
                            alpha,
                            from,
                            add,
                            physics_get(pose, *property),
                            physics_get(&data.setup, *property),
                            Some(value),
                        );
                        physics_set(pose, *property, v);
                    }
                }
                Some(id) => {
                    let data = sd.constraints[id.index()].as_physics().expect("kind");
                    let Some(Constraint::Physics(c)) = active_constraint(sk, *id) else {
                        return;
                    };
                    let pose = c.posed.select_mut(applied);
                    let v = absolute_value(
                        &curves.frames,
                        &curves.curves,
                        time,
                        alpha,
                        from,
                        add,
                        physics_get(pose, *property),
                        physics_get(&data.setup, *property),
                        None,
                    );
                    physics_set(pose, *property, v);
                }
            }
        }
        Timeline::PhysicsReset { constraint, frames } => {
            physics_reset(sk, *constraint, frames, last_time, time);
        }

        Timeline::Slider { constraint, curves } => {
            let Some(Constraint::Slider(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_slider()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            pose.time = absolute_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                pose.time,
                setup.time,
                None,
            );
        }
        Timeline::SliderMix { constraint, curves } => {
            let Some(Constraint::Slider(c)) = active_constraint(sk, *constraint) else {
                return;
            };
            let setup = sd.constraints[constraint.index()]
                .as_slider()
                .expect("kind")
                .setup;
            let pose = c.posed.select_mut(applied);
            pose.mix = absolute_value(
                &curves.frames,
                &curves.curves,
                time,
                alpha,
                from,
                add,
                pose.mix,
                setup.mix,
                None,
            );
        }
    }
}

/// Runs `f` on an active bone's selected pose and its setup pose.
#[inline]
fn bone1(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    bone: BoneId,
    applied: bool,
    f: impl FnOnce(&mut crate::skeleton::BonePose, &crate::data::BoneLocal),
) {
    let b = &mut sk.bones[bone.index()];
    if b.active {
        f(b.posed.select_mut(applied), &sd.bones[bone.index()].setup);
    }
}

#[inline]
fn color_timeline(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    slot: SlotId,
    applied: bool,
    f: impl FnOnce(&mut SlotPose, &crate::data::SlotData),
) {
    let s = &mut sk.slots[slot.index()];
    if sk.bones[s.bone.index()].active {
        f(s.posed.select_mut(applied), &sd.slots[slot.index()]);
    }
}

#[inline]
fn active_constraint(sk: &mut Skeleton, id: ConstraintId) -> Option<&mut Constraint> {
    if sk.constraints_active[id.index()] {
        Some(&mut sk.constraints[id.index()])
    } else {
        None
    }
}

#[inline]
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Sets `c` to `v` clamped to `[0, 1]`, as spine-cpp's `Color::set` does.
#[inline]
fn set_color(c: &mut Color, v: Color) {
    c.r = clamp01(v.r);
    c.g = clamp01(v.g);
    c.b = clamp01(v.b);
    c.a = clamp01(v.a);
}

/// Adds `d` to `c`, clamped to `[0, 1]`.
#[inline]
fn add_color(c: &mut Color, d: Color) {
    set_color(c, Color::new(c.r + d.r, c.g + d.g, c.b + d.b, c.a + d.a));
}

#[inline]
fn delta(to: Color, from: Color, alpha: f32) -> Color {
    Color::new(
        (to.r - from.r) * alpha,
        (to.g - from.g) * alpha,
        (to.b - from.b) * alpha,
        (to.a - from.a) * alpha,
    )
}

/// Constraint mix timelines with `N` curve channels.
fn mix_channels<const N: usize>(
    v: &mut [&mut f32; N],
    setup: &[f32; N],
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    from: MixFrom,
    add: bool,
) {
    if time < frames[0] {
        match from {
            MixFrom::Setup => {
                for c in 0..N {
                    *v[c] = setup[c];
                }
            }
            MixFrom::First => {
                for c in 0..N {
                    *v[c] += (setup[c] - *v[c]) * alpha;
                }
            }
            MixFrom::Current => {}
        }
        return;
    }
    let (_, values) = curve_values::<N>(frames, curves, time, N + 1);
    for c in 0..N {
        let base = if from == MixFrom::Setup {
            setup[c]
        } else {
            *v[c]
        };
        *v[c] = if add {
            base + values[c] * alpha
        } else {
            base + (values[c] - base) * alpha
        };
    }
}

/// Sets the slot's attachment to the one `key` names in the skeleton's skin
/// or the default skin, or clears it when `key` is `None` or unresolved.
pub(crate) fn set_attachment_by_key(
    sk: &mut Skeleton,
    slot: SlotId,
    key: Option<SkinKey>,
    applied: bool,
) {
    let attachment = key.and_then(|k| sk.get_attachment_by_key(k));
    let timeline = sk.timeline_attachment(attachment);
    sk.slots[slot.index()]
        .posed
        .select_mut(applied)
        .set_attachment(attachment, timeline);
}

/// Pushes the events `t` keys in `(last_time, time]` when it's an event
/// timeline, without touching a skeleton.
pub(crate) fn timeline_events(t: &Timeline, last_time: f32, time: f32, events: &mut Vec<Event>) {
    if let Timeline::Event {
        frames,
        events: keys,
    } = t
    {
        fire_events(frames, keys, last_time, time, events);
    }
}

/// `EventTimeline::apply`: pushes events keyed in `(last_time, time]`.
fn fire_events(
    frames: &[f32],
    keys: &[crate::data::AnimationEvent],
    mut last_time: f32,
    time: f32,
    events: &mut Vec<Event>,
) {
    let frame_count = frames.len();
    if last_time > time {
        // Looped: fire the rest of the previous loop first.
        fire_events(frames, keys, last_time, f32::MAX, events);
        last_time = -1.0;
    } else if last_time >= frames[frame_count - 1] {
        return;
    }
    if time < frames[0] {
        return;
    }
    let mut i = if last_time < frames[0] {
        0
    } else {
        let mut i = search(frames, last_time, 1) + 1;
        let frame_time = frames[i];
        while i > 0 && frames[i - 1] == frame_time {
            i -= 1;
        }
        i
    };
    while i < frame_count && time >= frames[i] {
        let k = &keys[i];
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
}

fn physics_global(data: &PhysicsConstraintData, p: PhysicsProperty) -> bool {
    match p {
        PhysicsProperty::Inertia => data.inertia_global,
        PhysicsProperty::Strength => data.strength_global,
        PhysicsProperty::Damping => data.damping_global,
        PhysicsProperty::Mass => data.mass_global,
        PhysicsProperty::Wind => data.wind_global,
        PhysicsProperty::Gravity => data.gravity_global,
        PhysicsProperty::Mix => data.mix_global,
    }
}

fn physics_get(pose: &crate::data::PhysicsConstraintPose, p: PhysicsProperty) -> f32 {
    match p {
        PhysicsProperty::Inertia => pose.inertia,
        PhysicsProperty::Strength => pose.strength,
        PhysicsProperty::Damping => pose.damping,
        PhysicsProperty::Mass => 1.0 / pose.mass_inverse,
        PhysicsProperty::Wind => pose.wind,
        PhysicsProperty::Gravity => pose.gravity,
        PhysicsProperty::Mix => pose.mix,
    }
}

fn physics_set(pose: &mut crate::data::PhysicsConstraintPose, p: PhysicsProperty, v: f32) {
    match p {
        PhysicsProperty::Inertia => pose.inertia = v,
        PhysicsProperty::Strength => pose.strength = v,
        PhysicsProperty::Damping => pose.damping = v,
        PhysicsProperty::Mass => pose.mass_inverse = 1.0 / v,
        PhysicsProperty::Wind => pose.wind = v,
        PhysicsProperty::Gravity => pose.gravity = v,
        PhysicsProperty::Mix => pose.mix = v,
    }
}

fn physics_reset(
    sk: &mut Skeleton,
    constraint: Option<ConstraintId>,
    frames: &[f32],
    mut last_time: f32,
    time: f32,
) {
    if let Some(id) = constraint
        && !sk.constraints_active[id.index()]
    {
        return;
    }
    if last_time > time {
        physics_reset(sk, constraint, frames, last_time, f32::MAX);
        last_time = -1.0;
    } else if last_time >= frames[frames.len() - 1] {
        return;
    }
    if time < frames[0] {
        return;
    }
    if last_time < frames[0] || time >= frames[search(frames, last_time, 1) + 1] {
        let skeleton_time = sk.time;
        let reset = |c: &mut Constraint| {
            if let Constraint::Physics(p) = c {
                PhysicsConstraint::reset(p, skeleton_time);
            }
        };
        match constraint {
            Some(id) => reset(&mut sk.constraints[id.index()]),
            None => {
                for i in 0..sk.physics.len() {
                    let id = sk.physics[i];
                    if sk.constraints_active[id.index()] {
                        reset(&mut sk.constraints[id.index()]);
                    }
                }
            }
        }
    }
}

/// `Attachment::isTimelineActive`: whether the slot, or any slot the
/// attachment's timelines also drive, shows an attachment using them.
fn is_timeline_active(
    sk: &Skeleton,
    sd: &SkeletonData,
    attachment: AttachmentId,
    slot: SlotId,
    applied: bool,
) -> bool {
    let uses = |s: SlotId| {
        let slot = &sk.slots[s.index()];
        sk.bones[slot.bone.index()].active
            && slot.posed.select(applied).timeline_attachment == Some(attachment)
    };
    if uses(slot) {
        return true;
    }
    sd.attachments[attachment.index()]
        .timeline_link()
        .is_some_and(|l| l.slots.iter().any(|&s| uses(s)))
}

fn timeline_slots(sd: &SkeletonData, attachment: AttachmentId) -> &[SlotId] {
    sd.attachments[attachment.index()]
        .timeline_link()
        .map_or(&[], |l| l.slots.as_slice())
}

/// `DeformTimeline::getCurvePercent`.
fn deform_curve_percent(curves: &CurveFrames, frame: usize, time: f32) -> f32 {
    let frames = &curves.frames;
    let c = &curves.curves;
    let i = c[frame] as i32;
    if i == CURVE_LINEAR {
        let x = frames[frame];
        return (time - x) / (frames[frame + 1] - x);
    }
    if i == CURVE_STEPPED {
        return 0.0;
    }
    let mut j = (i - CURVE_BEZIER) as usize;
    if c[j] > time {
        let x = frames[frame];
        return c[j + 1] * (time - x) / (c[j] - x);
    }
    let n = j + BEZIER_SIZE;
    j += 2;
    while j < n {
        if c[j] >= time {
            let x = c[j - 2];
            let y = c[j - 1];
            return y + (time - x) / (c[j] - x) * (c[j + 1] - y);
        }
        j += 2;
    }
    let x = c[n - 2];
    let y = c[n - 1];
    y + (1.0 - y) * (time - x) / (frames[frame + 1] - x)
}

fn apply_deform(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    slot: SlotId,
    attachment: AttachmentId,
    curves: &CurveFrames,
    vertices: &[Vec<f32>],
    time: f32,
    alpha: f32,
    from: MixFrom,
    add: bool,
    applied: bool,
) {
    if !is_timeline_active(sk, sd, attachment, slot, applied) {
        return;
    }
    let frames = &curves.frames;
    let targets = std::iter::once(slot).chain(timeline_slots(sd, attachment).iter().copied());
    if time < frames[0] {
        for s in targets {
            deform_before_first(
                sk,
                sd,
                s,
                attachment,
                vertices[0].len(),
                alpha,
                from,
                applied,
            );
        }
        return;
    }
    let (v1, v2, percent) = if time >= frames[frames.len() - 1] {
        (&vertices[frames.len() - 1], None, 0.0)
    } else {
        let frame = search(frames, time, 1);
        (
            &vertices[frame],
            Some(&vertices[frame + 1]),
            deform_curve_percent(curves, frame, time),
        )
    };
    for s in targets {
        deform_slot(
            sk, sd, s, attachment, v1, v2, percent, alpha, from, add, applied,
        );
    }
}

/// The vertex attachment a slot shows, if its timelines are `attachment`'s.
fn deform_target<'a>(
    sk: &'a mut Skeleton,
    sd: &'a SkeletonData,
    slot: SlotId,
    attachment: AttachmentId,
    applied: bool,
) -> Option<(&'a mut SlotPose, &'a crate::data::VertexData)> {
    let s = &mut sk.slots[slot.index()];
    if !sk.bones[s.bone.index()].active {
        return None;
    }
    let pose = s.posed.select_mut(applied);
    if pose.timeline_attachment != Some(attachment) {
        return None;
    }
    let att: &Attachment = resolve(&sd.attachments, sk.skin.as_deref(), pose.attachment?);
    Some((pose, att.vertex_data()?))
}

fn deform_before_first(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    slot: SlotId,
    attachment: AttachmentId,
    vertex_count: usize,
    alpha: f32,
    mut from: MixFrom,
    applied: bool,
) {
    let Some((pose, vd)) = deform_target(sk, sd, slot, attachment, applied) else {
        return;
    };
    let deform = &mut pose.deform;
    if deform.is_empty() {
        from = MixFrom::Setup;
    }
    match from {
        MixFrom::Setup => deform.clear(),
        MixFrom::First => {
            if alpha == 1.0 {
                deform.clear();
                return;
            }
            deform.resize(vertex_count, 0.0);
            if vd.bones.is_empty() {
                for i in 0..vertex_count {
                    deform[i] += (vd.vertices[i] - deform[i]) * alpha;
                }
            } else {
                let a = 1.0 - alpha;
                for d in deform.iter_mut() {
                    *d *= a;
                }
            }
        }
        MixFrom::Current => {}
    }
}

/// `DeformTimeline::applyToSlot` / `applyToPose`.
fn deform_slot(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    slot: SlotId,
    attachment: AttachmentId,
    v1: &[f32],
    v2: Option<&Vec<f32>>,
    percent: f32,
    alpha: f32,
    mut from: MixFrom,
    add: bool,
    applied: bool,
) {
    let Some((pose, vd)) = deform_target(sk, sd, slot, attachment, applied) else {
        return;
    };
    let vertex_count = v1.len();
    let deform = &mut pose.deform;
    if deform.is_empty() {
        from = MixFrom::Setup;
    }
    let from_setup = from == MixFrom::Setup;
    deform.resize(vertex_count, 0.0);
    let unweighted = vd.bones.is_empty();
    let setup = &vd.vertices;
    let Some(v2) = v2 else {
        if alpha == 1.0 {
            if add && !from_setup {
                if unweighted {
                    for i in 0..vertex_count {
                        deform[i] += v1[i] - setup[i];
                    }
                } else {
                    for i in 0..vertex_count {
                        deform[i] += v1[i];
                    }
                }
            } else {
                deform.copy_from_slice(v1);
            }
        } else if from_setup {
            if unweighted {
                for i in 0..vertex_count {
                    let s = setup[i];
                    deform[i] = s + (v1[i] - s) * alpha;
                }
            } else {
                for i in 0..vertex_count {
                    deform[i] = v1[i] * alpha;
                }
            }
        } else if add {
            if unweighted {
                for i in 0..vertex_count {
                    deform[i] += (v1[i] - setup[i]) * alpha;
                }
            } else {
                for i in 0..vertex_count {
                    deform[i] += v1[i] * alpha;
                }
            }
        } else {
            for i in 0..vertex_count {
                deform[i] += (v1[i] - deform[i]) * alpha;
            }
        }
        return;
    };
    let lerp = |i: usize| v1[i] + (v2[i] - v1[i]) * percent;
    if alpha == 1.0 {
        if add && !from_setup {
            if unweighted {
                for i in 0..vertex_count {
                    deform[i] += lerp(i) - setup[i];
                }
            } else {
                for i in 0..vertex_count {
                    deform[i] += lerp(i);
                }
            }
        } else if percent == 0.0 {
            deform.copy_from_slice(v1);
        } else {
            for i in 0..vertex_count {
                deform[i] = lerp(i);
            }
        }
    } else if from_setup {
        if unweighted {
            for i in 0..vertex_count {
                let s = setup[i];
                deform[i] = s + (lerp(i) - s) * alpha;
            }
        } else {
            for i in 0..vertex_count {
                deform[i] = lerp(i) * alpha;
            }
        }
    } else if add {
        if unweighted {
            for i in 0..vertex_count {
                deform[i] += (lerp(i) - setup[i]) * alpha;
            }
        } else {
            for i in 0..vertex_count {
                deform[i] += lerp(i) * alpha;
            }
        }
    } else {
        for i in 0..vertex_count {
            deform[i] += (lerp(i) - deform[i]) * alpha;
        }
    }
}

fn apply_sequence(
    sk: &mut Skeleton,
    sd: &SkeletonData,
    slot: SlotId,
    attachment: AttachmentId,
    frames: &[f32],
    time: f32,
    from: MixFrom,
    out: bool,
    applied: bool,
) {
    if !is_timeline_active(sk, sd, attachment, slot, applied) {
        return;
    }
    let Some(sequence) = sd.attachments[attachment.index()].sequence() else {
        return;
    };
    let targets = std::iter::once(slot).chain(timeline_slots(sd, attachment).iter().copied());
    if out || time < frames[0] {
        if from != MixFrom::Current {
            for s in targets {
                if let Some(pose) = sequence_target(sk, s, attachment, applied) {
                    pose.sequence_index = -1;
                }
            }
        }
        return;
    }
    let i = search(frames, time, 3);
    let before = frames[i];
    let mode_and_index = frames[i + 1] as i32;
    let delay = frames[i + 2];
    let count = sequence.count() as i32;
    let mut index = mode_and_index >> 4;
    let mode = mode_and_index & 0xf;
    if mode != 0 {
        index += ((time - before) / delay + 0.0001) as i32;
        index = match mode {
            1 => (count - 1).min(index),
            2 => index % count,
            3 => {
                let n = (count << 1) - 2;
                let i = if n == 0 { 0 } else { index % n };
                if i >= count { n - i } else { i }
            }
            4 => (count - 1 - index).max(0),
            5 => count - 1 - (index % count),
            6 => {
                let n = (count << 1) - 2;
                let i = if n == 0 { 0 } else { (index + count - 1) % n };
                if i >= count { n - i } else { i }
            }
            _ => index,
        };
    }
    for s in targets {
        if let Some(pose) = sequence_target(sk, s, attachment, applied) {
            pose.sequence_index = index;
        }
    }
}

fn sequence_target(
    sk: &mut Skeleton,
    slot: SlotId,
    attachment: AttachmentId,
    applied: bool,
) -> Option<&mut SlotPose> {
    let s = &mut sk.slots[slot.index()];
    if !sk.bones[s.bone.index()].active {
        return None;
    }
    let pose = s.posed.select_mut(applied);
    (pose.timeline_attachment == Some(attachment)).then_some(pose)
}
