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

//! IK constraint (`IkConstraint.cpp`): one- and two-bone solvers.

use crate::data::{ConstraintId, IkConstraintData, Inherit, ScaleYMode};
use crate::math::util::{EPSILON, PI, RAD_DEG};
use crate::skeleton::Skeleton;
use crate::skeleton::bone::{self, Bone, Frame};
use crate::skeleton::constraint::Constraint;

impl Skeleton {
    pub(crate) fn sort_ik(&mut self, id: ConstraintId, data: &IkConstraintData) {
        self.sort_bone(data.target);
        let parent = data.bones[0];
        self.sort_bone(parent);
        self.update_cache
            .push(crate::skeleton::UpdateCacheEntry::Constraint(id));
        self.bones[parent.index()].sorted = false;
        self.sort_reset(parent);
        self.constrain_bone(parent);
        if data.bones.len() > 1 {
            self.constrain_bone(data.bones[1]);
        }
    }

    pub(crate) fn update_ik(&mut self, id: ConstraintId, data: &IkConstraintData) {
        let Constraint::Ik(c) = &self.constraints[id.index()] else {
            unreachable!()
        };
        let p = *c.posed.applied();
        if p.mix == 0.0 {
            return;
        }
        let frame = self.frame();
        let target = self.bones[data.target.index()].applied();
        let (tx, ty) = (target.world_x, target.world_y);
        let skeleton_data = std::sync::Arc::clone(&self.data);
        let length = |b: crate::data::BoneId| skeleton_data.bones[b.index()].length;
        match data.bones.len() {
            1 => apply1(
                &mut self.bones,
                data.bones[0].index(),
                length(data.bones[0]),
                &frame,
                tx,
                ty,
                p.compress,
                p.stretch,
                data.scale_y_mode,
                p.mix,
            ),
            2 => apply2(
                &mut self.bones,
                data.bones[0].index(),
                data.bones[1].index(),
                (length(data.bones[0]), length(data.bones[1])),
                &frame,
                tx,
                ty,
                p.bend_direction,
                p.stretch,
                data.scale_y_mode,
                p.softness,
                p.mix,
            ),
            _ => {}
        }
    }
}

fn wrap180(v: &mut f32) {
    if *v > 180.0 {
        *v -= 360.0;
    } else if *v <= -180.0 {
        *v += 360.0;
    }
}

fn scale_y(scale_y: &mut f32, s: f32, mode: ScaleYMode) {
    match mode {
        ScaleYMode::Uniform => *scale_y *= s,
        ScaleYMode::Volume => *scale_y /= if s < 0.7 { 0.25 + 0.642_857 * s } else { s },
        ScaleYMode::None => {}
    }
}

fn parent_applied(bones: &[Bone], i: usize) -> (f32, f32, f32, f32, f32, f32) {
    match bones[i].parent {
        Some(p) => {
            let p = bones[p.index()].applied();
            (p.a, p.b, p.c, p.d, p.world_x, p.world_y)
        }
        None => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    }
}

