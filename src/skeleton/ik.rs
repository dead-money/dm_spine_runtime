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

//! IK constraint solver. Literal port of
//! `spine-cpp/src/spine/IkConstraint.cpp`:
//!
//! - [`apply_one_bone`] — rotates a single bone so its tip points at the
//!   target, optionally stretching/compressing the bone to match distance.
//! - [`apply_two_bone`] — the classical two-bone analytical solver that
//!   positions a parent-child pair so the child's end reaches the target,
//!   with bend-direction and softness control.
//!
//! Both are called through [`solve_ik_constraint`], which reads the
//! constraint's current runtime state + target bone, dispatches to the
//! right arity, and writes updated world transforms into the affected
//! bones.

#![allow(clippy::many_single_char_names)] // mirrors spine-cpp variable names

use crate::data::{BoneId, Inherit};
use crate::math::util::{atan2_deg, sin_deg};
use crate::skeleton::Skeleton;

/// Run the IK constraint at `constraint_idx`. No-op when `mix == 0` or
/// `active == false`. Matches `spine::IkConstraint::update`.
pub(crate) fn solve_ik_constraint(skeleton: &mut Skeleton, constraint_idx: usize) {
    let (mix, bones, target, bend_direction, compress, stretch, softness, uniform) = {
        let c = &skeleton.ik_constraints[constraint_idx];
        if !c.active || c.mix == 0.0 {
            return;
        }
        let data = &skeleton.data.ik_constraints[c.data_index.index()];
        (
            c.mix,
            c.bones.clone(),
            c.target,
            c.bend_direction,
            c.compress,
            c.stretch,
            c.softness,
            data.uniform,
        )
    };

    let (target_world_x, target_world_y) = {
        let t = &skeleton.bones[target.index()];
        (t.world_x, t.world_y)
    };

    match bones.len() {
        1 => {
            apply_one_bone(
                skeleton,
                bones[0],
                target_world_x,
                target_world_y,
                compress,
                stretch,
                uniform,
                mix,
            );
        }
        2 => {
            apply_two_bone(
                skeleton,
                bones[0],
                bones[1],
                target_world_x,
                target_world_y,
                i32::from(bend_direction),
                stretch,
                uniform,
                softness,
                mix,
            );
        }
        _ => {
            // spine-cpp only supports 1- and 2-bone IK. Anything else is a
            // no-op (matches the `switch(_bones.size())` fallthrough).
        }
    }
}

