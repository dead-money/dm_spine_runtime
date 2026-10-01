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

//! Path constraint (`PathConstraint.cpp`): positions bones along the cubic
//! bezier path attachment on a slot.

#![allow(clippy::many_single_char_names, clippy::needless_range_loop)]

use std::sync::Arc;

use crate::data::skin::resolve;
use crate::data::{
    Attachment, AttachmentRef, ConstraintId, PathAttachment, PathConstraintData, PositionMode,
    RotateMode, SkeletonData, Skin, SlotId, SpacingMode,
};
use crate::math::util::{DEG_RAD, EPSILON, PI, PI_2};
use crate::skeleton::bone::{self};
use crate::skeleton::constraint::{Constraint, PathConstraint};
use crate::skeleton::{Skeleton, UpdateCacheEntry};

const NONE: i32 = -1;
const BEFORE: i32 = -2;
const AFTER: i32 = -3;

impl Skeleton {
    pub(crate) fn sort_path(
        &mut self,
        id: ConstraintId,
        data: &PathConstraintData,
        sd: &SkeletonData,
    ) {
        let slot_index = data.slot;
        let slot_bone = self.slots[slot_index.index()].bone;
        let skin = self.skin.clone();
        if let Some(skin) = &skin {
            self.sort_path_slot(skin, slot_index, slot_bone, sd);
        }
        if let Some(default) = sd.default_skin()
            && !skin.as_ref().is_some_and(|s| Arc::ptr_eq(s, default))
        {
            self.sort_path_slot(default, slot_index, slot_bone, sd);
        }
        if let Some(attachment) = self.slots[slot_index.index()].posed.pose.attachment {
            let attachment = resolve(&sd.attachments, skin.as_deref(), attachment);
            self.sort_path_attachment(attachment, slot_bone);
        }
        for &b in &data.bones {
            self.sort_bone(b);
            self.constrain_bone(b);
        }
        self.update_cache.push(UpdateCacheEntry::Constraint(id));
        for &b in &data.bones {
            self.sort_reset(b);
        }
        for &b in &data.bones {
            self.bones[b.index()].sorted = true;
        }
    }

    fn sort_path_slot(
        &mut self,
        skin: &Skin,
        slot: SlotId,
        slot_bone: crate::data::BoneId,
        sd: &SkeletonData,
    ) {
        for &key in sd.skin_keys.slot_keys(slot) {
            if let Some(attachment) = skin.get(key) {
                self.sort_path_attachment(skin.resolve(&sd.attachments, attachment), slot_bone);
            }
        }
        for (_, attachment) in skin.extra_on(slot) {
            self.sort_path_attachment(skin.resolve(&sd.attachments, attachment), slot_bone);
        }
    }

