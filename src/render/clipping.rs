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

//! CPU clipping for clipping attachments.
//!
//! [`SkeletonClipping`] clips triangles against the world polygon of a
//! [`ClippingAttachment`], from its slot through its end slot in draw order.
//! A concave polygon is split into convex parts, unless the attachment is
//! marked convex or inverse, in which case its convex hull is used. An
//! inverse clip keeps what lies outside the polygon instead of inside.
//! [`SkeletonRenderer`](crate::render::SkeletonRenderer) drives it; use it
//! directly only when emitting geometry yourself.

#![allow(clippy::many_single_char_names, clippy::too_many_arguments)]

use crate::data::{Attachment, ClippingAttachment, SlotId};
use crate::math::Triangulator;
use crate::skeleton::Skeleton;

/// Clipping state for one pass over a draw order. Buffers are reused
/// between calls.
#[derive(Debug, Default, Clone)]
pub struct SkeletonClipping {
    triangulator: Triangulator,
    clipping_polygon: Vec<f32>,
    clipping_polygons: Vec<Vec<f32>>,
    polygon_count: usize,
    clip_output: Vec<f32>,
    scratch: Vec<f32>,
    inverse_vertices: Vec<f32>,
    clipped_vertices: Vec<f32>,
    clipped_triangles: Vec<u16>,
    clipped_uvs: Vec<f32>,
    end_slot: Option<SlotId>,
    active: bool,
    inverse: bool,
}

impl SkeletonClipping {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts clipping with the clipping attachment on `slot`. Returns the
    /// number of convex polygons clipped against, or 0 without starting if a
    /// clip is already active or the polygon has fewer than 3 points.
    pub fn clip_start(
        &mut self,
        skeleton: &Skeleton,
        slot: SlotId,
        clip: &ClippingAttachment,
    ) -> usize {
        if self.active {
            return 0;
        }
        let n = clip.vertex_data.world_vertices_length as usize;
        if n < 6 {
            return 0;
        }
        self.active = true;
        self.end_slot = clip.end_slot;
        self.inverse = clip.inverse;
        self.polygon_count = 0;
        self.clipping_polygon.clear();
        self.clipping_polygon.resize(n, 0.0);
        skeleton.compute_world_vertices(
            &clip.vertex_data,
            slot,
            0,
            n,
            &mut self.clipping_polygon,
            0,
            2,
        );
        let convex = make_clockwise(&mut self.clipping_polygon);
        if convex || self.inverse || clip.convex {
            if !convex {
                make_convex(&mut self.clipping_polygon, &mut self.clip_output);
            }
            let (x, y) = (self.clipping_polygon[0], self.clipping_polygon[1]);
            self.clipping_polygon.push(x);
            self.clipping_polygon.push(y);
            self.push_polygon_from_own();
        } else {
            let max_points = self.clipping_polygon.len() + 2;
            let polygons = self.triangulator.triangulate_convex(&self.clipping_polygon);
            for (i, p) in polygons.iter().enumerate() {
                if self.clipping_polygons.len() <= i {
                    self.clipping_polygons.push(Vec::new());
                }
                // Sized for the largest part so later frames don't regrow it.
                let dst = &mut self.clipping_polygons[i];
                dst.clear();
                dst.reserve(max_points);
                dst.extend_from_slice(p);
            }
            self.polygon_count = polygons.len();
        }
        self.polygon_count
    }

    fn push_polygon_from_own(&mut self) {
        if self.clipping_polygons.is_empty() {
            self.clipping_polygons.push(Vec::new());
        }
        self.clipping_polygons[0].clone_from(&self.clipping_polygon);
        self.polygon_count = 1;
    }

    /// Ends clipping if `slot` is the clip's end slot. Call after each slot
    /// in draw order, whether or not it drew.
    pub fn clip_end_slot(&mut self, slot: SlotId) {
        if self.active && self.end_slot == Some(slot) {
            self.clip_end();
        }
    }

    /// Ends clipping unconditionally. Call at the end of the draw order.
    pub fn clip_end(&mut self) {
        self.active = false;
        self.polygon_count = 0;
    }

