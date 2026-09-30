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

//! Transform constraint (`TransformConstraint.cpp` and the from/to property
//! classes in `TransformConstraintData.cpp`).

use crate::data::{
    ConstraintId, TransformConstraintData, TransformConstraintPose, TransformProperty,
};
use crate::math::util::{DEG_RAD, PI, PI_2, RAD_DEG};
use crate::skeleton::bone::{self, BonePose, Frame};
use crate::skeleton::constraint::Constraint;
use crate::skeleton::{Skeleton, UpdateCacheEntry};

impl Skeleton {
    pub(crate) fn sort_transform(&mut self, id: ConstraintId, data: &TransformConstraintData) {
        if !data.local_source {
            self.sort_bone(data.source);
        }
        let world_target = !data.local_target;
        if world_target {
            for &b in &data.bones {
                self.sort_bone(b);
            }
        }
        self.update_cache.push(UpdateCacheEntry::Constraint(id));
        for &b in &data.bones {
            self.sort_reset(b);
            self.constrain_bone(b);
        }
        for &b in &data.bones {
            self.bones[b.index()].sorted = world_target;
        }
    }

    pub(crate) fn update_transform(&mut self, id: ConstraintId, data: &TransformConstraintData) {
        let Constraint::Transform(c) = &self.constraints[id.index()] else {
            unreachable!()
        };
        let p = *c.posed.applied();
        if p.mix_rotate == 0.0
            && p.mix_x == 0.0
            && p.mix_y == 0.0
            && p.mix_scale_x == 0.0
            && p.mix_scale_y == 0.0
            && p.mix_shear_y == 0.0
        {
            return;
        }
        let f = self.frame();
        let (local_source, local_target) = (data.local_source, data.local_target);
        if local_source {
            bone::validate_local_transform(&mut self.bones, data.source.index(), &f);
        }
        for &b in &data.bones {
            let bi = b.index();
            if local_target {
                bone::modify_local(&mut self.bones, bi, &f);
            } else {
                bone::modify_world(&mut self.bones, bi, &f);
            }
            let hoisted = *self.bones[data.source.index()].applied();
            for from in &data.properties {
                // A bone constrained to itself sees its own earlier writes.
                let source = if b == data.source {
                    *self.bones[bi].applied()
                } else {
                    hoisted
                };
                let value = from_value(from.property, &f, &source, local_source, &data.offsets)
                    - from.offset;
                for to in &from.to {
                    if to_mix(to.property, &p) != 0.0 {
                        let mut clamped = to.offset + value * to.scale;
                        if data.clamp {
                            clamped = if to.offset < to.max {
                                clamp(clamped, to.offset, to.max)
                            } else {
                                clamp(clamped, to.max, to.offset)
                            };
                        }
                        to_apply(
                            to.property,
                            &f,
                            &p,
                            self.bones[bi].applied_mut(),
                            clamped,
                            local_target,
                            data.additive,
                        );
                    }
                }
            }
        }
    }
}

/// `MathUtil::clamp`, which unlike `f32::clamp` tolerates `min > max`.
#[inline]
fn clamp(x: f32, min: f32, max: f32) -> f32 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

/// `FromProperty::value`: reads a channel from the source bone.
pub(crate) fn from_value(
    property: TransformProperty,
    f: &Frame,
    source: &BonePose,
    local: bool,
    offsets: &[f32; 6],
) -> f32 {
    let (sx, sy) = (f.scale_x, f.scale_y);
    let o = |p: TransformProperty| offsets[p.offset_index()];
    match property {
        TransformProperty::Rotate => {
            if local {
                return source.rotation + o(TransformProperty::Rotate);
            }
            let mut value = (source.c / sy).atan2(source.a / sx) * RAD_DEG
                + if (source.a * source.d - source.b * source.c) * sx * sy > 0.0 {
                    o(TransformProperty::Rotate)
                } else {
                    -o(TransformProperty::Rotate)
                };
            if value < 0.0 {
                value += 360.0;
            }
            value
        }
        TransformProperty::X => {
            if local {
                source.x + o(TransformProperty::X)
            } else {
                (o(TransformProperty::X) * source.a
                    + o(TransformProperty::Y) * source.b
                    + source.world_x)
                    / sx
            }
        }
        TransformProperty::Y => {
            if local {
                source.y + o(TransformProperty::Y)
            } else {
                (o(TransformProperty::X) * source.c
                    + o(TransformProperty::Y) * source.d
                    + source.world_y)
                    / sy
            }
        }
        TransformProperty::ScaleX => {
            if local {
                return source.scale_x + o(TransformProperty::ScaleX);
            }
            let a = source.a / sx;
            let c = source.c / sy;
            (a * a + c * c).sqrt() + o(TransformProperty::ScaleX)
        }
        TransformProperty::ScaleY => {
            if local {
                return source.scale_y + o(TransformProperty::ScaleY);
            }
            let b = source.b / sx;
            let d = source.d / sy;
            (b * b + d * d).sqrt() + o(TransformProperty::ScaleY)
        }
        TransformProperty::ShearY => {
            if local {
                return source.shear_y + o(TransformProperty::ShearY);
            }
            let ix = 1.0 / sx;
            let iy = 1.0 / sy;
            ((source.d * iy).atan2(source.b * ix) - (source.c * iy).atan2(source.a * ix)) * RAD_DEG
                - 90.0
                + o(TransformProperty::ShearY)
        }
    }
}

