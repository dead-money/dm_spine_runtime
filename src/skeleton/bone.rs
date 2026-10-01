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

//! Runtime bones. Each bone has an unconstrained and a constrained
//! [`BonePose`]; the world transform of the applied one is computed lazily,
//! tracked by per-frame stamps against [`Frame::update`].

use crate::data::{BoneData, BoneId, BoneLocal, Inherit};
use crate::math::util::{DEG_RAD, EPSILON_SQ, RAD_DEG};
use crate::skeleton::pose::{Pose, Posed};

/// Skeleton-level values the bone transform needs.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Incremented once per `update_world_transform`.
    pub update: u32,
}

/// Local transform plus the world transform it produces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonePose {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,
    pub inherit: Inherit,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub world_x: f32,
    pub world_y: f32,
    /// `Frame::update` when the world transform was last computed.
    pub(crate) world: u32,
    /// `Frame::update` when the world transform was modified directly, which
    /// leaves the local transform stale until recomputed.
    pub(crate) local: u32,
}

impl Default for BonePose {
    fn default() -> Self {
        Self::from_local(&BoneLocal::default())
    }
}

impl Pose for BonePose {
    /// Local transform only; the world transform is recomputed from it.
    fn set_from(&mut self, other: &Self) {
        self.set_local(&other.local_transform());
    }
}

impl BonePose {
    #[must_use]
    pub fn from_local(l: &BoneLocal) -> Self {
        Self {
            x: l.x,
            y: l.y,
            rotation: l.rotation,
            scale_x: l.scale_x,
            scale_y: l.scale_y,
            shear_x: l.shear_x,
            shear_y: l.shear_y,
            inherit: l.inherit,
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            world_x: 0.0,
            world_y: 0.0,
            world: 0,
            local: 0,
        }
    }

    pub fn set_local(&mut self, l: &BoneLocal) {
        self.x = l.x;
        self.y = l.y;
        self.rotation = l.rotation;
        self.scale_x = l.scale_x;
        self.scale_y = l.scale_y;
        self.shear_x = l.shear_x;
        self.shear_y = l.shear_y;
        self.inherit = l.inherit;
    }

    #[must_use]
    pub fn local_transform(&self) -> BoneLocal {
        BoneLocal {
            x: self.x,
            y: self.y,
            rotation: self.rotation,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            shear_x: self.shear_x,
            shear_y: self.shear_y,
            inherit: self.inherit,
        }
    }

    #[must_use]
    pub fn world_rotation_x(&self) -> f32 {
        self.c.atan2(self.a) * RAD_DEG
    }

    #[must_use]
    pub fn world_rotation_y(&self) -> f32 {
        self.d.atan2(self.b) * RAD_DEG
    }

    #[must_use]
    pub fn world_scale_x(&self) -> f32 {
        (self.a * self.a + self.c * self.c).sqrt()
    }

    #[must_use]
    pub fn world_scale_y(&self) -> f32 {
        (self.b * self.b + self.d * self.d).sqrt()
    }

    #[must_use]
    pub fn world_to_local(&self, world_x: f32, world_y: f32) -> (f32, f32) {
        let det = self.a * self.d - self.b * self.c;
        let x = world_x - self.world_x;
        let y = world_y - self.world_y;
        (
            (x * self.d - y * self.b) / det,
            (y * self.a - x * self.c) / det,
        )
    }

    #[must_use]
    pub fn local_to_world(&self, local_x: f32, local_y: f32) -> (f32, f32) {
        (
            local_x * self.a + local_y * self.b + self.world_x,
            local_x * self.c + local_y * self.d + self.world_y,
        )
    }

    #[must_use]
    pub fn world_to_local_rotation(&self, world_rotation: f32) -> f32 {
        let r = world_rotation * DEG_RAD;
        let (sin, cos) = (r.sin(), r.cos());
        (self.a * sin - self.c * cos).atan2(self.d * cos - self.b * sin) * RAD_DEG + self.rotation
            - self.shear_x
    }