/// One-bone IK: rotate `bone` so its tip points at `(target_x, target_y)`.
/// Compress/stretch scaling applies when the bone is too long/short to
/// reach. Matches `spine::IkConstraint::apply` (1-bone overload).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // spine-cpp 7-arg signature, single-function port
fn apply_one_bone(
    skeleton: &mut Skeleton,
    bone_id: BoneId,
    target_x: f32,
    target_y: f32,
    compress: bool,
    stretch: bool,
    uniform: bool,
    alpha: f32,
) {
    let (
        parent,
        inherit,
        a_shear_x,
        a_rotation,
        a_scale_x,
        a_scale_y,
        a_shear_y,
        ax,
        ay,
        world_x,
        world_y,
    ) = {
        let b = &skeleton.bones[bone_id.index()];
        (
            b.parent,
            b.inherit,
            b.a_shear_x,
            b.a_rotation,
            b.a_scale_x,
            b.a_scale_y,
            b.a_shear_y,
            b.ax,
            b.ay,
            b.world_x,
            b.world_y,
        )
    };
    let Some(parent_id) = parent else {
        return;
    };
    let (mut pa, mut pb, mut pc, mut pd, parent_world_x, parent_world_y) = {
        let p = &skeleton.bones[parent_id.index()];
        (p.a, p.b, p.c, p.d, p.world_x, p.world_y)
    };
    let sk_scale_x = skeleton.scale_x;
    let sk_scale_y = skeleton.scale_y;

    let mut rotation_ik = -a_shear_x - a_rotation;
    let (mut tx, mut ty);

    match inherit {
        Inherit::OnlyTranslation => {
            tx = (target_x - world_x) * sk_scale_x.signum();
            ty = (target_y - world_y) * sk_scale_y.signum();
        }
        Inherit::NoRotationOrReflection => {
            let s = (pa * pd - pb * pc).abs() / (pa * pa + pc * pc).max(0.0001);
            // spine-cpp overwrites pa, pc, pb, pd here. pa/pc are
            // scaled down by the skeleton's scale; pb/pd become the
            // perpendicular.
            pa /= sk_scale_x;
            pc /= sk_scale_y;
            let sa = pa;
            let sc = pc;
            pb = -sc * s * sk_scale_x;
            pd = sa * s * sk_scale_y;
            rotation_ik += atan2_deg(sc, sa);
            // Fall through into the default branch's target-to-local math.
            let x = target_x - parent_world_x;
            let y = target_y - parent_world_y;
            let d = pa * pd - pb * pc;
            if d.abs() <= 0.0001 {
                tx = 0.0;
                ty = 0.0;
            } else {
                tx = (x * pd - y * pb) / d - ax;
                ty = (y * pa - x * pc) / d - ay;
            }
        }
        _ => {
            let x = target_x - parent_world_x;
            let y = target_y - parent_world_y;
            let d = pa * pd - pb * pc;
            if d.abs() <= 0.0001 {
                tx = 0.0;
                ty = 0.0;
            } else {
                tx = (x * pd - y * pb) / d - ax;
                ty = (y * pa - x * pc) / d - ay;
            }
        }
    }

    rotation_ik += atan2_deg(ty, tx);
    if a_scale_x < 0.0 {
        rotation_ik += 180.0;
    }
    if rotation_ik > 180.0 {
        rotation_ik -= 360.0;
    } else if rotation_ik < -180.0 {
        rotation_ik += 360.0;
    }

    let mut sx = a_scale_x;
    let mut sy = a_scale_y;
    if compress || stretch {
        match inherit {
            Inherit::NoScale | Inherit::NoScaleOrReflection => {
                tx = target_x - world_x;
                ty = target_y - world_y;
            }
            _ => {}
        }
        let bone_length = skeleton.data.bones[bone_id.index()].length;
        let b = bone_length * sx;
        if b > 0.0001 {
            let dd = tx * tx + ty * ty;
            if (compress && dd < b * b) || (stretch && dd > b * b) {
                let s = (dd.sqrt() / b - 1.0) * alpha + 1.0;
                sx *= s;
                if uniform {
                    sy *= s;
                }
            }
        }
    }

    skeleton.update_bone_world_transform_with(
        bone_id,
        ax,
        ay,
        a_rotation + rotation_ik * alpha,
        sx,
        sy,
        a_shear_x,
        a_shear_y,
    );
}

