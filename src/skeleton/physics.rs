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

//! Physics constraint simulator. Port of
//! `spine-cpp/src/spine/PhysicsConstraint.cpp`.
//!
//! Damped-spring simulator with fixed-timestep integration. Per
//! constrained bone, drives a spring between the bone's anchor world
//! position and the "rest" target carried by the constraint's
//! offsets/velocities. Wind/gravity/inertia constants are encoded in
//! the data; `physics` (the Physics enum passed through apply)
//! selects whether the sim steps, resets, or just applies the current
//! pose.

#![allow(clippy::many_single_char_names)]

use crate::skeleton::{Physics, Skeleton};

/// Run the Physics constraint at `constraint_idx`. Ports
/// `spine::PhysicsConstraint::update`. No-op if the constraint is
/// inactive or has zero mix.
#[allow(clippy::too_many_lines)]
pub(crate) fn solve_physics_constraint(
    skeleton: &mut Skeleton,
    constraint_idx: usize,
    physics: Physics,
) {
    let (mix, data_idx, bone_id, active) = {
        let c = &skeleton.physics_constraints[constraint_idx];
        (c.mix, c.data_index, c.bone, c.active)
    };
    if !active || mix == 0.0 {
        return;
    }

    let (d_x, d_y, d_rotate, d_shear_x, d_scale_x, d_limit, d_step, _d_strength) = {
        let d = &skeleton.data.physics_constraints[data_idx.index()];
        (
            d.x, d.y, d.rotate, d.shear_x, d.scale_x, d.limit, d.step, d.strength,
        )
    };

    let x_active = d_x > 0.0;
    let y_active = d_y > 0.0;
    let rotate_or_shear_x = d_rotate > 0.0 || d_shear_x > 0.0;
    let scale_x_active = d_scale_x > 0.0;

    let bone_length = skeleton.data.bones[bone_id.index()].length;
    let reference_scale = skeleton.data.reference_scale;
    let sk_time = skeleton.time;
    let sk_scale_x = skeleton.scale_x;
    let sk_scale_y = skeleton.scale_y;

    let mut remaining = skeleton.physics_constraints[constraint_idx].remaining;
    let mut last_time = skeleton.physics_constraints[constraint_idx].last_time;

    match physics {
        Physics::None => return,
        Physics::Pose => {
            let c = &mut skeleton.physics_constraints[constraint_idx];
            let bone = &mut skeleton.bones[bone_id.index()];
            if x_active {
                bone.world_x += c.x_offset * mix * d_x;
            }
            if y_active {
                bone.world_y += c.y_offset * mix * d_y;
            }
        }
        Physics::Reset | Physics::Update => {
            // Reset branch: zero the sim then fall through to Update.
            if physics == Physics::Reset {
                let c = &mut skeleton.physics_constraints[constraint_idx];
                c.remaining = 0.0;
                c.last_time = sk_time;
                c.reset = true;
                c.x_offset = 0.0;
                c.x_velocity = 0.0;
                c.y_offset = 0.0;
                c.y_velocity = 0.0;
                c.rotate_offset = 0.0;
                c.rotate_velocity = 0.0;
                c.scale_offset = 0.0;
                c.scale_velocity = 0.0;
                remaining = 0.0;
                last_time = sk_time;
            }
            let delta = (sk_time - last_time).max(0.0);
            remaining += delta;
            {
                let c = &mut skeleton.physics_constraints[constraint_idx];
                c.last_time = sk_time;
                c.remaining = remaining;
            }

            let (bx, by) = {
                let b = &skeleton.bones[bone_id.index()];
                (b.world_x, b.world_y)
            };

            let need_reset = skeleton.physics_constraints[constraint_idx].reset;
            if need_reset {
                let c = &mut skeleton.physics_constraints[constraint_idx];
                c.reset = false;
                c.ux = bx;
                c.uy = by;
            } else {
                let mut a = remaining;
                let i_inertia = skeleton.physics_constraints[constraint_idx].inertia;
                let t_step = d_step;
                let f = reference_scale;
                let qx = d_limit * delta;
                let qy = qx * sk_scale_y.abs();
                let qx_effective = qx * sk_scale_x.abs();

                // Translation integration.
                if x_active || y_active {
                    if x_active {
                        let c = &mut skeleton.physics_constraints[constraint_idx];
                        let u = (c.ux - bx) * i_inertia;
                        let clamped = if u > qx_effective {
                            qx_effective
                        } else if u < -qx_effective {
                            -qx_effective
                        } else {
                            u
                        };
                        c.x_offset += clamped;
                        c.ux = bx;
                    }
                    if y_active {
                        let c = &mut skeleton.physics_constraints[constraint_idx];
                        let u = (c.uy - by) * i_inertia;
                        let clamped = if u > qy {
                            qy
                        } else if u < -qy {
                            -qy
                        } else {
                            u
                        };
                        c.y_offset += clamped;
                        c.uy = by;
                    }
                    if a >= t_step {
                        let c_snap = &skeleton.physics_constraints[constraint_idx];
                        let damping_base = c_snap.damping;
                        let mass = c_snap.mass_inverse;
                        let strength = c_snap.strength;
                        let wind = c_snap.wind;
                        let gravity = c_snap.gravity;
                        let d_pow = damping_base.powf(60.0 * t_step);
                        let m = mass * t_step;
                        let e = strength;
                        let w = wind * f * sk_scale_x;
                        let g = gravity * f * sk_scale_y;
                        while a >= t_step {
                            let c = &mut skeleton.physics_constraints[constraint_idx];
                            if x_active {
                                c.x_velocity += (w - c.x_offset * e) * m;
                                c.x_offset += c.x_velocity * t_step;
                                c.x_velocity *= d_pow;
                            }
                            if y_active {
                                c.y_velocity -= (g + c.y_offset * e) * m;
                                c.y_offset += c.y_velocity * t_step;
                                c.y_velocity *= d_pow;
                            }
                            a -= t_step;
                        }
                    }
                    if x_active {
                        let x_off = skeleton.physics_constraints[constraint_idx].x_offset;
                        skeleton.bones[bone_id.index()].world_x += x_off * mix * d_x;
                    }
                    if y_active {
                        let y_off = skeleton.physics_constraints[constraint_idx].y_offset;
                        skeleton.bones[bone_id.index()].world_y += y_off * mix * d_y;
                    }
                }

                // Rotation/shear/scale integration.
                if rotate_or_shear_x || scale_x_active {
                    let (ba, bc) = {
                        let b = &skeleton.bones[bone_id.index()];
                        (b.a, b.c)
                    };
                    let ca = bc.atan2(ba);
                    let (cx, cy, tx, ty) = {
                        let c = &skeleton.physics_constraints[constraint_idx];
                        (c.cx, c.cy, c.tx, c.ty)
                    };
                    let bx_now = skeleton.bones[bone_id.index()].world_x;
                    let by_now = skeleton.bones[bone_id.index()].world_y;
                    let mut dx = cx - bx_now;
                    let mut dy = cy - by_now;
                    if dx > qx_effective {
                        dx = qx_effective;
                    } else if dx < -qx_effective {
                        dx = -qx_effective;
                    }
                    if dy > qy {
                        dy = qy;
                    } else if dy < -qy {
                        dy = -qy;
                    }

                    let mut cos;
                    let mut sin;
                    let mut mr = 0.0_f32;
                    if rotate_or_shear_x {
                        mr = (d_rotate + d_shear_x) * mix;
                        let c = &mut skeleton.physics_constraints[constraint_idx];
                        let mut r = (dy + ty).atan2(dx + tx) - ca - c.rotate_offset * mr;
                        let two_pi = std::f32::consts::TAU;
                        let inv_2pi = 1.0 / two_pi;
                        // spine-cpp: `r - ceil(r * InvPi_2 - 0.5) * Pi_2`.
                        c.rotate_offset += (r - (r * inv_2pi - 0.5).ceil() * two_pi) * i_inertia;
                        r = c.rotate_offset * mr + ca;
                        cos = r.cos();
                        sin = r.sin();
                        if scale_x_active {
                            let world_scale_x = (ba * ba + bc * bc).sqrt();
                            let r_scale = bone_length * world_scale_x;
                            if r_scale > 0.0 {
                                c.scale_offset += (dx * cos + dy * sin) * i_inertia / r_scale;
                            }
                        }
                    } else {
                        cos = ca.cos();
                        sin = ca.sin();
                        let world_scale_x = (ba * ba + bc * bc).sqrt();
                        let r_scale = bone_length * world_scale_x;
                        if r_scale > 0.0 {
                            let c = &mut skeleton.physics_constraints[constraint_idx];
                            c.scale_offset += (dx * cos + dy * sin) * i_inertia / r_scale;
                        }
                    }

                    a = remaining;
                    if a >= t_step {
                        let c_snap = &skeleton.physics_constraints[constraint_idx];
                        let m = c_snap.mass_inverse * t_step;
                        let e = c_snap.strength;
                        let w = c_snap.wind;
                        let g_base = c_snap.gravity;
                        let damping_base = c_snap.damping;
                        // spine-cpp applies a y_down sign flip to gravity here
                        // (`Bone::yDown ? -1 : 1`); we default to y-up (false).
                        let g = g_base;
                        let h = if f == 0.0 { 0.0 } else { bone_length / f };
                        let d_pow = damping_base.powf(60.0 * t_step);
                        loop {
                            a -= t_step;
                            if scale_x_active {
                                let c = &mut skeleton.physics_constraints[constraint_idx];
                                c.scale_velocity += (w * cos - g * sin - c.scale_offset * e) * m;
                                c.scale_offset += c.scale_velocity * t_step;
                                c.scale_velocity *= d_pow;
                            }
                            if rotate_or_shear_x {
                                let c = &mut skeleton.physics_constraints[constraint_idx];
                                c.rotate_velocity -=
                                    ((w * sin + g * cos) * h + c.rotate_offset * e) * m;
                                c.rotate_offset += c.rotate_velocity * t_step;
                                c.rotate_velocity *= d_pow;
                                if a < t_step {
                                    break;
                                }
                                let r = c.rotate_offset * mr + ca;
                                cos = r.cos();
                                sin = r.sin();
                            } else if a < t_step {
                                break;
                            }
                        }
                    }
                    skeleton.physics_constraints[constraint_idx].remaining = a;
                }
            }

            // Cache the bone's position after the sim step so the next
            // frame's rotate/shear/scale branch can compute deltas.
            let c = &mut skeleton.physics_constraints[constraint_idx];
            let b = &skeleton.bones[bone_id.index()];
            c.cx = b.world_x;
            c.cy = b.world_y;
        }
    }

    // Apply rotate/shear to the world matrix.
    if rotate_or_shear_x {
        let c = &skeleton.physics_constraints[constraint_idx];
        let o = c.rotate_offset * mix;
        let bone = &mut skeleton.bones[bone_id.index()];
        if d_shear_x > 0.0 {
            let mut r = 0.0_f32;
            if d_rotate > 0.0 {
                r = o * d_rotate;
                let sin_r = r.sin();
                let cos_r = r.cos();
                let a = bone.b;
                bone.b = cos_r * a - sin_r * bone.d;
                bone.d = sin_r * a + cos_r * bone.d;
            }
            r += o * d_shear_x;
            let sin_r = r.sin();
            let cos_r = r.cos();
            let a = bone.a;
            bone.a = cos_r * a - sin_r * bone.c;
            bone.c = sin_r * a + cos_r * bone.c;
        } else {
            let o_r = o * d_rotate;
            let sin_r = o_r.sin();
            let cos_r = o_r.cos();
            let a = bone.a;
            bone.a = cos_r * a - sin_r * bone.c;
            bone.c = sin_r * a + cos_r * bone.c;
            let a = bone.b;
            bone.b = cos_r * a - sin_r * bone.d;
            bone.d = sin_r * a + cos_r * bone.d;
        }
    }
    if scale_x_active {
        let c = &skeleton.physics_constraints[constraint_idx];
        let s = 1.0 + c.scale_offset * mix * d_scale_x;
        let bone = &mut skeleton.bones[bone_id.index()];
        bone.a *= s;
        bone.c *= s;
    }
    if physics != Physics::Pose {
        let (ba, bc) = {
            let b = &skeleton.bones[bone_id.index()];
            (b.a, b.c)
        };
        let c = &mut skeleton.physics_constraints[constraint_idx];
        c.tx = bone_length * ba;
        c.ty = bone_length * bc;
    }
    skeleton.update_applied_transform(bone_id);
}
