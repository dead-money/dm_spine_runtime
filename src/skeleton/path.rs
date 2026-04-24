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

//! Path constraint solver. Port of `spine-cpp/src/spine/PathConstraint.cpp`.
//!
//! Positions one or more constrained bones along a cubic-bezier path
//! carried by the target slot's [`PathAttachment`]. Supports Fixed /
//! Percent / Length / Proportional spacing modes and Tangent / Chain /
//! `ChainScale` rotate modes.
//!
//! [`PathAttachment`]: crate::data::attachment::PathAttachment

#![allow(
    clippy::many_single_char_names,
    // Spine's path solver uses index math pervasively; iterators would
    // obscure the spine-cpp diff.
    clippy::needless_range_loop
)]

use crate::data::{Attachment, BoneId, PositionMode, RotateMode, SpacingMode};
use crate::skeleton::Skeleton;

const EPSILON: f32 = 0.00001;
const NONE: i32 = -1;
const BEFORE: i32 = -2;
const AFTER: i32 = -3;

/// Run the Path constraint at `constraint_idx`. Early-outs when the
/// target slot's attachment isn't a `PathAttachment` or all mix values
/// are zero. Matches `spine::PathConstraint::update`.
#[allow(clippy::too_many_lines)]
pub(crate) fn solve_path_constraint(skeleton: &mut Skeleton, constraint_idx: usize) {
    let (active, mix_rotate, mix_x, mix_y, target, bones, data_idx, position, spacing) = {
        let c = &skeleton.path_constraints[constraint_idx];
        (
            c.active,
            c.mix_rotate,
            c.mix_x,
            c.mix_y,
            c.target,
            c.bones.clone(),
            c.data_index,
            c.position,
            c.spacing,
        )
    };
    if !active || (mix_rotate == 0.0 && mix_x == 0.0 && mix_y == 0.0) {
        return;
    }

    // Resolve the attachment through the slot.
    let attachment_id = skeleton.slots[target.index()].attachment;
    let Some(att_id) = attachment_id else { return };
    let is_path = matches!(
        &skeleton.data.attachments[att_id.index()],
        Attachment::Path(_)
    );
    if !is_path {
        return;
    }

    let (position_mode, spacing_mode, rotate_mode, offset_rotation) = {
        let d = &skeleton.data.path_constraints[data_idx.index()];
        (
            d.position_mode,
            d.spacing_mode,
            d.rotate_mode,
            d.offset_rotation,
        )
    };

    let tangents = rotate_mode == RotateMode::Tangent;
    let scale_mode = rotate_mode == RotateMode::ChainScale;
    let bone_count = bones.len();
    let spaces_count = if tangents { bone_count } else { bone_count + 1 };

    // --- Build spaces[] + (optionally) lengths[] per spacing mode ----
    let mut spaces = vec![0.0_f32; spaces_count];
    let mut lengths = if scale_mode {
        vec![0.0_f32; bone_count]
    } else {
        Vec::new()
    };
    compute_spaces(
        skeleton,
        &bones,
        spacing,
        spacing_mode,
        scale_mode,
        &mut spaces,
        &mut lengths,
    );

    // --- Compute world positions along the path ----------------------
    let positions = compute_world_positions(
        skeleton,
        att_id,
        target,
        position,
        position_mode,
        spacing_mode,
        spaces_count,
        tangents,
        &spaces,
    );

    // --- Apply to each constrained bone ------------------------------
    let (tip, offset_rotation_rad);
    if offset_rotation == 0.0 {
        tip = rotate_mode == RotateMode::Chain;
        offset_rotation_rad = 0.0;
    } else {
        tip = false;
        let (ta, tb, tc, td) = {
            let slot_bone = skeleton.data.slots[target.index()].bone;
            let p = &skeleton.bones[slot_bone.index()];
            (p.a, p.b, p.c, p.d)
        };
        let deg_rad = std::f32::consts::PI / 180.0;
        offset_rotation_rad = offset_rotation
            * if ta * td - tb * tc > 0.0 {
                deg_rad
            } else {
                -deg_rad
            };
    }

    let mut bone_x = positions[0];
    let mut bone_y = positions[1];
    let mut p = 3_usize;
    for (i, bone_id) in bones.iter().copied().enumerate() {
        // Translate: blend the bone's world position toward the path sample.
        {
            let bone = &mut skeleton.bones[bone_id.index()];
            bone.world_x += (bone_x - bone.world_x) * mix_x;
            bone.world_y += (bone_y - bone.world_y) * mix_y;
        }
        let x = positions[p];
        let y = positions[p + 1];
        let dx = x - bone_x;
        let dy = y - bone_y;

        if scale_mode {
            let length = lengths[i];
            if length >= EPSILON {
                let s = ((dx * dx + dy * dy).sqrt() / length - 1.0) * mix_rotate + 1.0;
                let bone = &mut skeleton.bones[bone_id.index()];
                bone.a *= s;
                bone.c *= s;
            }
        }
        bone_x = x;
        bone_y = y;

        if mix_rotate > 0.0 {
            let (a, c) = {
                let bone = &skeleton.bones[bone_id.index()];
                (bone.a, bone.c)
            };
            let mut r = if tangents {
                positions[p - 1]
            } else if spaces[i + 1] < EPSILON {
                positions[p + 2]
            } else {
                dy.atan2(dx)
            };
            r -= c.atan2(a);

            if tip {
                let cos = r.cos();
                let sin = r.sin();
                let bone_length = skeleton.data.bones[bone_id.index()].length;
                bone_x += (bone_length * (cos * a - sin * c) - dx) * mix_rotate;
                bone_y += (bone_length * (sin * a + cos * c) - dy) * mix_rotate;
            } else {
                r += offset_rotation_rad;
            }

            if r > std::f32::consts::PI {
                r -= std::f32::consts::TAU;
            } else if r < -std::f32::consts::PI {
                r += std::f32::consts::TAU;
            }
            r *= mix_rotate;
            let cos = r.cos();
            let sin = r.sin();
            let bone = &mut skeleton.bones[bone_id.index()];
            let (ba, bb, bc, bd) = (bone.a, bone.b, bone.c, bone.d);
            bone.a = cos * ba - sin * bc;
            bone.b = cos * bb - sin * bd;
            bone.c = sin * ba + cos * bc;
            bone.d = sin * bb + cos * bd;
        }

        skeleton.update_applied_transform(bone_id);
        p += 3;
    }
}