fn to_mix(property: TransformProperty, p: &TransformConstraintPose) -> f32 {
    match property {
        TransformProperty::Rotate => p.mix_rotate,
        TransformProperty::X => p.mix_x,
        TransformProperty::Y => p.mix_y,
        TransformProperty::ScaleX => p.mix_scale_x,
        TransformProperty::ScaleY => p.mix_scale_y,
        TransformProperty::ShearY => p.mix_shear_y,
    }
}

/// `ToProperty::apply`: writes a channel to a constrained bone.
#[allow(clippy::many_single_char_names)]
fn to_apply(
    property: TransformProperty,
    f: &Frame,
    p: &TransformConstraintPose,
    bone: &mut BonePose,
    mut value: f32,
    local: bool,
    additive: bool,
) {
    let (sx, sy) = (f.scale_x, f.scale_y);
    match property {
        TransformProperty::Rotate => {
            if local {
                bone.rotation += (if additive {
                    value
                } else {
                    value - bone.rotation
                }) * p.mix_rotate;
            } else {
                let (ix, iy) = (1.0 / sx, 1.0 / sy);
                let (a, b, c, d) = (bone.a * ix, bone.b * ix, bone.c * iy, bone.d * iy);
                value *= DEG_RAD;
                if !additive {
                    value -= c.atan2(a);
                }
                if value > PI {
                    value -= PI_2;
                } else if value < -PI {
                    value += PI_2;
                }
                value *= p.mix_rotate;
                let (cos, sin) = (value.cos(), value.sin());
                bone.a = (cos * a - sin * c) * sx;
                bone.b = (cos * b - sin * d) * sx;
                bone.c = (sin * a + cos * c) * sy;
                bone.d = (sin * b + cos * d) * sy;
            }
        }
        TransformProperty::X => {
            if local {
                bone.x += (if additive { value } else { value - bone.x }) * p.mix_x;
            } else {
                if !additive {
                    value -= bone.world_x / sx;
                }
                bone.world_x += value * p.mix_x * sx;
            }
        }
        TransformProperty::Y => {
            if local {
                bone.y += (if additive { value } else { value - bone.y }) * p.mix_y;
            } else {
                if !additive {
                    value -= bone.world_y / sy;
                }
                bone.world_y += value * p.mix_y * sy;
            }
        }
        TransformProperty::ScaleX => {
            if local {
                if additive {
                    bone.scale_x *= 1.0 + (value - 1.0) * p.mix_scale_x;
                } else if bone.scale_x != 0.0 {
                    bone.scale_x += (value - bone.scale_x) * p.mix_scale_x;
                }
            } else if additive {
                let s = 1.0 + (value - 1.0) * p.mix_scale_x;
                bone.a *= s;
                bone.c *= s;
            } else {
                let a = bone.a / sx;
                let c = bone.c / sy;
                let mut s = (a * a + c * c).sqrt();
                if s != 0.0 {
                    s = 1.0 + (value - s) * p.mix_scale_x / s;
                    bone.a *= s;
                    bone.c *= s;
                }
            }
        }
        TransformProperty::ScaleY => {
            if local {
                if additive {
                    bone.scale_y *= 1.0 + (value - 1.0) * p.mix_scale_y;
                } else if bone.scale_y != 0.0 {
                    bone.scale_y += (value - bone.scale_y) * p.mix_scale_y;
                }
            } else if additive {
                let s = 1.0 + (value - 1.0) * p.mix_scale_y;
                bone.b *= s;
                bone.d *= s;
            } else {
                let b = bone.b / sx;
                let d = bone.d / sy;
                let mut s = (b * b + d * d).sqrt();
                if s != 0.0 {
                    s = 1.0 + (value - s) * p.mix_scale_y / s;
                    bone.b *= s;
                    bone.d *= s;
                }
            }
        }
        TransformProperty::ShearY => {
            if local {
                if !additive {
                    value -= bone.shear_y;
                }
                bone.shear_y += value * p.mix_shear_y;
            } else {
                let b = bone.b / sx;
                let d = bone.d / sy;
                let by = d.atan2(b);
                value = (value + 90.0) * DEG_RAD;
                if additive {
                    value -= PI / 2.0;
                } else {
                    value -= by - (bone.c / sy).atan2(bone.a / sx);
                    if value > PI {
                        value -= PI_2;
                    } else if value < -PI {
                        value += PI_2;
                    }
                }
                value = by + value * p.mix_shear_y;
                let s = (b * b + d * d).sqrt();
                bone.b = value.cos() * s * sx;
                bone.d = value.sin() * s * sy;
            }
        }
    }
}