    #[must_use]
    pub fn local_to_world_rotation(&self, local_rotation: f32) -> f32 {
        let r = (local_rotation - self.rotation - self.shear_x) * DEG_RAD;
        let (sin, cos) = (r.sin(), r.cos());
        (cos * self.c + sin * self.d).atan2(cos * self.a + sin * self.b) * RAD_DEG
    }

    pub fn rotate_world(&mut self, degrees: f32) {
        let r = degrees * DEG_RAD;
        let (sin, cos) = (r.sin(), r.cos());
        let (ra, rb) = (self.a, self.b);
        self.a = cos * ra - sin * self.c;
        self.b = cos * rb - sin * self.d;
        self.c = sin * ra + cos * self.c;
        self.d = sin * rb + cos * self.d;
    }

    /// Local shear and scale from a world matrix relative to the parent.
    fn set_local_matrix(&mut self, ra: f32, rb: f32, rc: f32, rd: f32) {
        let x = ra * ra + rc * rc;
        let y = rb * rb + rd * rd;
        if x > EPSILON_SQ {
            self.shear_x = rc.atan2(ra) * RAD_DEG;
            self.scale_x = x.sqrt();
        } else {
            self.shear_x = 0.0;
            self.scale_x = 0.0;
        }
        self.scale_y = y.sqrt();
        if y > EPSILON_SQ {
            self.shear_y = rd.atan2(rb) * RAD_DEG;
            if ra * rd - rb * rc < 0.0 {
                self.scale_y = -self.scale_y;
                self.shear_y += 90.0;
            } else {
                self.shear_y -= 90.0;
            }
            wrap_shear(&mut self.shear_y);
        } else {
            self.shear_y = 0.0;
        }
    }

    /// As [`Self::set_local_matrix`], also recovering rotation (`ro` offsets it).
    fn set_local_matrix_rotation(&mut self, ra: f32, rb: f32, rc: f32, rd: f32, ro: f32) {
        self.shear_x = 0.0;
        let x = ra * ra + rc * rc;
        let y = rb * rb + rd * rd;
        if x > EPSILON_SQ {
            let r = rc.atan2(ra) * RAD_DEG;
            self.rotation = r + ro;
            self.scale_x = x.sqrt();
            self.scale_y = y.sqrt();
            if y > EPSILON_SQ {
                self.shear_y = rd.atan2(rb) * RAD_DEG;
                if ra * rd - rb * rc < 0.0 {
                    self.scale_y = -self.scale_y;
                    self.shear_y += 90.0 - r;
                } else {
                    self.shear_y -= 90.0 + r;
                }
                wrap_shear(&mut self.shear_y);
            } else {
                self.shear_y = 0.0;
            }
        } else {
            self.scale_x = 0.0;
            self.scale_y = y.sqrt();
            self.shear_y = 0.0;
            self.rotation = if y > EPSILON_SQ {
                rd.atan2(rb) * RAD_DEG - 90.0 + ro
            } else {
                ro
            };
        }
    }
}

#[inline]
fn wrap_shear(shear_y: &mut f32) {
    if *shear_y > 180.0 {
        *shear_y -= 360.0;
    } else if *shear_y <= -180.0 {
        *shear_y += 360.0;
    }
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub data: BoneId,
    pub parent: Option<BoneId>,
    pub children: Vec<BoneId>,
    pub posed: Posed<BonePose>,
    /// Off when the bone needs a skin that isn't applied.
    pub active: bool,
    pub(crate) sorted: bool,
}

impl Bone {
    #[must_use]
    pub fn new(data: &BoneData) -> Self {
        let pose = BonePose::from_local(&data.setup);
        Self {
            data: data.index,
            parent: data.parent,
            children: Vec::new(),
            posed: Posed::new(pose, pose),
            active: false,
            sorted: false,
        }
    }

    #[inline]
    #[must_use]
    pub fn applied(&self) -> &BonePose {
        self.posed.applied()
    }

    #[inline]
    pub fn applied_mut(&mut self) -> &mut BonePose {
        self.posed.applied_mut()
    }

    pub fn setup_pose(&mut self, data: &BoneData) {
        self.posed.pose.set_local(&data.setup);
    }
}