/// Fill `spaces` (and `lengths` when `scale_mode` is true) per the
/// configured [`SpacingMode`]. Ports the switch statement in
/// `PathConstraint::update` that precedes the world-position compute.
fn compute_spaces(
    skeleton: &Skeleton,
    bones: &[BoneId],
    spacing: f32,
    spacing_mode: SpacingMode,
    scale_mode: bool,
    spaces: &mut [f32],
    lengths: &mut [f32],
) {
    let spaces_count = spaces.len();
    match spacing_mode {
        SpacingMode::Percent => {
            if scale_mode {
                for i in 0..spaces_count.saturating_sub(1) {
                    let bone_id = bones[i];
                    let setup_length = skeleton.data.bones[bone_id.index()].length;
                    let bone = &skeleton.bones[bone_id.index()];
                    let x = setup_length * bone.a;
                    let y = setup_length * bone.c;
                    lengths[i] = (x * x + y * y).sqrt();
                }
            }
            for space in spaces.iter_mut().skip(1) {
                *space = spacing;
            }
        }
        SpacingMode::Proportional => {
            let mut sum = 0.0_f32;
            let mut i = 0_usize;
            let n = spaces_count.saturating_sub(1);
            while i < n {
                let bone_id = bones[i];
                let setup_length = skeleton.data.bones[bone_id.index()].length;
                if setup_length < EPSILON {
                    if scale_mode {
                        lengths[i] = 0.0;
                    }
                    i += 1;
                    spaces[i] = spacing;
                } else {
                    let bone = &skeleton.bones[bone_id.index()];
                    let x = setup_length * bone.a;
                    let y = setup_length * bone.c;
                    let length = (x * x + y * y).sqrt();
                    if scale_mode {
                        lengths[i] = length;
                    }
                    i += 1;
                    spaces[i] = length;
                    sum += length;
                }
            }
            if sum > 0.0 {
                let mul = spaces_count as f32 / sum * spacing;
                for space in spaces.iter_mut().skip(1) {
                    *space *= mul;
                }
            }
        }
        SpacingMode::Length | SpacingMode::Fixed => {
            let length_spacing = spacing_mode == SpacingMode::Length;
            let mut i = 0_usize;
            let n = spaces_count.saturating_sub(1);
            while i < n {
                let bone_id = bones[i];
                let setup_length = skeleton.data.bones[bone_id.index()].length;
                if setup_length < EPSILON {
                    if scale_mode {
                        lengths[i] = 0.0;
                    }
                    i += 1;
                    spaces[i] = spacing;
                } else {
                    let bone = &skeleton.bones[bone_id.index()];
                    let x = setup_length * bone.a;
                    let y = setup_length * bone.c;
                    let length = (x * x + y * y).sqrt();
                    if scale_mode {
                        lengths[i] = length;
                    }
                    i += 1;
                    spaces[i] = (if length_spacing {
                        setup_length + spacing
                    } else {
                        spacing
                    }) * length
                        / setup_length;
                }
            }
        }
    }
}