/// Rotates one bone to point at the target.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply1(
    bones: &mut [Bone],
    i: usize,
    length: f32,
    f: &Frame,
    target_x: f32,
    target_y: f32,
    compress: bool,
    stretch: bool,
    scale_y_mode: ScaleYMode,
    mix: f32,
) {
    bone::modify_local(bones, i, f);
    let (pa, mut pb, pc, mut pd, pwx, pwy) = parent_applied(bones, i);
    let (sx, sy) = (f.scale_x, f.scale_y);
    let b = bones[i].applied_mut();
    let mut rotation_ik = -b.shear_x - b.rotation;
    let (mut tx, mut ty);
    let default_target = |pa: f32, pb: f32, pd: f32, b: &crate::skeleton::BonePose| {
        let x = target_x - pwx;
        let y = target_y - pwy;
        let d = pa * pd - pb * pc;
        if d.abs() <= EPSILON {
            (0.0, 0.0)
        } else {
            ((x * pd - y * pb) / d - b.x, (y * pa - x * pc) / d - b.y)
        }
    };
    match b.inherit {
        Inherit::OnlyTranslation => {
            tx = (target_x - b.world_x) * sign(sx);
            ty = (target_y - b.world_y) * sign(sy);
        }
        Inherit::NoRotationOrReflection => {
            let s = (pa * pd - pb * pc).abs() / EPSILON.max(pa * pa + pc * pc);
            let sa = pa / sx;
            let sc = pc / sy;
            pb = -sc * s * sx;
            pd = sa * s * sy;
            rotation_ik += sc.atan2(sa) * RAD_DEG;
            (tx, ty) = default_target(pa, pb, pd, b);
        }
        _ => {
            (tx, ty) = default_target(pa, pb, pd, b);
        }
    }
    rotation_ik += ty.atan2(tx) * RAD_DEG;
    if b.scale_x < 0.0 {
        rotation_ik += 180.0;
    }
    wrap180(&mut rotation_ik);
    b.rotation += rotation_ik * mix;
    if compress || stretch {
        if matches!(b.inherit, Inherit::NoScale | Inherit::NoScaleOrReflection) {
            tx = target_x - b.world_x;
            ty = target_y - b.world_y;
        }
        let bl = length * b.scale_x;
        if bl > EPSILON {
            let dd = tx * tx + ty * ty;
            if (compress && dd < bl * bl) || (stretch && dd > bl * bl) {
                let s = (dd.sqrt() / bl - 1.0) * mix + 1.0;
                b.scale_x *= s;
                scale_y(&mut b.scale_y, s, scale_y_mode);
            }
        }
    }
}