/// The parent's applied world matrix `(a, b, c, d, world_x, world_y)`.
#[inline]
fn parent_world(bones: &[Bone], i: usize) -> Option<(f32, f32, f32, f32, f32, f32)> {
    let p = bones[i].parent?;
    let pp = bones[p.index()].applied();
    Some((pp.a, pp.b, pp.c, pp.d, pp.world_x, pp.world_y))
}

/// Computes the applied world transform unless it was already computed this
/// frame.
pub fn update_bone(bones: &mut [Bone], i: usize, f: &Frame) {
    if bones[i].applied().world != f.update {
        update_world_transform(bones, i, f);
    }
}

/// Computes the applied world transform from the local transform and the
/// parent's applied world transform, first recovering the local transform if
/// a constraint wrote the world transform this frame.
#[allow(clippy::many_single_char_names)]
pub fn update_world_transform(bones: &mut [Bone], i: usize, f: &Frame) {
    if bones[i].applied().local == f.update {
        update_local_transform(bones, i, f);
    } else {
        bones[i].applied_mut().world = f.update;
    }

    let parent = parent_world(bones, i);
    let (sx, sy) = (f.scale_x, f.scale_y);
    let p = bones[i].applied_mut();
    let Some((mut pa, mut pb, mut pc, mut pd, pwx, pwy)) = parent else {
        let rx = (p.rotation + p.shear_x) * DEG_RAD;
        let ry = (p.rotation + 90.0 + p.shear_y) * DEG_RAD;
        p.a = rx.cos() * p.scale_x * sx;
        p.b = ry.cos() * p.scale_y * sx;
        p.c = rx.sin() * p.scale_x * sy;
        p.d = ry.sin() * p.scale_y * sy;
        p.world_x = p.x * sx + f.x;
        p.world_y = p.y * sy + f.y;
        return;
    };

    p.world_x = pa * p.x + pb * p.y + pwx;
    p.world_y = pc * p.x + pd * p.y + pwy;

    match p.inherit {
        Inherit::Normal => {
            let rx = (p.rotation + p.shear_x) * DEG_RAD;
            let ry = (p.rotation + 90.0 + p.shear_y) * DEG_RAD;
            let la = rx.cos() * p.scale_x;
            let lb = ry.cos() * p.scale_y;
            let lc = rx.sin() * p.scale_x;
            let ld = ry.sin() * p.scale_y;
            p.a = pa * la + pb * lc;
            p.b = pa * lb + pb * ld;
            p.c = pc * la + pd * lc;
            p.d = pc * lb + pd * ld;
        }
        Inherit::OnlyTranslation => {
            let rx = (p.rotation + p.shear_x) * DEG_RAD;
            let ry = (p.rotation + 90.0 + p.shear_y) * DEG_RAD;
            p.a = rx.cos() * p.scale_x * sx;
            p.b = ry.cos() * p.scale_y * sx;
            p.c = rx.sin() * p.scale_x * sy;
            p.d = ry.sin() * p.scale_y * sy;
        }
        Inherit::NoRotationOrReflection => {
            let (sxi, syi) = (1.0 / sx, 1.0 / sy);
            pa *= sxi;
            pc *= syi;
            let mut s = pa * pa + pc * pc;
            let r;
            if s > EPSILON_SQ {
                s = (pa * pd * syi - pb * sxi * pc).abs() / s;
                pb = pc * s;
                pd = pa * s;
                r = p.rotation - pc.atan2(pa) * RAD_DEG;
            } else {
                pa = 0.0;
                pc = 0.0;
                r = p.rotation - 90.0 + pd.atan2(pb) * RAD_DEG;
            }
            let rx = (r + p.shear_x) * DEG_RAD;
            let ry = (r + p.shear_y + 90.0) * DEG_RAD;
            let la = rx.cos() * p.scale_x;
            let lb = ry.cos() * p.scale_y;
            let lc = rx.sin() * p.scale_x;
            let ld = ry.sin() * p.scale_y;
            p.a = (pa * la - pb * lc) * sx;
            p.b = (pa * lb - pb * ld) * sx;
            p.c = (pc * la + pd * lc) * sy;
            p.d = (pc * lb + pd * ld) * sy;
        }
        Inherit::NoScale | Inherit::NoScaleOrReflection => {
            let (sxi, syi) = (1.0 / sx, 1.0 / sy);
            let r = p.rotation * DEG_RAD;
            let (cos_r, sin_r) = (r.cos(), r.sin());
            let mut za = (pa * cos_r + pb * sin_r) * sxi;
            let mut zc = (pc * cos_r + pd * sin_r) * syi;
            let s = 1.0 / (za * za + zc * zc).sqrt();
            za *= s;
            zc *= s;
            let (mut zb, mut zd) = (-zc, za);
            if p.inherit == Inherit::NoScale
                && (pa * pd - pb * pc < 0.0) != ((sx < 0.0) != (sy < 0.0))
            {
                zb = -zb;
                zd = -zd;
            }
            let rx = p.shear_x * DEG_RAD;
            let ry = (90.0 + p.shear_y) * DEG_RAD;
            let la = rx.cos() * p.scale_x;
            let lb = ry.cos() * p.scale_y;
            let lc = rx.sin() * p.scale_x;
            let ld = ry.sin() * p.scale_y;
            p.a = (za * la + zb * lc) * sx;
            p.b = (za * lb + zb * ld) * sx;
            p.c = (zc * la + zd * lc) * sy;
            p.d = (zc * lb + zd * ld) * sy;
        }
    }
}