/// `spine::PathConstraint::computeWorldPositions` — walks the path
/// attachment's cubic-bezier curves and samples world positions at each
/// of the `spaces_count` step offsets accumulated from `position`.
/// Output layout: `[x0, y0, r0, x1, y1, r1, ...]` (3 floats per step,
/// plus a preamble of `[x0, y0]`; matches spine-cpp's spacing).
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn compute_world_positions(
    skeleton: &Skeleton,
    path_attachment: crate::data::AttachmentId,
    slot: crate::data::SlotId,
    initial_position: f32,
    position_mode: PositionMode,
    spacing_mode: SpacingMode,
    spaces_count: usize,
    tangents: bool,
    spaces: &[f32],
) -> Vec<f32> {
    let mut positions = vec![0.0_f32; spaces_count * 3 + 2];
    let mut prev_curve: i32 = NONE;

    let (closed, constant_speed, lengths) =
        match &skeleton.data.attachments[path_attachment.index()] {
            Attachment::Path(p) => (p.closed, p.constant_speed, p.lengths.clone()),
            _ => return positions,
        };
    let vertices_length = match &skeleton.data.attachments[path_attachment.index()] {
        Attachment::Path(p) => p.vertex_data.world_vertices_length as usize,
        _ => 0,
    };
    let mut curve_count = (vertices_length / 6) as i32;
    let position = initial_position;

    if !constant_speed {
        curve_count -= if closed { 1 } else { 2 };
        let path_length = lengths[curve_count as usize];
        let mut position_local = position;
        if position_mode == PositionMode::Percent {
            position_local *= path_length;
        }
        let multiplier = match spacing_mode {
            SpacingMode::Percent => path_length,
            SpacingMode::Proportional => path_length / spaces_count as f32,
            _ => 1.0,
        };
        let mut world = vec![0.0_f32; 8];
        let mut curve = 0_i32;
        let mut o = 0_usize;
        for i in 0..spaces_count {
            let space = spaces[i] * multiplier;
            position_local += space;
            let mut p = position_local;
            if closed {
                p %= path_length;
                if p < 0.0 {
                    p += path_length;
                }
                curve = 0;
            } else if p < 0.0 {
                if prev_curve != BEFORE {
                    prev_curve = BEFORE;
                    compute_world_vertices(skeleton, path_attachment, slot, 2, 4, &mut world, 0);
                }
                add_before_position(p, &world, 0, &mut positions, o);
                o += 3;
                continue;
            } else if p > path_length {
                if prev_curve != AFTER {
                    prev_curve = AFTER;
                    compute_world_vertices(
                        skeleton,
                        path_attachment,
                        slot,
                        vertices_length - 6,
                        4,
                        &mut world,
                        0,
                    );
                }
                add_after_position(p - path_length, &world, 0, &mut positions, o);
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
                    compute_world_vertices(
                        skeleton,
                        path_attachment,
                        slot,
                        vertices_length - 4,
                        4,
                        &mut world,
                        0,
                    );
                    compute_world_vertices(skeleton, path_attachment, slot, 0, 4, &mut world, 4);
                } else {
                    compute_world_vertices(
                        skeleton,
                        path_attachment,
                        slot,
                        (curve * 6 + 2) as usize,
                        8,
                        &mut world,
                        0,
                    );
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
                &mut positions,
                o,
                tangents || (i > 0 && space < EPSILON),
            );
            o += 3;
        }
        return positions;
    }

    // Constant-speed branch: precompute per-curve arc length first.
    let (world, curve_lengths, path_length) = precompute_curves(
        skeleton,
        path_attachment,
        slot,
        closed,
        vertices_length,
        curve_count,
    );
    let mut position_local = position;
    if position_mode == PositionMode::Percent {
        position_local *= path_length;
    }
    let multiplier = match spacing_mode {
        SpacingMode::Percent => path_length,
        SpacingMode::Proportional => path_length / spaces_count as f32,
        _ => 1.0,
    };

    let mut segments = [0.0_f32; 10];
    let mut curve = 0_i32;
    let mut segment = 0_usize;
    let mut curve_length = 0.0_f32;
    let mut x1 = 0.0_f32;
    let mut y1 = 0.0_f32;
    let mut cx1 = 0.0_f32;
    let mut cy1 = 0.0_f32;
    let mut cx2 = 0.0_f32;
    let mut cy2 = 0.0_f32;
    let mut x2 = 0.0_f32;
    let mut y2 = 0.0_f32;
    let mut o = 0_usize;
    let verts_total = world.len();
    for i in 0..spaces_count {
        let space = spaces[i] * multiplier;
        position_local += space;
        let mut p = position_local;

        if closed {
            p %= path_length;
            if p < 0.0 {
                p += path_length;
            }
            curve = 0;
        } else if p < 0.0 {
            add_before_position(p, &world, 0, &mut positions, o);
            o += 3;
            continue;
        } else if p > path_length {
            add_after_position(p - path_length, &world, verts_total - 4, &mut positions, o);
            o += 3;
            continue;
        }

        loop {
            let length = curve_lengths[curve as usize];
            if p > length {
                curve += 1;
                continue;
            }
            if curve == 0 {
                p /= length;
            } else {
                let prev = curve_lengths[(curve - 1) as usize];
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
            for ii_i in 1..8_usize {
                dfx += ddfx;
                dfy += ddfy;
                ddfx += dddfx;
                ddfy += dddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                segments[ii_i] = curve_length;
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

        // Weight by segment length.
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
            &mut positions,
            o,
            tangents || (i > 0 && space < EPSILON),
        );
        o += 3;
    }
    positions
}

/// Precompute world vertex positions + per-curve arc lengths for the
/// constant-speed branch. Returns `(world, curve_lengths, path_length)`.
fn precompute_curves(
    skeleton: &Skeleton,
    path_attachment: crate::data::AttachmentId,
    slot: crate::data::SlotId,
    closed: bool,
    vertices_length: usize,
    mut curve_count: i32,
) -> (Vec<f32>, Vec<f32>, f32) {
    let mut world;
    let effective_length;
    if closed {
        effective_length = vertices_length + 2;
        world = vec![0.0_f32; effective_length];
        compute_world_vertices(
            skeleton,
            path_attachment,
            slot,
            2,
            effective_length - 4,
            &mut world,
            0,
        );
        compute_world_vertices(
            skeleton,
            path_attachment,
            slot,
            0,
            2,
            &mut world,
            effective_length - 4,
        );
        world[effective_length - 2] = world[0];
        world[effective_length - 1] = world[1];
    } else {
        curve_count -= 1;
        effective_length = vertices_length - 4;
        world = vec![0.0_f32; effective_length];
        compute_world_vertices(
            skeleton,
            path_attachment,
            slot,
            2,
            effective_length,
            &mut world,
            0,
        );
    }

    let mut curve_lengths = vec![0.0_f32; curve_count.max(0) as usize];
    let mut path_length = 0.0_f32;
    let mut x1 = world[0];
    let mut y1 = world[1];
    let mut w = 2;
    for i in 0..curve_count as usize {
        let cx1 = world[w];
        let cy1 = world[w + 1];
        let cx2 = world[w + 2];
        let cy2 = world[w + 3];
        let x2 = world[w + 4];
        let y2 = world[w + 5];
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
        curve_lengths[i] = path_length;
        x1 = x2;
        y1 = y2;
        w += 6;
    }
    (world, curve_lengths, path_length)
}

/// Thin wrapper that pulls the path attachment's `VertexData` out of
/// `skeleton.data.attachments[…]` and delegates to the shared
/// `Skeleton::compute_world_vertices` helper (Phase 6a). Path solvers
/// always want stride = 2.
fn compute_world_vertices(
    skeleton: &Skeleton,
    path_attachment: crate::data::AttachmentId,
    slot_id: crate::data::SlotId,
    start: usize,
    count: usize,
    world_vertices: &mut [f32],
    offset: usize,
) {
    let Attachment::Path(p) = &skeleton.data.attachments[path_attachment.index()] else {
        return;
    };
    skeleton.compute_world_vertices(
        &p.vertex_data,
        slot_id,
        start,
        count,
        world_vertices,
        offset,
        2,
    );
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