    fn sort_path_attachment(&mut self, attachment: &Attachment, slot_bone: crate::data::BoneId) {
        let Attachment::Path(path) = attachment else {
            return;
        };
        let bones = &path.vertex_data.bones;
        if bones.is_empty() {
            self.sort_bone(slot_bone);
        } else {
            let mut i = 0;
            while i < bones.len() {
                let n = bones[i] as usize;
                i += 1;
                let end = i + n;
                while i < end {
                    self.sort_bone(crate::data::BoneId(bones[i] as u16));
                    i += 1;
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn update_path(
        &mut self,
        id: ConstraintId,
        data: &PathConstraintData,
        sd: &SkeletonData,
    ) {
        let slot = &self.slots[data.slot.index()];
        let Some(attachment) = slot.applied().attachment else {
            return;
        };
        let owner;
        let attachment = match attachment {
            AttachmentRef::Data(id) => &sd.attachments[id.index()],
            AttachmentRef::Owned(_) => {
                owner = self.skin.clone();
                resolve(&sd.attachments, owner.as_deref(), attachment)
            }
        };
        let Attachment::Path(path) = attachment else {
            return;
        };
        let Constraint::Path(c) = &mut self.constraints[id.index()] else {
            unreachable!()
        };
        let p = *c.posed.applied();
        let (mix_rotate, mix_x, mix_y) = (p.mix_rotate, p.mix_x, p.mix_y);
        if mix_rotate == 0.0 && mix_x == 0.0 && mix_y == 0.0 {
            return;
        }
        let mut scratch = std::mem::take(c);
        let f = self.frame();

        let tangents = data.rotate_mode == RotateMode::Tangent;
        let scale = data.rotate_mode == RotateMode::ChainScale;
        let bone_count = data.bones.len();
        let spaces_count = if tangents { bone_count } else { bone_count + 1 };
        scratch.spaces.clear();
        scratch.spaces.resize(spaces_count, 0.0);
        if scale {
            scratch.lengths.clear();
            scratch.lengths.resize(bone_count, 0.0);
        }
        let spacing = p.spacing;
        let spaces = &mut scratch.spaces;
        let lengths = &mut scratch.lengths;
        match data.spacing_mode {
            SpacingMode::Percent => {
                if scale {
                    for i in 0..spaces_count - 1 {
                        let b = data.bones[i];
                        let setup_length = sd.bones[b.index()].length;
                        let bp = self.bones[b.index()].applied();
                        let x = setup_length * bp.a;
                        let y = setup_length * bp.c;
                        lengths[i] = (x * x + y * y).sqrt();
                    }
                }
                for s in spaces.iter_mut().skip(1) {
                    *s = spacing;
                }
            }
            SpacingMode::Proportional => {
                let mut sum = 0.0;
                let n = spaces_count - 1;
                let mut i = 0;
                while i < n {
                    let b = data.bones[i];
                    let setup_length = sd.bones[b.index()].length;
                    if setup_length < EPSILON {
                        if scale {
                            lengths[i] = 0.0;
                        }
                        i += 1;
                        spaces[i] = spacing;
                    } else {
                        let bp = self.bones[b.index()].applied();
                        let x = setup_length * bp.a;
                        let y = setup_length * bp.c;
                        let length = (x * x + y * y).sqrt();
                        if scale {
                            lengths[i] = length;
                        }
                        i += 1;
                        spaces[i] = length;
                        sum += length;
                    }
                }
                if sum > 0.0 {
                    sum = spaces_count as f32 / sum * spacing;
                    for s in spaces.iter_mut().skip(1) {
                        *s *= sum;
                    }
                }
            }
            SpacingMode::Length | SpacingMode::Fixed => {
                let length_spacing = data.spacing_mode == SpacingMode::Length;
                let n = spaces_count - 1;
                let mut i = 0;
                while i < n {
                    let b = data.bones[i];
                    let setup_length = sd.bones[b.index()].length;
                    if setup_length < EPSILON {
                        if scale {
                            lengths[i] = 0.0;
                        }
                        i += 1;
                        spaces[i] = spacing;
                    } else {
                        let bp = self.bones[b.index()].applied();
                        let x = setup_length * bp.a;
                        let y = setup_length * bp.c;
                        let length = (x * x + y * y).sqrt();
                        if scale {
                            lengths[i] = length;
                        }
                        i += 1;
                        spaces[i] = (if length_spacing {
                            (setup_length + spacing).max(0.0)
                        } else {
                            spacing
                        }) * length
                            / setup_length;
                    }
                }
            }
        }

        self.compute_path_positions(
            &mut scratch,
            path,
            data,
            data.slot,
            p.position,
            spaces_count,
            tangents,
        );
        let positions = &scratch.positions;
        let mut bone_x = positions[0];
        let mut bone_y = positions[1];
        let mut offset_rotation = data.offset_rotation;
        let tip;
        if offset_rotation == 0.0 {
            tip = data.rotate_mode == RotateMode::Chain;
        } else {
            tip = false;
            let slot_bone = self.slots[data.slot.index()].bone;
            let bp = self.bones[slot_bone.index()].applied();
            offset_rotation *= if bp.a * bp.d - bp.b * bp.c > 0.0 {
                DEG_RAD
            } else {
                -DEG_RAD
            };
        }
        let mut ip = 3;
        for i in 0..bone_count {
            let bi = data.bones[i].index();
            bone::modify_world(&mut self.bones, bi, &f);
            let length_setup = sd.bones[bi].length;
            let bone = self.bones[bi].applied_mut();
            bone.world_x += (bone_x - bone.world_x) * mix_x;
            bone.world_y += (bone_y - bone.world_y) * mix_y;
            let x = positions[ip];
            let y = positions[ip + 1];
            let dx = x - bone_x;
            let dy = y - bone_y;
            if scale {
                let length = scratch.lengths[i];
                if length >= EPSILON {
                    let s = ((dx * dx + dy * dy).sqrt() / length - 1.0) * mix_rotate + 1.0;
                    bone.a *= s;
                    bone.c *= s;
                }
            }
            bone_x = x;
            bone_y = y;
            if mix_rotate > 0.0 {
                let (a, b, c, d) = (bone.a, bone.b, bone.c, bone.d);
                let mut r = if tangents {
                    positions[ip - 1]
                } else if scratch.spaces[i + 1] < EPSILON {
                    positions[ip + 2]
                } else {
                    dy.atan2(dx)
                };
                r -= c.atan2(a);
                if tip {
                    let (cos, sin) = (r.cos(), r.sin());
                    bone_x += (length_setup * (cos * a - sin * c) - dx) * mix_rotate;
                    bone_y += (length_setup * (sin * a + cos * c) - dy) * mix_rotate;
                } else {
                    r += offset_rotation;
                }
                if r > PI {
                    r -= PI_2;
                } else if r < -PI {
                    r += PI_2;
                }
                r *= mix_rotate;
                let (cos, sin) = (r.cos(), r.sin());
                bone.a = cos * a - sin * c;
                bone.b = cos * b - sin * d;
                bone.c = sin * a + cos * c;
                bone.d = sin * b + cos * d;
            }
            ip += 3;
        }

        let Constraint::Path(c) = &mut self.constraints[id.index()] else {
            unreachable!()
        };
        *c = scratch;
    }

    /// Samples `spaces_count` positions along the path into
    /// `scratch.positions` as `[x, y, rotation]` triples, rotation in radians.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn compute_path_positions(
        &self,
        scratch: &mut PathConstraint,
        path: &PathAttachment,
        data: &PathConstraintData,
        slot: SlotId,
        mut position: f32,
        spaces_count: usize,
        tangents: bool,
    ) {
        let out = &mut scratch.positions;
        out.clear();
        out.resize(spaces_count * 3 + 2, 0.0);
        let world = &mut scratch.world;
        let spaces = &scratch.spaces;
        let closed = path.closed;
        let mut vertices_length = path.vertex_data.world_vertices_length as usize;
        let mut curve_count = (vertices_length / 6) as i32;
        let mut prev_curve = NONE;
        let cwv = |start: usize, count: usize, world: &mut [f32], offset: usize| {
            self.compute_world_vertices(&path.vertex_data, slot, start, count, world, offset, 2);
        };

        if !path.constant_speed {
            let lengths = &path.lengths;
            curve_count -= if closed { 1 } else { 2 };
            let path_length = lengths[curve_count as usize];
            if data.position_mode == PositionMode::Percent {
                position *= path_length;
            }
            let multiplier = match data.spacing_mode {
                SpacingMode::Percent => path_length,
                SpacingMode::Proportional => path_length / spaces_count as f32,
                _ => 1.0,
            };
            world.clear();
            world.resize(8, 0.0);
            let mut curve = 0i32;
            let mut o = 0;
            for i in 0..spaces_count {
                let space = spaces[i] * multiplier;
                position += space;
                let mut p = position;
                if closed {
                    p %= path_length;
                    if p < 0.0 {
                        p += path_length;
                    }
                    curve = 0;
                } else if p < 0.0 {
                    if prev_curve != BEFORE {
                        prev_curve = BEFORE;
                        cwv(2, 4, world, 0);
                    }
                    add_before_position(p, world, 0, out, o);
                    o += 3;
                    continue;
                } else if p > path_length {
                    if prev_curve != AFTER {
                        prev_curve = AFTER;
                        cwv(vertices_length - 6, 4, world, 0);
                    }
                    add_after_position(p - path_length, world, 0, out, o);
                    o += 3;
                    continue;
                }
                loop {
                    let length = lengths[curve as usize];
                    if p > length {
                        curve += 1;
                        continue;
                    }
                    if curve == 0 {
                        p /= length;
                    } else {
                        let prev = lengths[(curve - 1) as usize];
                        p = (p - prev) / (length - prev);
                    }
                    break;
                }
                if curve != prev_curve {
                    prev_curve = curve;
                    if closed && curve == curve_count {
                        cwv(vertices_length - 4, 4, world, 0);
                        cwv(0, 4, world, 4);
                    } else {
                        cwv((curve * 6 + 2) as usize, 8, world, 0);
                    }
                }
                add_curve_position(
                    p,
                    world[0],
                    world[1],
                    world[2],
                    world[3],
                    world[4],
                    world[5],
                    world[6],
                    world[7],
                    out,
                    o,
                    tangents || (i > 0 && space < EPSILON),
                );
                o += 3;
            }
            return;
        }

        // Constant speed.
        if closed {
            vertices_length += 2;
            world.clear();
            world.resize(vertices_length, 0.0);
            cwv(2, vertices_length - 4, world, 0);
            cwv(0, 2, world, vertices_length - 4);
            world[vertices_length - 2] = world[0];
            world[vertices_length - 1] = world[1];
        } else {
            curve_count -= 1;
            vertices_length -= 4;
            world.clear();
            world.resize(vertices_length, 0.0);
            cwv(2, vertices_length, world, 0);
        }

        let curves = &mut scratch.curves;
        curves.clear();
        curves.resize(curve_count.max(0) as usize, 0.0);
        let mut path_length = 0.0;
        let mut x1 = world[0];
        let mut y1 = world[1];
        let (mut cx1, mut cy1, mut cx2, mut cy2, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut w = 2;
        for i in 0..curve_count.max(0) as usize {
            cx1 = world[w];
            cy1 = world[w + 1];
            cx2 = world[w + 2];
            cy2 = world[w + 3];
            x2 = world[w + 4];
            y2 = world[w + 5];
            let tmpx = (x1 - cx1 * 2.0 + cx2) * 0.1875;
            let tmpy = (y1 - cy1 * 2.0 + cy2) * 0.1875;
            let dddfx = ((cx1 - cx2) * 3.0 - x1 + x2) * 0.093_75;
            let dddfy = ((cy1 - cy2) * 3.0 - y1 + y2) * 0.093_75;
            let mut ddfx = tmpx * 2.0 + dddfx;
            let mut ddfy = tmpy * 2.0 + dddfy;
            let mut dfx = (cx1 - x1) * 0.75 + tmpx + dddfx * 0.166_666_67;
            let mut dfy = (cy1 - y1) * 0.75 + tmpy + dddfy * 0.166_666_67;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx;
            dfy += ddfy;
            ddfx += dddfx;
            ddfy += dddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx;
            dfy += ddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx + dddfx;
            dfy += ddfy + dddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            curves[i] = path_length;
            x1 = x2;
            y1 = y2;
            w += 6;
        }

        if data.position_mode == PositionMode::Percent {
            position *= path_length;
        }
        let multiplier = match data.spacing_mode {
            SpacingMode::Percent => path_length,
            SpacingMode::Proportional => path_length / spaces_count as f32,
            _ => 1.0,
        };

        let segments = &mut scratch.segments;
        let mut curve_length = 0.0;
        let mut curve = 0i32;
        let mut segment = 0usize;
        let mut o = 0;
        for i in 0..spaces_count {
            let space = spaces[i] * multiplier;
            position += space;
            let mut p = position;
            if closed {
                p %= path_length;
                if p < 0.0 {
                    p += path_length;
                }
                curve = 0;
                segment = 0;
            } else if p < 0.0 {
                add_before_position(p, world, 0, out, o);
                o += 3;
                continue;
            } else if p > path_length {
                add_after_position(p - path_length, world, vertices_length - 4, out, o);
                o += 3;
                continue;
            }
            loop {
                let length = curves[curve as usize];
                if p > length {
                    curve += 1;
                    continue;
                }
                if curve == 0 {
                    p /= length;
                } else {
                    let prev = curves[(curve - 1) as usize];
                    p = (p - prev) / (length - prev);
                }
                break;
            }
            if curve != prev_curve {
                prev_curve = curve;
                let ii = (curve * 6) as usize;
                x1 = world[ii];
                y1 = world[ii + 1];
                cx1 = world[ii + 2];
                cy1 = world[ii + 3];
                cx2 = world[ii + 4];
                cy2 = world[ii + 5];
                x2 = world[ii + 6];
                y2 = world[ii + 7];
                let tmpx = (x1 - cx1 * 2.0 + cx2) * 0.03;
                let tmpy = (y1 - cy1 * 2.0 + cy2) * 0.03;
                let dddfx = ((cx1 - cx2) * 3.0 - x1 + x2) * 0.006;
                let dddfy = ((cy1 - cy2) * 3.0 - y1 + y2) * 0.006;
                let mut ddfx = tmpx * 2.0 + dddfx;
                let mut ddfy = tmpy * 2.0 + dddfy;
                let mut dfx = (cx1 - x1) * 0.3 + tmpx + dddfx * 0.166_666_67;
                let mut dfy = (cy1 - y1) * 0.3 + tmpy + dddfy * 0.166_666_67;
                curve_length = (dfx * dfx + dfy * dfy).sqrt();
                segments[0] = curve_length;
                for s in segments.iter_mut().take(8).skip(1) {
                    dfx += ddfx;
                    dfy += ddfy;
                    ddfx += dddfx;
                    ddfy += dddfy;
                    curve_length += (dfx * dfx + dfy * dfy).sqrt();
                    *s = curve_length;
                }
                dfx += ddfx;
                dfy += ddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                segments[8] = curve_length;
                dfx += ddfx + dddfx;
                dfy += ddfy + dddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                segments[9] = curve_length;
                segment = 0;
            }
            p *= curve_length;
            loop {
                let length = segments[segment];
                if p > length {
                    segment += 1;
                    continue;
                }
                if segment == 0 {
                    p /= length;
                } else {
                    let prev = segments[segment - 1];
                    p = segment as f32 + (p - prev) / (length - prev);
                }
                break;
            }
            add_curve_position(
                p * 0.1,
                x1,
                y1,
                cx1,
                cy1,
                cx2,
                cy2,
                x2,
                y2,
                out,
                o,
                tangents || (i > 0 && space < EPSILON),
            );
            o += 3;
        }
    }
}

fn add_before_position(p: f32, temp: &[f32], i: usize, output: &mut [f32], o: usize) {
    let x1 = temp[i];
    let y1 = temp[i + 1];
    let dx = temp[i + 2] - x1;
    let dy = temp[i + 3] - y1;
    let r = dy.atan2(dx);
    output[o] = x1 + p * r.cos();
    output[o + 1] = y1 + p * r.sin();
    output[o + 2] = r;
}

fn add_after_position(p: f32, temp: &[f32], i: usize, output: &mut [f32], o: usize) {
    let x1 = temp[i + 2];
    let y1 = temp[i + 3];
    let dx = x1 - temp[i];
    let dy = y1 - temp[i + 1];
    let r = dy.atan2(dx);
    output[o] = x1 + p * r.cos();
    output[o + 1] = y1 + p * r.sin();
    output[o + 2] = r;
}

#[allow(clippy::too_many_arguments)]
fn add_curve_position(
    p: f32,
    x1: f32,
    y1: f32,
    cx1: f32,
    cy1: f32,
    cx2: f32,
    cy2: f32,
    x2: f32,
    y2: f32,
    output: &mut [f32],
    o: usize,
    tangents: bool,
) {
    if p < EPSILON || p.is_nan() {
        output[o] = x1;
        output[o + 1] = y1;
        output[o + 2] = (cy1 - y1).atan2(cx1 - x1);
        return;
    }
    let tt = p * p;
    let ttt = tt * p;
    let u = 1.0 - p;
    let uu = u * u;
    let uuu = uu * u;
    let ut = u * p;
    let ut3 = ut * 3.0;
    let uut3 = u * ut3;
    let utt3 = ut3 * p;
    let x = x1 * uuu + cx1 * uut3 + cx2 * utt3 + x2 * ttt;
    let y = y1 * uuu + cy1 * uut3 + cy2 * utt3 + y2 * ttt;
    output[o] = x;
    output[o + 1] = y;
    if tangents {
        if p < 0.001 {
            output[o + 2] = (cy1 - y1).atan2(cx1 - x1);
        } else {
            output[o + 2] = (y - (y1 * uu + cy1 * ut * 2.0 + cy2 * tt))
                .atan2(x - (x1 * uu + cx1 * ut * 2.0 + cx2 * tt));
        }
    }
}