/// Recovers the applied local transform from its world transform and the
/// parent's.
#[allow(clippy::many_single_char_names)]
pub fn update_local_transform(bones: &mut [Bone], i: usize, f: &Frame) {
    let parent = parent_world(bones, i);
    let (sx, sy) = (f.scale_x, f.scale_y);
    let p = bones[i].applied_mut();
    p.local = 0;
    p.world = f.update;

    let Some((mut pa, pb, mut pc, pd, pwx, pwy)) = parent else {
        let (sxi, syi) = (1.0 / sx, 1.0 / sy);
        p.x = (p.world_x - f.x) * sxi;
        p.y = (p.world_y - f.y) * syi;
        let (a, b, c, d) = (p.a * sxi, p.b * sxi, p.c * syi, p.d * syi);
        p.set_local_matrix_rotation(a, b, c, d, 0.0);
        return;
    };

    let pad = pa * pd - pb * pc;
    let pid = 1.0 / pad;
    let (ia, ib, ic, id) = (pd * pid, pb * pid, pc * pid, pa * pid);
    let dx = p.world_x - pwx;
    let dy = p.world_y - pwy;
    p.x = dx * ia - dy * ib;
    p.y = dy * id - dx * ic;

    match p.inherit {
        Inherit::Normal => {
            let (a, b, c, d) = (
                ia * p.a - ib * p.c,
                ia * p.b - ib * p.d,
                id * p.c - ic * p.a,
                id * p.d - ic * p.b,
            );
            p.set_local_matrix_rotation(a, b, c, d, 0.0);
        }
        Inherit::OnlyTranslation => {
            let (sxi, syi) = (1.0 / sx, 1.0 / sy);
            let (a, b, c, d) = (p.a * sxi, p.b * sxi, p.c * syi, p.d * syi);
            p.set_local_matrix_rotation(a, b, c, d, 0.0);
        }
        Inherit::NoRotationOrReflection => {
            let (sxi, syi) = (1.0 / sx, 1.0 / sy);
            pa *= sxi;
            pc *= syi;
            let (wa, wb, wc, wd) = (p.a * sxi, p.b * sxi, p.c * syi, p.d * syi);
            let s = 1.0 / (pa * pa + pc * pc);
            let det = 1.0 / (pad * sxi * syi).abs();
            p.set_local_matrix_rotation(
                (pa * wa + pc * wc) * s,
                (pa * wb + pc * wd) * s,
                (pa * wc - pc * wa) * det,
                (pa * wd - pc * wb) * det,
                pc.atan2(pa) * RAD_DEG,
            );
        }
        Inherit::NoScale | Inherit::NoScaleOrReflection => {
            let (sxi, syi) = (1.0 / sx, 1.0 / sy);
            let (wa, wb, wc, wd) = (p.a * sxi, p.b * sxi, p.c * syi, p.d * syi);
            let mut tx = pd * p.a - pb * p.c;
            let mut ty = pa * p.c - pc * p.a;
            if pad < 0.0 {
                tx = -tx;
                ty = -ty;
            }
            let mut r = ty.atan2(tx) * RAD_DEG;
            p.rotation = r;
            r *= DEG_RAD;
            let (cos_r, sin_r) = (r.cos(), r.sin());
            let mut za = (pa * cos_r + pb * sin_r) * sxi;
            let mut zc = (pc * cos_r + pd * sin_r) * syi;
            let s = 1.0 / (za * za + zc * zc).sqrt();
            za *= s;
            zc *= s;
            let si = if p.inherit == Inherit::NoScale && (pad < 0.0) != ((sx < 0.0) != (sy < 0.0)) {
                -1.0
            } else {
                1.0
            };
            p.set_local_matrix(
                za * wa + zc * wc,
                za * wb + zc * wd,
                (za * wc - zc * wa) * si,
                (za * wd - zc * wb) * si,
            );
        }
    }
}