#[inline]
fn sign(v: f32) -> f32 {
    if v < 0.0 {
        -1.0
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// Bends a parent and child bone to reach the target.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names
)]
pub(crate) fn apply2(
    bones: &mut [Bone],
    pi: usize,
    ci: usize,
    (parent_length, child_length): (f32, f32),
    f: &Frame,
    target_x: f32,
    target_y: f32,
    bend_dir: i32,
    stretch: bool,
    scale_y_mode: ScaleYMode,
    mut softness: f32,
    mix: f32,
) {
    if bones[pi].applied().inherit != Inherit::Normal
        || bones[ci].applied().inherit != Inherit::Normal
    {
        return;
    }
    bone::modify_local(bones, pi, f);
    bone::modify_local(bones, ci, f);
    let bend_dir_f = bend_dir as f32;
    let parent = *bones[pi].applied();
    let mut child = *bones[ci].applied();
    let (px, py) = (parent.x, parent.y);
    let (mut psx, mut psy, mut csx) = (parent.scale_x, parent.scale_y, child.scale_x);
    let (os1, mut s2);
    if psx < 0.0 {
        psx = -psx;
        os1 = 180.0;
        s2 = -1.0;
    } else {
        os1 = 0.0;
        s2 = 1.0;
    }
    if psy < 0.0 {
        psy = -psy;
        s2 = -s2;
    }
    let os2 = if csx < 0.0 {
        csx = -csx;
        180.0
    } else {
        0.0
    };
    let (mut a, mut b, mut c, mut d) = (parent.a, parent.b, parent.c, parent.d);
    let u = (psx - psy).abs() <= EPSILON;
    let (cwx, cwy);
    if !u || stretch {
        child.y = 0.0;
        cwx = a * child.x + parent.world_x;
        cwy = c * child.x + parent.world_y;
    } else {
        cwx = a * child.x + b * child.y + parent.world_x;
        cwy = c * child.x + d * child.y + parent.world_y;
    }
    let (ppa, ppb, ppc, ppd, ppwx, ppwy) = parent_applied(bones, pi);
    a = ppa;
    b = ppb;
    c = ppc;
    d = ppd;
    let mut id = a * d - b * c;
    let mut x = cwx - ppwx;
    let mut y = cwy - ppwy;
    id = if id.abs() <= EPSILON { 0.0 } else { 1.0 / id };
    let dx = (x * d - y * b) * id - px;
    let dy = (y * a - x * c) * id - py;
    let l1 = (dx * dx + dy * dy).sqrt();
    let mut l2 = child_length * csx;
    let (mut a1, mut a2);
    if l1 < EPSILON {
        bones[ci].applied_mut().y = child.y;
        apply1(
            bones,
            pi,
            parent_length,
            f,
            target_x,
            target_y,
            false,
            stretch,
            ScaleYMode::None,
            mix,
        );
        bones[ci].applied_mut().rotation = 0.0;
        return;
    }
    x = target_x - ppwx;
    y = target_y - ppwy;
    let mut tx = (x * d - y * b) * id - px;
    let mut ty = (y * a - x * c) * id - py;
    let mut dd = tx * tx + ty * ty;
    if softness != 0.0 {
        softness *= psx * (csx + 1.0) * 0.5;
        let td = dd.sqrt();
        let sd = td - l1 - l2 * psx + softness;
        if sd > 0.0 {
            let mut p = (sd / (softness * 2.0)).min(1.0) - 1.0;
            p = (sd - softness * (1.0 - p * p)) / td;
            tx -= p * tx;
            ty -= p * ty;
            dd = tx * tx + ty * ty;
        }
    }

    let mut parent_scale_x = parent.scale_x;
    let mut parent_scale_y = parent.scale_y;
    'outer: {
        if u {
            l2 *= psx;
            let mut cos = (dd - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
            if cos < -1.0 {
                cos = -1.0;
                a2 = PI * bend_dir_f;
            } else if cos > 1.0 {
                cos = 1.0;
                a2 = 0.0;
                if stretch {
                    a = (dd.sqrt() / (l1 + l2) - 1.0) * mix + 1.0;
                    parent_scale_x *= a;
                    scale_y(&mut parent_scale_y, a, scale_y_mode);
                }
            } else {
                a2 = cos.acos() * bend_dir_f;
            }
            a = l1 + l2 * cos;
            b = l2 * a2.sin();
            a1 = (ty * a - tx * b).atan2(tx * a + ty * b);
        } else {
            a = psx * l2;
            b = psy * l2;
            let aa = a * a;
            let bb = b * b;
            let ta = ty.atan2(tx);
            c = bb * l1 * l1 + aa * dd - aa * bb;
            let c1 = -2.0 * bb * l1;
            let c2 = bb - aa;
            d = c1 * c1 - 4.0 * c2 * c;
            if d >= 0.0 {
                let mut q = d.sqrt();
                if c1 < 0.0 {
                    q = -q;
                }
                q = -(c1 + q) * 0.5;
                let mut r0 = q / c2;
                let r1 = c / q;
                let r = if r0.abs() < r1.abs() { r0 } else { r1 };
                r0 = dd - r * r;
                if r0 >= 0.0 {
                    y = r0.sqrt() * bend_dir_f;
                    a1 = ta - y.atan2(r);
                    a2 = (y / psy).atan2((r - l1) / psx);
                    break 'outer;
                }
            }
            let mut min_angle = PI;
            let mut min_x = l1 - a;
            let mut min_dist = min_x * min_x;
            let mut min_y = 0.0;
            let mut max_angle = 0.0;
            let mut max_x = l1 + a;
            let mut max_dist = max_x * max_x;
            let mut max_y = 0.0;
            c = -a * l1 / (aa - bb);
            if (-1.0..=1.0).contains(&c) {
                c = c.acos();
                x = a * c.cos() + l1;
                y = b * c.sin();
                d = x * x + y * y;
                if d < min_dist {
                    min_angle = c;
                    min_dist = d;
                    min_x = x;
                    min_y = y;
                }
                if d > max_dist {
                    max_angle = c;
                    max_dist = d;
                    max_x = x;
                    max_y = y;
                }
            }
            if dd <= (min_dist + max_dist) * 0.5 {
                a1 = ta - (min_y * bend_dir_f).atan2(min_x);
                a2 = min_angle * bend_dir_f;
            } else {
                a1 = ta - (max_y * bend_dir_f).atan2(max_x);
                a2 = max_angle * bend_dir_f;
            }
        }
    }
    let os = child.y.atan2(child.x) * s2;
    a1 = (a1 - os) * RAD_DEG + os1 - parent.rotation;
    wrap180(&mut a1);
    {
        let p = bones[pi].applied_mut();
        p.scale_x = parent_scale_x;
        p.scale_y = parent_scale_y;
        p.rotation += a1 * mix;
    }
    a2 = ((a2 + os) * RAD_DEG - child.shear_x) * s2 + os2 - child.rotation;
    wrap180(&mut a2);
    let cp = bones[ci].applied_mut();
    cp.y = child.y;
    cp.rotation += a2 * mix;
}