    /// Whether a clip is active, so drawn geometry should go through
    /// [`Self::clip_triangles`].
    #[must_use]
    pub fn is_clipping(&self) -> bool {
        self.active
    }

    /// Clipped world positions from the last [`Self::clip_triangles`], as
    /// interleaved `x, y`.
    #[must_use]
    pub fn clipped_vertices(&self) -> &[f32] {
        &self.clipped_vertices
    }

    /// Triangle list indexing [`Self::clipped_vertices`].
    #[must_use]
    pub fn clipped_triangles(&self) -> &[u16] {
        &self.clipped_triangles
    }

    /// UVs parallel to [`Self::clipped_vertices`], interpolated from the
    /// input triangles.
    #[must_use]
    pub fn clipped_uvs(&self) -> &[f32] {
        &self.clipped_uvs
    }

    /// Clips a triangle list into the `clipped_*` buffers. Positions are read
    /// every `stride` floats of `vertices`; `uvs` are always 2 floats per
    /// vertex. Triangles fully inside the clip are copied through and those
    /// fully outside are dropped. Returns whether any triangle was cut, and
    /// always `true` for an inverse clip.
    ///
    /// Call only while [`Self::is_clipping`]. Panics if `triangles` indexes
    /// past `vertices` or `uvs`.
    pub fn clip_triangles(
        &mut self,
        vertices: &[f32],
        triangles: &[u16],
        uvs: &[f32],
        stride: usize,
    ) -> bool {
        self.clipped_vertices.clear();
        self.clipped_triangles.clear();
        self.clipped_uvs.clear();
        let mut index: u16 = 0;

        if self.inverse {
            for tri in triangles.as_chunks::<3>().0 {
                let (t0, t1, t2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                let (x1, y1) = (vertices[t0 * stride], vertices[t0 * stride + 1]);
                let (x2, y2) = (vertices[t1 * stride], vertices[t1 * stride + 1]);
                let (x3, y3) = (vertices[t2 * stride], vertices[t2 * stride + 1]);
                clip_inverse(
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    &self.clipping_polygons[0],
                    &mut self.clip_output,
                    &mut self.scratch,
                    &mut self.inverse_vertices,
                );
                let nn = self.inverse_vertices.len();
                if nn == 0 {
                    continue;
                }
                let (u1, v1) = (uvs[t0 << 1], uvs[(t0 << 1) + 1]);
                let (u2, v2) = (uvs[t1 << 1], uvs[(t1 << 1) + 1]);
                let (u3, v3) = (uvs[t2 << 1], uvs[(t2 << 1) + 1]);
                let d0 = y2 - y3;
                let d1 = x3 - x2;
                let d2 = x1 - x3;
                let d4 = y3 - y1;
                let d = 1.0 / (d0 * d2 + d1 * (y1 - y3));
                let iv = &self.inverse_vertices;
                let mut offset = 0;
                while offset < nn {
                    let polygon_size = iv[offset] as usize;
                    offset += 1;
                    let vertex_count = polygon_size >> 1;
                    for ii in (0..polygon_size).step_by(2) {
                        let (x, y) = (iv[offset + ii], iv[offset + ii + 1]);
                        self.clipped_vertices.push(x);
                        self.clipped_vertices.push(y);
                        let c0 = x - x3;
                        let c1 = y - y3;
                        let a = (d0 * c0 + d1 * c1) * d;
                        let b = (d4 * c0 + d2 * c1) * d;
                        let c = 1.0 - a - b;
                        self.clipped_uvs.push(u1 * a + u2 * b + u3 * c);
                        self.clipped_uvs.push(v1 * a + v2 * b + v3 * c);
                    }
                    push_fan(&mut self.clipped_triangles, index, vertex_count);
                    index += vertex_count as u16;
                    offset += polygon_size;
                }
            }
            return true;
        }

        let mut clipped = false;
        for tri in triangles.as_chunks::<3>().0 {
            let (t0, t1, t2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (x1, y1) = (vertices[t0 * stride], vertices[t0 * stride + 1]);
            let (u1, v1) = (uvs[t0 << 1], uvs[(t0 << 1) + 1]);
            let (x2, y2) = (vertices[t1 * stride], vertices[t1 * stride + 1]);
            let (u2, v2) = (uvs[t1 << 1], uvs[(t1 << 1) + 1]);
            let (x3, y3) = (vertices[t2 * stride], vertices[t2 * stride + 1]);
            let (u3, v3) = (uvs[t2 << 1], uvs[(t2 << 1) + 1]);
            let (mut d0, mut d1, mut d2, mut d4, mut d) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for p in 0..self.polygon_count {
                if clip(
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                    &self.clipping_polygons[p],
                    &mut self.clip_output,
                    &mut self.scratch,
                ) {
                    let len = self.clip_output.len();
                    if len == 0 {
                        continue;
                    }
                    clipped = true;
                    let count = len >> 1;
                    if d == 0.0 {
                        d0 = y2 - y3;
                        d1 = x3 - x2;
                        d2 = x1 - x3;
                        d4 = y3 - y1;
                        d = 1.0 / (d0 * d2 - d1 * d4);
                    }
                    for ii in (0..len).step_by(2) {
                        let (x, y) = (self.clip_output[ii], self.clip_output[ii + 1]);
                        self.clipped_vertices.push(x);
                        self.clipped_vertices.push(y);
                        let c0 = x - x3;
                        let c1 = y - y3;
                        let a = (d0 * c0 + d1 * c1) * d;
                        let b = (d4 * c0 + d2 * c1) * d;
                        let c = 1.0 - a - b;
                        self.clipped_uvs.push(u1 * a + u2 * b + u3 * c);
                        self.clipped_uvs.push(v1 * a + v2 * b + v3 * c);
                    }
                    push_fan(&mut self.clipped_triangles, index, count);
                    index += count as u16;
                } else {
                    self.clipped_vertices
                        .extend_from_slice(&[x1, y1, x2, y2, x3, y3]);
                    self.clipped_uvs
                        .extend_from_slice(&[u1, v1, u2, v2, u3, v3]);
                    self.clipped_triangles
                        .extend_from_slice(&[index, index + 1, index + 2]);
                    index += 3;
                    break;
                }
            }
        }
        clipped
    }

    /// As [`Self::clip_triangles`] with zeroed UVs, for callers that only
    /// need positions. Allocates the UV buffer on each call.
    pub fn clip_triangles_positions(&mut self, vertices: &[f32], triangles: &[u16]) -> bool {
        let uvs = vec![0.0; vertices.len()];
        self.clip_triangles(vertices, triangles, &uvs, 2)
    }
}

/// Triangle fan over `vertex_count` vertices starting at `index`.
fn push_fan(triangles: &mut Vec<u16>, index: u16, vertex_count: usize) {
    for ii in 1..vertex_count.saturating_sub(1) {
        let ii = ii as u16;
        triangles.extend_from_slice(&[index, index + ii, index + ii + 1]);
    }
}

/// Sutherland-Hodgman clip of one triangle against a closed convex
/// polygon. The result, without its closing point, is left in `output`.
/// Returns whether the triangle was clipped; an empty `output` means it
/// was clipped away entirely.
fn clip(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    polygon: &[f32],
    clip_output: &mut Vec<f32>,
    scratch: &mut Vec<f32>,
) -> bool {
    let mut clipped = false;
    // Start so the final pass writes into `clip_output`.
    let mut in_is_output = polygon.len() % 4 >= 2;
    {
        let input = if in_is_output {
            &mut *clip_output
        } else {
            &mut *scratch
        };
        input.clear();
        input.extend_from_slice(&[x1, y1, x2, y2, x3, y3, x1, y1]);
    }
    if in_is_output {
        scratch.clear();
    } else {
        clip_output.clear();
    }
    let last = polygon.len() as isize - 4;
    let mut i = 0usize;
    loop {
        let (input, output) = if in_is_output {
            (&*clip_output, &mut *scratch)
        } else {
            (&*scratch, &mut *clip_output)
        };
        let (edge_x, edge_y) = (polygon[i], polygon[i + 1]);
        let ex = edge_x - polygon[i + 2];
        let ey = edge_y - polygon[i + 3];
        let output_start = output.len();
        let (mut px, mut py) = (input[0], input[1]);
        let mut s1 = ey * (edge_x - px) - ex * (edge_y - py);
        let nn = input.len() - 2;
        let mut ii = 2;
        while ii <= nn {
            let (qx, qy) = (input[ii], input[ii + 1]);
            let s2 = ey * (edge_x - qx) - ex * (edge_y - qy);
            if s1 > 0.0 {
                if s2 > 0.0 {
                    output.push(qx);
                    output.push(qy);
                } else {
                    let ix = qx - px;
                    let iy = qy - py;
                    let t = s1 / (ix * ey - iy * ex);
                    if (0.0..=1.0).contains(&t) {
                        output.push(px + ix * t);
                        output.push(py + iy * t);
                        clipped = true;
                    } else {
                        output.push(qx);
                        output.push(qy);
                    }
                }
            } else if s2 > 0.0 {
                let ix = qx - px;
                let iy = qy - py;
                let t = s1 / (ix * ey - iy * ex);
                if (0.0..=1.0).contains(&t) {
                    output.push(px + ix * t);
                    output.push(py + iy * t);
                    output.push(qx);
                    output.push(qy);
                    clipped = true;
                } else {
                    output.push(qx);
                    output.push(qy);
                }
            } else {
                clipped = true;
            }
            px = qx;
            py = qy;
            s1 = s2;
            ii += 2;
        }
        if output_start == output.len() {
            clip_output.clear();
            return true;
        }
        let (fx, fy) = (output[0], output[1]);
        output.push(fx);
        output.push(fy);
        if i as isize == last {
            break;
        }
        // Swap roles; the old input becomes the next output.
        in_is_output = !in_is_output;
        if in_is_output {
            scratch.clear();
        } else {
            clip_output.clear();
        }
        i += 2;
    }
    // The output of the last pass is the buffer that isn't the input.
    if in_is_output {
        clip_output.clear();
        clip_output.extend_from_slice(&scratch[..scratch.len() - 2]);
    } else {
        clip_output.truncate(clip_output.len() - 2);
    }
    clipped
}

/// Pieces of a triangle outside a closed convex polygon, written to
/// `inverse` as size-prefixed runs.
fn clip_inverse(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    polygon: &[f32],
    clip_output: &mut Vec<f32>,
    scratch: &mut Vec<f32>,
    inverse: &mut Vec<f32>,
) {
    inverse.clear();
    let last = polygon.len() as isize - 4;
    let mut in_is_output = polygon.len() % 4 >= 2;
    {
        let input = if in_is_output {
            &mut *clip_output
        } else {
            &mut *scratch
        };
        input.clear();
        input.extend_from_slice(&[x1, y1, x2, y2, x3, y3, x1, y1]);
    }
    if in_is_output {
        scratch.clear();
    } else {
        clip_output.clear();
    }
    let mut i = 0usize;
    loop {
        let (input, output) = if in_is_output {
            (&*clip_output, &mut *scratch)
        } else {
            (&*scratch, &mut *clip_output)
        };
        let (edge_x, edge_y) = (polygon[i], polygon[i + 1]);
        let ex = edge_x - polygon[i + 2];
        let ey = edge_y - polygon[i + 3];
        let output_start = output.len();
        let fragment_start = inverse.len();
        inverse.push(0.0);
        let (mut px, mut py) = (input[0], input[1]);
        let mut s1 = ey * (edge_x - px) - ex * (edge_y - py);
        let nn = input.len() - 2;
        let mut ii = 2;
        while ii <= nn {
            let (qx, qy) = (input[ii], input[ii + 1]);
            let s2 = ey * (edge_x - qx) - ex * (edge_y - qy);
            if s1 > 0.0 {
                if s2 > 0.0 {
                    output.push(qx);
                    output.push(qy);
                } else {
                    let ix = qx - px;
                    let iy = qy - py;
                    let t = s1 / (ix * ey - iy * ex);
                    if (0.0..=1.0).contains(&t) {
                        let (cx, cy) = (px + ix * t, py + iy * t);
                        output.push(cx);
                        output.push(cy);
                        inverse.extend_from_slice(&[cx, cy, qx, qy]);
                    } else {
                        output.push(qx);
                        output.push(qy);
                    }
                }
            } else if s2 > 0.0 {
                let ix = qx - px;
                let iy = qy - py;
                let t = s1 / (ix * ey - iy * ex);
                if (0.0..=1.0).contains(&t) {
                    let (cx, cy) = (px + ix * t, py + iy * t);
                    inverse.push(cx);
                    inverse.push(cy);
                    output.extend_from_slice(&[cx, cy, qx, qy]);
                } else {
                    output.push(qx);
                    output.push(qy);
                }
            } else {
                inverse.push(qx);
                inverse.push(qy);
            }
            px = qx;
            py = qy;
            s1 = s2;
            ii += 2;
        }
        let fragment_size = inverse.len() - fragment_start - 1;
        if fragment_size >= 6 {
            inverse[fragment_start] = fragment_size as f32;
        } else {
            inverse.truncate(fragment_start);
        }
        if output_start == output.len() {
            break;
        }
        let (fx, fy) = (output[0], output[1]);
        output.push(fx);
        output.push(fy);
        if i as isize == last {
            break;
        }
        in_is_output = !in_is_output;
        if in_is_output {
            scratch.clear();
        } else {
            clip_output.clear();
        }
        i += 2;
    }
}

/// Makes the polygon clockwise; returns whether it is convex.
pub(crate) fn make_clockwise(v: &mut [f32]) -> bool {
    let n = v.len();
    let (mut no_cw, mut no_ccw) = (true, true);
    let mut area = 0.0;
    let (mut prev_x, mut prev_y) = (v[n - 2], v[n - 1]);
    let (mut curr_x, mut curr_y) = (v[0], v[1]);
    let mut i = 2;
    while i < n {
        let (next_x, next_y) = (v[i], v[i + 1]);
        area += curr_x * next_y - next_x * curr_y;
        let cross = (curr_x - prev_x) * (next_y - curr_y) - (curr_y - prev_y) * (next_x - curr_x);
        no_ccw &= cross <= 0.0;
        no_cw &= cross >= 0.0;
        prev_x = curr_x;
        prev_y = curr_y;
        curr_x = next_x;
        curr_y = next_y;
        i += 2;
    }
    area += curr_x * v[1] - v[0] * curr_y;
    let cross = (curr_x - prev_x) * (v[1] - curr_y) - (curr_y - prev_y) * (v[0] - curr_x);
    no_ccw &= cross <= 0.0;
    no_cw &= cross >= 0.0;
    if area >= 0.0 {
        let last_x = n - 2;
        let half = n >> 1;
        let mut i = 0;
        while i < half {
            let other = last_x - i;
            v.swap(i, other);
            v.swap(i + 1, other + 1);
            i += 2;
        }
        return no_cw;
    }
    no_ccw
}

/// Replaces the polygon with its convex hull (monotone chain).
fn make_convex(polygon: &mut Vec<f32>, sorted: &mut Vec<f32>) {
    let n = polygon.len();
    sorted.clear();
    sorted.extend_from_slice(polygon);
    let mut i = 2;
    while i < n {
        let (x, y) = (sorted[i], sorted[i + 1]);
        let mut p = i as isize - 2;
        while p >= 0
            && (sorted[p as usize] > x || (sorted[p as usize] == x && sorted[p as usize + 1] > y))
        {
            sorted[p as usize + 2] = sorted[p as usize];
            sorted[p as usize + 3] = sorted[p as usize + 1];
            p -= 2;
        }
        sorted[(p + 2) as usize] = x;
        sorted[(p + 3) as usize] = y;
        i += 2;
    }
    // The hull can briefly need a few more slots than the input.
    polygon.resize(n + 4, 0.0);
    let v = polygon;
    v[..4].copy_from_slice(&sorted[..4]);
    let turn = |v: &[f32], s: usize, x: f32, y: f32| {
        (v[s - 2] - v[s - 4]) * (y - v[s - 3]) - (v[s - 1] - v[s - 3]) * (x - v[s - 4])
    };
    let mut s = 4;
    let mut i = 4;
    while i < n {
        let (x, y) = (sorted[i], sorted[i + 1]);
        while turn(v, s, x, y) >= 0.0 {
            s -= 2;
            if s == 2 {
                break;
            }
        }
        v[s] = x;
        v[s + 1] = y;
        i += 2;
        s += 2;
    }
    v[s] = sorted[n - 4];
    v[s + 1] = sorted[n - 3];
    let t = s;
    s += 2;
    let mut i = n as isize - 6;
    while i >= 0 {
        let (x, y) = (sorted[i as usize], sorted[i as usize + 1]);
        while turn(v, s, x, y) >= 0.0 {
            s -= 2;
            if s == t {
                break;
            }
        }
        v[s] = x;
        v[s + 1] = y;
        i -= 2;
        s += 2;
    }
    v.truncate(s - 2);
}

/// The clipping data of `attachment`, or `None` for any other kind.
#[must_use]
pub fn as_clipping(attachment: &Attachment) -> Option<&ClippingAttachment> {
    match attachment {
        Attachment::Clipping(c) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(v: &[f32], tris: &[u16]) -> f32 {
        tris.as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let p = |i: u16| (v[i as usize * 2], v[i as usize * 2 + 1]);
                let ((ax, ay), (bx, by), (cx, cy)) = (p(t[0]), p(t[1]), p(t[2]));
                ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() * 0.5
            })
            .sum()
    }

    /// Clipper with a closed clockwise unit square as its only polygon.
    fn square_clipper(inverse: bool) -> SkeletonClipping {
        let mut c = SkeletonClipping::new();
        let mut square = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        assert!(make_clockwise(&mut square));
        square.extend_from_slice(&[square[0], square[1]]);
        c.clipping_polygons = vec![square];
        c.polygon_count = 1;
        c.active = true;
        c.inverse = inverse;
        c
    }

    #[test]
    fn clip_and_inverse_partition_a_triangle() {
        // Right triangle over the square's corner; [0.5, 1]² lies inside.
        let tri = [0.5, 0.5, 1.5, 0.5, 0.5, 1.5];
        let uvs = [0.0; 6];
        let total = 0.5;

        let mut inside = square_clipper(false);
        assert!(inside.clip_triangles(&tri, &[0, 1, 2], &uvs, 2));
        let a_in = area(inside.clipped_vertices(), inside.clipped_triangles());

        let mut outside = square_clipper(true);
        assert!(outside.clip_triangles(&tri, &[0, 1, 2], &uvs, 2));
        let a_out = area(outside.clipped_vertices(), outside.clipped_triangles());

        assert!((a_in - 0.25).abs() < 1e-5, "inside {a_in}");
        assert!(
            (a_in + a_out - total).abs() < 1e-5,
            "inside {a_in} outside {a_out}"
        );
    }

    #[test]
    fn inverse_keeps_triangle_fully_outside() {
        let tri = [2.0, 2.0, 3.0, 2.0, 2.0, 3.0];
        let mut outside = square_clipper(true);
        outside.clip_triangles(&tri, &[0, 1, 2], &[0.0; 6], 2);
        let a = area(outside.clipped_vertices(), outside.clipped_triangles());
        assert!((a - 0.5).abs() < 1e-5, "{a}");
    }

    #[test]
    fn make_convex_takes_the_hull() {
        // Square with a notch; the hull is the square.
        let mut notched = vec![0.0, 0.0, 0.5, 0.2, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let mut scratch = Vec::new();
        assert!(!make_clockwise(&mut notched));
        make_convex(&mut notched, &mut scratch);
        assert_eq!(notched.len(), 8, "{notched:?}");
    }
}