/// Recomputes the local transform if a constraint wrote the world transform
/// this frame.
pub fn validate_local_transform(bones: &mut [Bone], i: usize, f: &Frame) {
    if bones[i].applied().local == f.update {
        update_local_transform(bones, i, f);
    }
}

/// Call before changing the local transform after it may have been used.
pub fn modify_local(bones: &mut [Bone], i: usize, f: &Frame) {
    if bones[i].applied().local == f.update {
        update_local_transform(bones, i, f);
    }
    bones[i].applied_mut().world = 0;
    reset_world(bones, i, f);
}

/// Call before changing the world transform directly.
pub fn modify_world(bones: &mut [Bone], i: usize, f: &Frame) {
    let p = bones[i].applied_mut();
    p.local = f.update;
    p.world = f.update;
    reset_world(bones, i, f);
}

/// Invalidates descendants whose world transform was computed this frame
/// from the old parent transform.
fn reset_world(bones: &mut [Bone], i: usize, f: &Frame) {
    for c in 0..bones[i].children.len() {
        let child = bones[i].children[c].index();
        if bones[child].applied().world == f.update {
            if bones[child].applied().local == f.update {
                update_local_transform(bones, child, f);
            }
            bones[child].applied_mut().world = 0;
            reset_world(bones, child, f);
        }
    }
}

#[cfg(test)]
#[allow(clippy::excessive_precision, clippy::unreadable_literal)]
mod tests {
    use std::sync::Arc;

    use crate::data::{BoneData, BoneId, Inherit, SkeletonData};
    use crate::skeleton::{Physics, Skeleton};

    /// No example rig uses the `NoScale` inherit modes under a reflected
    /// parent. Expected values are from `tools/spine_capture/spine_synthetic`.
    fn pose(inherit: Inherit) -> [f32; 6] {
        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.setup.x = 10.0;
        root.setup.y = 5.0;
        root.setup.scale_x = -1.0;
        root.setup.rotation = 30.0;
        let mut child = BoneData::new(BoneId(1), "child", Some(BoneId(0)));
        child.setup.x = 20.0;
        child.setup.rotation = 45.0;
        child.setup.scale_x = 2.0;
        child.setup.scale_y = 0.5;
        child.setup.shear_x = 10.0;
        child.setup.shear_y = -5.0;
        child.setup.inherit = inherit;
        sd.bones = vec![root, child];
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_world_transform(Physics::None);
        let p = sk.bones[1].applied();
        [p.a, p.b, p.c, p.d, p.world_x, p.world_y]
    }

    fn assert_close(got: [f32; 6], want: [f32; 6]) {
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-5, "got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn no_scale_under_reflected_parent() {
        assert_close(
            pose(Inherit::NoScale),
            [
                -1.81261575,
                0.0868240595,
                0.84523654,
                0.492403924,
                -7.32050705,
                -5.0,
            ],
        );
    }

    #[test]
    fn no_scale_or_reflection_under_reflected_parent() {
        assert_close(
            pose(Inherit::NoScaleOrReflection),
            [
                -1.99238956,
                -0.171010107,
                0.174311399,
                -0.469846368,
                -7.32050705,
                -5.0,
            ],
        );
    }
}