/// Two-bone IK: positions `parent` and `child` so `child`'s tip reaches
/// `(target_x, target_y)`. Bend direction selects the elbow orientation;
/// softness rounds the approach near the reachable limit.
///
/// Matches `spine::IkConstraint::apply` (2-bone overload) — the math is
/// intricate (quadratic roots for non-uniform scale, iterative fallback
/// when the quadratic has no real solution) but self-contained.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn apply_two_bone(
    skeleton: &mut Skeleton,
    parent_id: BoneId,
    child_id: BoneId,
    target_x: f32,
    target_y: f32,
    bend_dir: i32,
    stretch: bool,
    uniform: bool,
    mut softness: f32,
    alpha: f32,
) {
    // spine-cpp bails immediately when either bone isn't in Normal inherit.
    let (parent_inherit, child_inherit) = {
        let p = &skeleton.bones[parent_id.index()];
        let c = &skeleton.bones[child_id.index()];
        (p.inherit, c.inherit)
    };
    if parent_inherit != Inherit::Normal || child_inherit != Inherit::Normal {
        return;
    }

    let (
        px,
        py,
        psx_raw,
        psy_raw,
        parent_world_x,
        parent_world_y,
        parent_a,
        parent_b,
        parent_c,
        parent_d,
    ) = {
        let p = &skeleton.bones[parent_id.index()];
        (
            p.ax,
            p.ay,
            p.a_scale_x,
            p.a_scale_y,
            p.world_x,
            p.world_y,
            p.a,
            p.b,
            p.c,
            p.d,
        )
    };
    let (cx, cy_raw, csx_raw, c_scale_x, c_scale_y, c_shear_x, c_shear_y, c_rotation) = {
        let c = &skeleton.bones[child_id.index()];
        (
            c.ax,
            c.ay,
            c.a_scale_x,
            c.a_scale_x,
            c.a_scale_y,
            c.a_shear_x,
            c.a_shear_y,
            c.a_rotation,
        )
    };
    // `csx_raw` above is right — `csx` starts as `child._ascaleX`.
    let _ = csx_raw; // bound as mutable below via `let mut csx = csx_raw;`

    let mut psx = psx_raw;
    let mut psy = psy_raw;
    let mut sx = psx;
    let mut sy = psy;
    let mut csx = csx_raw;
    let (o1, s2);
    if psx < 0.0 {
        psx = -psx;
        o1 = 180.0;
        s2 = -1.0;
    } else {
        o1 = 0.0;
        s2 = 1.0;
    }
    let s2 = if psy < 0.0 {
        psy = -psy;
        -s2
    } else {
        s2
    };
    let o2 = if csx < 0.0 {
        csx = -csx;
        180.0
    } else {
        0.0
    };

    let r = psx - psy;
    let u = r.abs() <= 0.0001;

    let (cwx, cwy, cy);
    if !u || stretch {
        cy = 0.0;
        cwx = parent_a * cx + parent_world_x;
        cwy = parent_c * cx + parent_world_y;
    } else {
        cy = cy_raw;
        cwx = parent_a * cx + parent_b * cy + parent_world_x;
        cwy = parent_c * cx + parent_d * cy + parent_world_y;
    }

    // Grandparent (parent.parent) transform — spine-cpp's `pp = parent.getParent()`.
    let Some(grandparent_id) = skeleton.bones[parent_id.index()].parent else {
        return;
    };
    let (pp_a, pp_b, pp_c, pp_d, pp_world_x, pp_world_y) = {
        let gp = &skeleton.bones[grandparent_id.index()];
        (gp.a, gp.b, gp.c, gp.d, gp.world_x, gp.world_y)
    };

    let (a_gp, b_gp, c_gp, d_gp) = (pp_a, pp_b, pp_c, pp_d);
    let id_raw = a_gp * d_gp - b_gp * c_gp;
    let id = if id_raw.abs() <= 0.0001 {
        0.0
    } else {
        1.0 / id_raw
    };

    let x_g = cwx - pp_world_x;
    let y_g = cwy - pp_world_y;
    let dx = (x_g * d_gp - y_g * b_gp) * id - px;
    let dy = (y_g * a_gp - x_g * c_gp) * id - py;
    let l1 = (dx * dx + dy * dy).sqrt();
    let mut l2 = skeleton.data.bones[child_id.index()].length * csx;
    if l1 < 0.0001 {
        // Degenerate: fall back to the one-bone solver for the parent.
        apply_one_bone(
            skeleton, parent_id, target_x, target_y, false, stretch, false, alpha,
        );
        skeleton.update_bone_world_transform_with(
            child_id, cx, cy, 0.0, c_scale_x, c_scale_y, c_shear_x, c_shear_y,
        );
        return;
    }

    let x = target_x - pp_world_x;
    let y = target_y - pp_world_y;
    let mut tx = (x * d_gp - y * b_gp) * id - px;
    let mut ty = (y * a_gp - x * c_gp) * id - py;
    let mut dd = tx * tx + ty * ty;

    if softness != 0.0 {
        softness *= psx * (csx + 1.0) * 0.5;
        let td = dd.sqrt();
        let sd = td - l1 - l2 * psx + softness;
        if sd > 0.0 {
            let p = (sd / (softness * 2.0)).min(1.0) - 1.0;
            let p = (sd - softness * (1.0 - p * p)) / td;
            tx -= p * tx;
            ty -= p * ty;
            dd = tx * tx + ty * ty;
        }
    }

    let (a1, a2);
    if u {
        l2 *= psx;
        let mut cosine = (dd - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
        if cosine < -1.0 {
            cosine = -1.0;
            a2 = std::f32::consts::PI * bend_dir as f32;
        } else if cosine > 1.0 {
            cosine = 1.0;
            a2 = 0.0;
            if stretch {
                let a = (dd.sqrt() / (l1 + l2) - 1.0) * alpha + 1.0;
                sx *= a;
                if uniform {
                    sy *= a;
                }
            }
        } else {
            a2 = cosine.acos() * bend_dir as f32;
        }
        let a = l1 + l2 * cosine;
        let b = l2 * sin_deg(a2.to_degrees());
        a1 = (ty * a - tx * b).atan2(tx * a + ty * b);
    } else {
        // Non-uniform scale: solve quadratic for foot-of-perpendicular.
        let a = psx * l2;
        let b = psy * l2;
        let aa = a * a;
        let bb = b * b;
        let ll = l1 * l1;
        let ta = ty.atan2(tx);
        let c0 = bb * ll + aa * dd - aa * bb;
        let c1 = -2.0 * bb * l1;
        let c2 = bb - aa;
        let d = c1 * c1 - 4.0 * c2 * c0;
        let mut resolved = false;
        let mut a1_result = 0.0_f32;
        let mut a2_result = 0.0_f32;
        if d >= 0.0 {
            let mut q = d.sqrt();
            if c1 < 0.0 {
                q = -q;
            }
            q = -(c1 + q) * 0.5;
            let r0 = q / c2;
            let r1 = c0 / q;
            let r = if r0.abs() < r1.abs() { r0 } else { r1 };
            if dd - r * r >= 0.0 {
                let y_r = (dd - r * r).sqrt() * bend_dir as f32;
                a1_result = ta - y_r.atan2(r);
                a2_result = (y_r / psy).atan2((r - l1) / psx);
                resolved = true;
            }
        }
        if !resolved {
            // Iterative fallback: scan the reachable boundary.
            let mut min_angle = std::f32::consts::PI;
            let mut min_x = l1 - a;
            let mut min_dist = min_x * min_x;
            let mut min_y = 0.0_f32;
            let mut max_angle = 0.0_f32;
            let mut max_x = l1 + a;
            let mut max_dist = max_x * max_x;
            let mut max_y = 0.0_f32;
            let c_in = -a * l1 / (aa - bb);
            if (-1.0..=1.0).contains(&c_in) {
                let c_acos = c_in.acos();
                let x_b = a * c_acos.cos() + l1;
                let y_b = b * c_acos.sin();
                let d_b = x_b * x_b + y_b * y_b;
                if d_b < min_dist {
                    min_angle = c_acos;
                    min_dist = d_b;
                    min_x = x_b;
                    min_y = y_b;
                }
                if d_b > max_dist {
                    max_angle = c_acos;
                    max_dist = d_b;
                    max_x = x_b;
                    max_y = y_b;
                }
            }
            if dd <= (min_dist + max_dist) * 0.5 {
                a1_result = ta - (min_y * bend_dir as f32).atan2(min_x);
                a2_result = min_angle * bend_dir as f32;
            } else {
                a1_result = ta - (max_y * bend_dir as f32).atan2(max_x);
                a2_result = max_angle * bend_dir as f32;
            }
        }
        a1 = a1_result;
        a2 = a2_result;
    }

    // Final: convert a1/a2 (radians) back to degrees, handle orientation
    // offsets, clamp to [-180, 180], and write world transforms.
    let os = cy.atan2(cx) * s2;
    let rad_deg = 180.0 / std::f32::consts::PI;

    let parent_arotation = skeleton.bones[parent_id.index()].a_rotation;
    let mut a1_deg = (a1 - os) * rad_deg + o1 - parent_arotation;
    if a1_deg > 180.0 {
        a1_deg -= 360.0;
    } else if a1_deg < -180.0 {
        a1_deg += 360.0;
    }
    skeleton.update_bone_world_transform_with(
        parent_id,
        px,
        py,
        parent_arotation + a1_deg * alpha,
        sx,
        sy,
        0.0,
        0.0,
    );

    let mut a2_deg = ((a2 + os) * rad_deg - c_shear_x) * s2 + o2 - c_rotation;
    if a2_deg > 180.0 {
        a2_deg -= 360.0;
    } else if a2_deg < -180.0 {
        a2_deg += 360.0;
    }
    skeleton.update_bone_world_transform_with(
        child_id,
        cx,
        cy,
        c_rotation + a2_deg * alpha,
        c_scale_x,
        c_scale_y,
        c_shear_x,
        c_shear_y,
    );
}
