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

//! Transform constraint solver. Ports
//! `spine-cpp/src/spine/TransformConstraint.cpp` — four apply variants
//! discriminated by `data.local` and `data.relative`:
//!
//! - `applyAbsoluteWorld` (default): blend target's world TRS into bone's
//!   world TRS, then rebuild applied local via [`update_applied_transform`].
//! - `applyRelativeWorld`: additive version — target's values blend onto
//!   bone's current world TRS rather than overwriting.
//! - `applyAbsoluteLocal` / `applyRelativeLocal`: operate on applied local
//!   values and then call `update_bone_world_transform_with` to rebuild world.

#![allow(clippy::many_single_char_names)]

use crate::data::BoneId;
use crate::skeleton::Skeleton;

/// Run the Transform constraint at `constraint_idx`. Matches
/// `spine::TransformConstraint::update`.
pub(crate) fn solve_transform_constraint(skeleton: &mut Skeleton, constraint_idx: usize) {
    let (active, mix_rotate, mix_x, mix_y, mix_scale_x, mix_scale_y, mix_shear_y, data_idx) = {
        let c = &skeleton.transform_constraints[constraint_idx];
        (
            c.active,
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.mix_scale_x,
            c.mix_scale_y,
            c.mix_shear_y,
            c.data_index,
        )
    };
    if !active {
        return;
    }
    if mix_rotate == 0.0
        && mix_x == 0.0
        && mix_y == 0.0
        && mix_scale_x == 0.0
        && mix_scale_y == 0.0
        && mix_shear_y == 0.0
    {
        return;
    }
    let (is_local, is_relative) = {
        let d = &skeleton.data.transform_constraints[data_idx.index()];
        (d.local, d.relative)
    };
    match (is_local, is_relative) {
        (false, false) => apply_absolute_world(skeleton, constraint_idx),
        (false, true) => apply_relative_world(skeleton, constraint_idx),
        (true, false) => apply_absolute_local(skeleton, constraint_idx),
        (true, true) => apply_relative_local(skeleton, constraint_idx),
    }
}

#[allow(clippy::too_many_lines)]
fn apply_absolute_world(skeleton: &mut Skeleton, idx: usize) {
    let (mix_rotate, mix_x, mix_y, mix_scale_x, mix_scale_y, mix_shear_y, bones, target, data_idx) = {
        let c = &skeleton.transform_constraints[idx];
        (
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.mix_scale_x,
            c.mix_scale_y,
            c.mix_shear_y,
            c.bones.clone(),
            c.target,
            c.data_index,
        )
    };
    let (offset_rotation, offset_x, offset_y, offset_scale_x, offset_scale_y, offset_shear_y) = {
        let d = &skeleton.data.transform_constraints[data_idx.index()];
        (
            d.offset_rotation,
            d.offset_x,
            d.offset_y,
            d.offset_scale_x,
            d.offset_scale_y,
            d.offset_shear_y,
        )
    };

    let translate = mix_x != 0.0 || mix_y != 0.0;
    let (ta, tb, tc, td) = {
        let t = &skeleton.bones[target.index()];
        (t.a, t.b, t.c, t.d)
    };
    let deg_rad = std::f32::consts::PI / 180.0;
    let deg_rad_reflect = if ta * td - tb * tc > 0.0 {
        deg_rad
    } else {
        -deg_rad
    };
    let offset_rotation_rad = offset_rotation * deg_rad_reflect;
    let offset_shear_y_rad = offset_shear_y * deg_rad_reflect;

    for bone_id in bones {
        let (mut a, mut b, mut c, mut d, mut world_x, mut world_y) = {
            let bone = &skeleton.bones[bone_id.index()];
            (bone.a, bone.b, bone.c, bone.d, bone.world_x, bone.world_y)
        };

        if mix_rotate != 0.0 {
            let mut r = tc.atan2(ta) - c.atan2(a) + offset_rotation_rad;
            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            r *= mix_rotate;
            let cos = r.cos();
            let sin = r.sin();
            let new_a = cos * a - sin * c;
            let new_b = cos * b - sin * d;
            let new_c = sin * a + cos * c;
            let new_d = sin * b + cos * d;
            a = new_a;
            b = new_b;
            c = new_c;
            d = new_d;
        }

        if translate {
            let (tx, ty) = skeleton.bone_local_to_world(target, offset_x, offset_y);
            world_x += (tx - world_x) * mix_x;
            world_y += (ty - world_y) * mix_y;
        }

        if mix_scale_x > 0.0 {
            let mut s = (a * a + c * c).sqrt();
            if s != 0.0 {
                s = (s + ((ta * ta + tc * tc).sqrt() - s + offset_scale_x) * mix_scale_x) / s;
            }
            a *= s;
            c *= s;
        }
        if mix_scale_y > 0.0 {
            let mut s = (b * b + d * d).sqrt();
            if s != 0.0 {
                s = (s + ((tb * tb + td * td).sqrt() - s + offset_scale_y) * mix_scale_y) / s;
            }
            b *= s;
            d *= s;
        }

        if mix_shear_y > 0.0 {
            let by = d.atan2(b);
            let mut r = td.atan2(tb) - tc.atan2(ta) - (by - c.atan2(a));
            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            let r_new = by + (r + offset_shear_y_rad) * mix_shear_y;
            let s = (b * b + d * d).sqrt();
            b = r_new.cos() * s;
            d = r_new.sin() * s;
        }

        // Write world fields back then rebuild applied local.
        {
            let bone = &mut skeleton.bones[bone_id.index()];
            bone.a = a;
            bone.b = b;
            bone.c = c;
            bone.d = d;
            bone.world_x = world_x;
            bone.world_y = world_y;
        }
        skeleton.update_applied_transform(bone_id);
    }
}

