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

//! Physics constraint (`PhysicsConstraint.cpp`): a damped spring on a
//! fixed timestep, applied to one bone's world transform.

#![allow(clippy::many_single_char_names, clippy::similar_names)]

use crate::data::{ConstraintId, PhysicsConstraintData, ScaleYMode};
use crate::math::util::{INV_PI_2, PI_2};
use crate::skeleton::bone;
use crate::skeleton::constraint::Constraint;
use crate::skeleton::{Physics, Skeleton, UpdateCacheEntry};

impl Skeleton {
    pub(crate) fn sort_physics(&mut self, id: ConstraintId, data: &PhysicsConstraintData) {
        let b = data.bone;
        self.sort_bone(b);
        self.update_cache.push(UpdateCacheEntry::Constraint(id));
        self.sort_reset(b);
        self.constrain_bone(b);
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn update_physics(
        &mut self,
        id: ConstraintId,
        data: &PhysicsConstraintData,
        physics: Physics,
    ) {
        let bi = data.bone.index();
        let l = self.data.bones[bi].length;
        let reference_scale = self.data.reference_scale;
        let f = self.frame();
        let (time, wind_x, wind_y, gravity_x, gravity_y) = (
            self.time,
            self.wind_x,
            self.wind_y,
            self.gravity_x,
            self.gravity_y,
        );
        let Constraint::Physics(c) = &mut self.constraints[id.index()] else {
            unreachable!()
        };
        let p = *c.posed.applied();
        let mix = p.mix;
        if mix == 0.0 {
            return;
        }
        let x = data.x > 0.0;
        let y = data.y > 0.0;
        let rotate_or_shear_x = data.rotate > 0.0 || data.shear_x > 0.0;
        let scale_x = data.scale_x > 0.0;
        let t = data.step;
        let mut z = 0.0;
        if physics == Physics::None {
            return;
        }
        bone::modify_world(&mut self.bones, bi, &f);
        let bone = self.bones[bi].applied_mut();

        match physics {
            Physics::Reset | Physics::Update => {
                if physics == Physics::Reset {
                    c.reset(time);
                }
                let delta = (time - c.last_time).max(0.0);
                let aa = c.remaining;
                c.remaining += delta;
                c.last_time = time;
                let (bx, by) = (bone.world_x, bone.world_y);
                if c.reset {
                    c.reset = false;
                    c.ux = bx;
                    c.uy = by;
                } else {
                    let mut a = c.remaining;
                    let i = p.inertia;
                    let fs = reference_scale;
                    let mut d = -1.0;
                    let mut m = 0.0;
                    let mut e = 0.0;
                    let mut qx = data.limit * delta;
                    let qy = qx * f.scale_y.abs();
                    qx *= f.scale_x.abs();
                    if x || y {
                        if x {
                            let u = (c.ux - bx) * i;
                            c.x_offset += if u > qx {
                                qx
                            } else if u < -qx {
                                -qx
                            } else {
                                u
                            };
                            c.ux = bx;
                        }
                        if y {
                            let u = (c.uy - by) * i;
                            c.y_offset += if u > qy {
                                qy
                            } else if u < -qy {
                                -qy
                            } else {
                                u
                            };
                            c.uy = by;
                        }
                        if a >= t {
                            let (xs, ys) = (c.x_offset, c.y_offset);
                            d = p.damping.powf(60.0 * t);
                            m = t * p.mass_inverse;
                            e = p.strength;
                            let w = fs * p.wind;
                            let g = fs * p.gravity;
                            let ax = (w * wind_x + g * gravity_x) * f.scale_x;
                            let ay = (w * wind_y + g * gravity_y) * f.scale_y;
                            loop {
                                if x {
                                    c.x_velocity += (ax - c.x_offset * e) * m;
                                    c.x_offset += c.x_velocity * t;
                                    c.x_velocity *= d;
                                }
                                if y {
                                    c.y_velocity -= (ay + c.y_offset * e) * m;
                                    c.y_offset += c.y_velocity * t;
                                    c.y_velocity *= d;
                                }
                                a -= t;
                                if a < t {
                                    break;
                                }
                            }
                            c.x_lag = c.x_offset - xs;
                            c.y_lag = c.y_offset - ys;
                        }
                        z = (1.0 - a / t).max(0.0);
                        if x {
                            bone.world_x += (c.x_offset - c.x_lag * z) * mix * data.x;
                        }
                        if y {
                            bone.world_y += (c.y_offset - c.y_lag * z) * mix * data.y;
                        }
                    }
                    if rotate_or_shear_x || scale_x {
                        let ca = bone.c.atan2(bone.a);
                        let (mut cc, mut s);
                        let mut mr = 0.0;
                        let mut dx = c.cx - bone.world_x;
                        let mut dy = c.cy - bone.world_y;
                        if dx > qx {
                            dx = qx;
                        } else if dx < -qx {
                            dx = -qx;
                        }
                        if dy > qy {
                            dy = qy;
                        } else if dy < -qy {
                            dy = -qy;
                        }
                        if rotate_or_shear_x {
                            mr = (data.rotate + data.shear_x) * mix;
                            z = c.rotate_lag * (1.0 - aa / t).max(0.0);
                            let mut r =
                                (dy + c.ty).atan2(dx + c.tx) - ca - (c.rotate_offset - z) * mr;
                            c.rotate_offset += (r - (r * INV_PI_2 - 0.5).ceil() * PI_2) * i;
                            r = (c.rotate_offset - z) * mr + ca;
                            cc = r.cos();
                            s = r.sin();
                            if scale_x {
                                r = l * bone.world_scale_x();
                                if r > 0.0 {
                                    c.scale_offset += (dx * cc + dy * s) * i / r;
                                }
                            }
                        } else {
                            cc = ca.cos();
                            s = ca.sin();
                            let r =
                                l * bone.world_scale_x() - c.scale_lag * (1.0 - aa / t).max(0.0);
                            if r > 0.0 {
                                c.scale_offset += (dx * cc + dy * s) * i / r;
                            }
                        }
                        a = c.remaining;
                        if a >= t {
                            if d == -1.0 {
                                d = p.damping.powf(60.0 * t);
                                m = t * p.mass_inverse;
                                e = p.strength;
                            }
                            let ax = p.wind * wind_x + p.gravity * gravity_x;
                            let ay = p.wind * wind_y + p.gravity * gravity_y;
                            let rs = c.rotate_offset;
                            let ss = c.scale_offset;
                            let h = l / fs;
                            loop {
                                a -= t;
                                if scale_x {
                                    c.scale_velocity += (ax * cc - ay * s - c.scale_offset * e) * m;
                                    c.scale_offset += c.scale_velocity * t;
                                    c.scale_velocity *= d;
                                }
                                if rotate_or_shear_x {
                                    c.rotate_velocity -=
                                        ((ax * s + ay * cc) * h + c.rotate_offset * e) * m;
                                    c.rotate_offset += c.rotate_velocity * t;
                                    c.rotate_velocity *= d;
                                    if a < t {
                                        break;
                                    }
                                    let r = c.rotate_offset * mr + ca;
                                    cc = r.cos();
                                    s = r.sin();
                                } else if a < t {
                                    break;
                                }
                            }
                            c.rotate_lag = c.rotate_offset - rs;
                            c.scale_lag = c.scale_offset - ss;
                        }
                        z = (1.0 - a / t).max(0.0);
                    }
                    c.remaining = a;
                }
                c.cx = bone.world_x;
                c.cy = bone.world_y;
            }
            Physics::Pose => {
                z = (1.0 - c.remaining / t).max(0.0);
                if x {
                    bone.world_x += (c.x_offset - c.x_lag * z) * mix * data.x;
                }
                if y {
                    bone.world_y += (c.y_offset - c.y_lag * z) * mix * data.y;
                }
            }
            Physics::None => unreachable!(),
        }
        if rotate_or_shear_x {
            let mut o = (c.rotate_offset - c.rotate_lag * z) * mix;
            if data.shear_x > 0.0 {
                let mut r = 0.0;
                if data.rotate > 0.0 {
                    r = o * data.rotate;
                    let (s, cc) = (r.sin(), r.cos());
                    let a = bone.b;
                    bone.b = cc * a - s * bone.d;
                    bone.d = s * a + cc * bone.d;
                }
                r += o * data.shear_x;
                let (s, cc) = (r.sin(), r.cos());
                let a = bone.a;
                bone.a = cc * a - s * bone.c;
                bone.c = s * a + cc * bone.c;
            } else {
                o *= data.rotate;
                let (s, cc) = (o.sin(), o.cos());
                let a = bone.a;
                bone.a = cc * a - s * bone.c;
                bone.c = s * a + cc * bone.c;
                let a = bone.b;
                bone.b = cc * a - s * bone.d;
                bone.d = s * a + cc * bone.d;
            }
        }
        if scale_x {
            let mut s = 1.0 + (c.scale_offset - c.scale_lag * z) * mix * data.scale_x;
            bone.a *= s;
            bone.c *= s;
            match data.scale_y_mode {
                ScaleYMode::Uniform => {
                    bone.b *= s;
                    bone.d *= s;
                }
                ScaleYMode::Volume => {
                    s = s.abs();
                    s = if s >= 0.7 {
                        1.0 / s
                    } else {
                        4.0 - 3.673_47 * s
                    };
                    bone.b *= s;
                    bone.d *= s;
                }
                ScaleYMode::None => {}
            }
        }
        if physics != Physics::Pose {
            c.tx = l * bone.a;
            c.ty = l * bone.c;
        }
    }
}