#[allow(clippy::too_many_lines)]
fn apply_relative_world(skeleton: &mut Skeleton, idx: usize) {
    let (mix_rotate, mix_x, mix_y, mix_scale_x, mix_scale_y, mix_shear_y, bones, target, data_idx) = {
        let c = &skeleton.transform_constraints[idx];
        (
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.mix_scale_x,
            c.mix_scale_y,
            c.mix_shear_y,
            c.bones.clone(),
            c.target,
            c.data_index,
        )
    };
    let (offset_rotation, offset_x, offset_y, offset_scale_x, offset_scale_y, offset_shear_y) = {
        let d = &skeleton.data.transform_constraints[data_idx.index()];
        (
            d.offset_rotation,
            d.offset_x,
            d.offset_y,
            d.offset_scale_x,
            d.offset_scale_y,
            d.offset_shear_y,
        )
    };

    let translate = mix_x != 0.0 || mix_y != 0.0;
    let (ta, tb, tc, td) = {
        let t = &skeleton.bones[target.index()];
        (t.a, t.b, t.c, t.d)
    };
    let deg_rad = std::f32::consts::PI / 180.0;
    let deg_rad_reflect = if ta * td - tb * tc > 0.0 {
        deg_rad
    } else {
        -deg_rad
    };
    let offset_rotation_rad = offset_rotation * deg_rad_reflect;
    let offset_shear_y_rad = offset_shear_y * deg_rad_reflect;

    for bone_id in bones {
        let (mut a, mut b, mut c, mut d, mut world_x, mut world_y) = {
            let bone = &skeleton.bones[bone_id.index()];
            (bone.a, bone.b, bone.c, bone.d, bone.world_x, bone.world_y)
        };

        if mix_rotate != 0.0 {
            let mut r = tc.atan2(ta) + offset_rotation_rad;
            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            r *= mix_rotate;
            let cos = r.cos();
            let sin = r.sin();
            let new_a = cos * a - sin * c;
            let new_b = cos * b - sin * d;
            let new_c = sin * a + cos * c;
            let new_d = sin * b + cos * d;
            a = new_a;
            b = new_b;
            c = new_c;
            d = new_d;
        }

        if translate {
            let (tx, ty) = skeleton.bone_local_to_world(target, offset_x, offset_y);
            world_x += tx * mix_x;
            world_y += ty * mix_y;
        }

        if mix_scale_x != 0.0 {
            let s = ((ta * ta + tc * tc).sqrt() - 1.0 + offset_scale_x) * mix_scale_x + 1.0;
            a *= s;
            c *= s;
        }
        if mix_scale_y != 0.0 {
            let s = ((tb * tb + td * td).sqrt() - 1.0 + offset_scale_y) * mix_scale_y + 1.0;
            b *= s;
            d *= s;
        }

        if mix_shear_y > 0.0 {
            let mut r = td.atan2(tb) - tc.atan2(ta);
            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            let r_new =
                d.atan2(b) + (r - std::f32::consts::FRAC_PI_2 + offset_shear_y_rad) * mix_shear_y;
            let s = (b * b + d * d).sqrt();
            b = r_new.cos() * s;
            d = r_new.sin() * s;
        }

        {
            let bone = &mut skeleton.bones[bone_id.index()];
            bone.a = a;
            bone.b = b;
            bone.c = c;
            bone.d = d;
            bone.world_x = world_x;
            bone.world_y = world_y;
        }
        skeleton.update_applied_transform(bone_id);
    }
}

fn apply_absolute_local(skeleton: &mut Skeleton, idx: usize) {
    let (mix_rotate, mix_x, mix_y, mix_scale_x, mix_scale_y, mix_shear_y, bones, target, data_idx) = {
        let c = &skeleton.transform_constraints[idx];
        (
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.mix_scale_x,
            c.mix_scale_y,
            c.mix_shear_y,
            c.bones.clone(),
            c.target,
            c.data_index,
        )
    };
    let (offset_rotation, offset_x, offset_y, offset_scale_x, offset_scale_y, offset_shear_y) = {
        let d = &skeleton.data.transform_constraints[data_idx.index()];
        (
            d.offset_rotation,
            d.offset_x,
            d.offset_y,
            d.offset_scale_x,
            d.offset_scale_y,
            d.offset_shear_y,
        )
    };

    let (t_ax, t_ay, t_arotation, t_ascale_x, t_ascale_y, t_ashear_y) = {
        let t = &skeleton.bones[target.index()];
        (
            t.ax,
            t.ay,
            t.a_rotation,
            t.a_scale_x,
            t.a_scale_y,
            t.a_shear_y,
        )
    };

    for bone_id in bones {
        let (ax, ay, a_rotation, a_scale_x, a_scale_y, a_shear_x, a_shear_y) = {
            let bone = &skeleton.bones[bone_id.index()];
            (
                bone.ax,
                bone.ay,
                bone.a_rotation,
                bone.a_scale_x,
                bone.a_scale_y,
                bone.a_shear_x,
                bone.a_shear_y,
            )
        };

        let mut rotation = a_rotation;
        if mix_rotate != 0.0 {
            let mut r = t_arotation - rotation + offset_rotation;
            r -= (r / 360.0 - 0.5).ceil() * 360.0;
            rotation += r * mix_rotate;
        }
        let x = ax + (t_ax - ax + offset_x) * mix_x;
        let y = ay + (t_ay - ay + offset_y) * mix_y;

        let mut scale_x = a_scale_x;
        let mut scale_y = a_scale_y;
        if mix_scale_x != 0.0 && scale_x != 0.0 {
            scale_x = (scale_x + (t_ascale_x - scale_x + offset_scale_x) * mix_scale_x) / scale_x;
        }
        if mix_scale_y != 0.0 && scale_y != 0.0 {
            scale_y = (scale_y + (t_ascale_y - scale_y + offset_scale_y) * mix_scale_y) / scale_y;
        }

        let shear_y = a_shear_y;
        if mix_shear_y != 0.0 {
            let mut r = t_ashear_y - shear_y + offset_shear_y;
            r -= (r / 360.0 - 0.5).ceil() * 360.0;
            // spine-cpp writes into `bone._shearY` here (the local, not
            // applied). Replicate that.
            skeleton.bones[bone_id.index()].shear_y += r * mix_shear_y;
        }
        let _ = shear_y; // kept for parity with spine-cpp's local binding

        skeleton.update_bone_world_transform_with(
            bone_id, x, y, rotation, scale_x, scale_y, a_shear_x, a_shear_y,
        );
        // Re-read bone.shear_y into shear_y for the call signature: spine-cpp
        // passes `bone._ashearY` (cached) to the call, not the just-written
        // `_shearY`. We've done the same above; the line `bone._shearY += ...`
        // mutates next-frame local, not this-frame applied.
    }
}

fn apply_relative_local(skeleton: &mut Skeleton, idx: usize) {
    let (mix_rotate, mix_x, mix_y, mix_scale_x, mix_scale_y, mix_shear_y, bones, target, data_idx) = {
        let c = &skeleton.transform_constraints[idx];
        (
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.mix_scale_x,
            c.mix_scale_y,
            c.mix_shear_y,
            c.bones.clone(),
            c.target,
            c.data_index,
        )
    };
    let (offset_rotation, offset_x, offset_y, offset_scale_x, offset_scale_y, offset_shear_y) = {
        let d = &skeleton.data.transform_constraints[data_idx.index()];
        (
            d.offset_rotation,
            d.offset_x,
            d.offset_y,
            d.offset_scale_x,
            d.offset_scale_y,
            d.offset_shear_y,
        )
    };

    let (t_ax, t_ay, t_arotation, t_ascale_x, t_ascale_y, t_ashear_y) = {
        let t = &skeleton.bones[target.index()];
        (
            t.ax,
            t.ay,
            t.a_rotation,
            t.a_scale_x,
            t.a_scale_y,
            t.a_shear_y,
        )
    };

    for bone_id in bones {
        let (ax, ay, a_rotation, a_scale_x, a_scale_y, a_shear_x, a_shear_y) = {
            let bone = &skeleton.bones[bone_id.index()];
            (
                bone.ax,
                bone.ay,
                bone.a_rotation,
                bone.a_scale_x,
                bone.a_scale_y,
                bone.a_shear_x,
                bone.a_shear_y,
            )
        };

        let rotation = a_rotation + (t_arotation + offset_rotation) * mix_rotate;
        let x = ax + (t_ax + offset_x) * mix_x;
        let y = ay + (t_ay + offset_y) * mix_y;
        let scale_x = a_scale_x * (((t_ascale_x - 1.0 + offset_scale_x) * mix_scale_x) + 1.0);
        let scale_y = a_scale_y * (((t_ascale_y - 1.0 + offset_scale_y) * mix_scale_y) + 1.0);
        let shear_y = a_shear_y + (t_ashear_y + offset_shear_y) * mix_shear_y;

        skeleton.update_bone_world_transform_with(
            bone_id, x, y, rotation, scale_x, scale_y, a_shear_x, shear_y,
        );
    }
}

// Silences a dead-import warning when the file is read without its full
// context (`BoneId` is implicit in the function loop bindings above).
#[allow(dead_code)]
const _BONE_ID: fn(u16) -> BoneId = |i| BoneId(i);
