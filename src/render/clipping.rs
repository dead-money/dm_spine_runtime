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

//! `SkeletonClipping` — clips triangle streams against a
//! `ClippingAttachment`'s polygon, producing a new triangle fan per
//! clipped triangle.
//!
//! Literal port of `spine-cpp/src/spine/SkeletonClipping.cpp`. The
//! clipper is used by the render walker (6e wiring): for every slot
//! that carries a `ClippingAttachment`, `clip_start` begins a clip
//! region; subsequent slots' emitted triangles are routed through
//! `clip_triangles`; `clip_end(slot)` closes the region when the
//! `end_slot` is reached.

#![allow(clippy::many_single_char_names)] // spine-cpp short names are preserved for diff parity.

use crate::data::{Attachment, SlotId};
use crate::math::Triangulator;
use crate::skeleton::Skeleton;

/// Stateful clipper, mirroring `spine-cpp`'s `SkeletonClipping`. One
/// instance per `SkeletonRenderer`; scratch buffers are reused across
/// frames.
#[derive(Debug, Default)]
pub struct SkeletonClipping {
    triangulator: Triangulator,

    /// The clipping attachment's world-space polygon (post-makeClockwise).
    clipping_polygon: Vec<f32>,
    /// Convex decomposition of `clipping_polygon`. Each sub-polygon has
    /// its first vertex duplicated at the end (closing the loop) so
    /// the clip-edge walk sees `n - 4` unique edges.
    clipping_polygons: Vec<Vec<f32>>,

    /// Output-accumulator for a single `clip` call — the clipped
    /// polygon expressed as interleaved `x, y` vertices.
    clip_output: Vec<f32>,
    /// Second scratch buffer used by `clip` to ping-pong between the
    /// "input" (previous edge's output) and the "output" (current
    /// edge's accumulator).
    scratch: Vec<f32>,

    /// Accumulated clipped vertices / triangles / UVs across all
    /// triangles in one `clip_triangles` call. Consumed by the
    /// renderer to build a single post-clip `RenderCommand`.
    clipped_vertices: Vec<f32>,
    clipped_triangles: Vec<u16>,
    clipped_uvs: Vec<f32>,

    /// The slot that started the current clip region (`None` when no
    /// clip is active). Used to close the clip on the matching
    /// `end_slot`.
    active_slot: Option<SlotId>,
    /// Attachment-pointer surrogate: the `ClippingAttachment`'s
    /// `end_slot` stored at `clip_start` time, consulted by
    /// `clip_end(slot)`.
    active_end_slot: Option<SlotId>,
}

impl SkeletonClipping {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin clipping against the `ClippingAttachment` currently
    /// attached to `slot`. Computes the world-space clipping polygon,
    /// orients it clockwise, and decomposes it into convex sub-polygons.
    ///
    /// Returns the number of convex sub-polygons (zero if already
    /// clipping or if the attachment isn't actually a clipping
    /// attachment).
    pub fn clip_start(&mut self, skeleton: &Skeleton, slot_id: SlotId) -> usize {
        if self.active_slot.is_some() {
            return 0;
        }
        let Some(attachment_id) = skeleton.slots[slot_id.index()].attachment else {
            return 0;
        };
        let Attachment::Clipping(clip) = &skeleton.data.attachments[attachment_id.index()] else {
            return 0;
        };

        let n = clip.vertex_data.world_vertices_length as usize;
        self.clipping_polygon.clear();
        self.clipping_polygon.resize(n, 0.0);
        skeleton.compute_world_vertices(
            &clip.vertex_data,
            slot_id,
            0,
            n,
            &mut self.clipping_polygon,
            0,
            2,
        );
        make_clockwise(&mut self.clipping_polygon);

        // Decompose into convex polygons. The triangulator returns a
        // `&[u16]` triangle list, and `decompose` then merges adjacent
        // convex fans back into the smallest set of convex polygons.
        let triangles: Vec<u16> = self
            .triangulator
            .triangulate(&self.clipping_polygon)
            .to_vec();
        let polygons: Vec<Vec<f32>> = self
            .triangulator
            .decompose(&self.clipping_polygon, &triangles)
            .to_vec();
        self.clipping_polygons = polygons;

        // Re-orient each convex sub-polygon clockwise and duplicate
        // its first vertex at the end (closing the loop for edge
        // iteration in `clip`).
        for poly in &mut self.clipping_polygons {
            make_clockwise(poly);
            let (x0, y0) = (poly[0], poly[1]);
            poly.push(x0);
            poly.push(y0);
        }

        self.active_slot = Some(slot_id);
        self.active_end_slot = Some(clip.end_slot);
        self.clipping_polygons.len()
    }

    /// End clipping if `slot` is the active clipping region's
    /// `end_slot` (matches spine-cpp's `clipEnd(Slot&)`).
    pub fn clip_end_on(&mut self, slot_id: SlotId) {
        if self.active_end_slot == Some(slot_id) {
            self.clip_end();
        }
    }

    /// Unconditionally end clipping and discard all per-region state.
    pub fn clip_end(&mut self) {
        self.active_slot = None;
        self.active_end_slot = None;
        self.clipping_polygon.clear();
        self.clipping_polygons.clear();
        self.clipped_vertices.clear();
        self.clipped_triangles.clear();
        self.clipped_uvs.clear();
    }

    /// `true` iff a clip region is currently active.
    #[must_use]
    pub fn is_clipping(&self) -> bool {
        self.active_slot.is_some()
    }

    /// Clip a triangle list against the active clipping polygons.
    /// `vertices` is interleaved `x, y` with `stride` floats between
    /// consecutive vertices (typically 2). `uvs` has the same stride.
    /// Outputs go into the `clipped_*` accumulators — read with
    /// [`Self::clipped_vertices`] etc.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    pub fn clip_triangles(
        &mut self,
        vertices: &[f32],
        triangles: &[u16],
        uvs: &[f32],
        stride: usize,
    ) {
        self.clipped_vertices.clear();
        self.clipped_triangles.clear();
        self.clipped_uvs.clear();

        let polygons_count = self.clipping_polygons.len();
        if polygons_count == 0 {
            return;
        }

        let mut index: u16 = 0;
        let mut i = 0_usize;
        'outer: while i < triangles.len() {
            let vo0 = triangles[i] as usize * stride;
            let (x1, y1) = (vertices[vo0], vertices[vo0 + 1]);
            let (u1, v1) = (uvs[vo0], uvs[vo0 + 1]);

            let vo1 = triangles[i + 1] as usize * stride;
            let (x2, y2) = (vertices[vo1], vertices[vo1 + 1]);
            let (u2, v2) = (uvs[vo1], uvs[vo1 + 1]);

            let vo2 = triangles[i + 2] as usize * stride;
            let (x3, y3) = (vertices[vo2], vertices[vo2 + 1]);
            let (u3, v3) = (uvs[vo2], uvs[vo2 + 1]);

            for p in 0..polygons_count {
                let s = self.clipped_vertices.len();

                // Take the clipping polygon out of self so the mutable
                // borrow on self.clip_output / self.scratch doesn't
                // conflict. Put it back at end of iteration.
                let polygon = std::mem::take(&mut self.clipping_polygons[p]);
                let clipped = clip(
                    x1, y1, x2, y2, x3, y3,
                    &polygon,
                    &mut self.clip_output,
                    &mut self.scratch,
                );
                self.clipping_polygons[p] = polygon;

                if clipped {
                    let clip_len = self.clip_output.len();
                    if clip_len == 0 {
                        continue;
                    }
                    // Barycentric projection for UVs. spine-cpp:
                    // d0=y2-y3, d1=x3-x2, d2=x1-x3, d4=y3-y1,
                    // d = 1/(d0*d2 + d1*(y1 - y3)).
                    let d0 = y2 - y3;
                    let d1 = x3 - x2;
                    let d2 = x1 - x3;
                    let d4 = y3 - y1;
                    let d = 1.0 / (d0 * d2 + d1 * (y1 - y3));

                    let clip_count = clip_len >> 1;
                    self.clipped_vertices.resize(s + clip_count * 2, 0.0);
                    self.clipped_uvs.resize(s + clip_count * 2, 0.0);
                    let mut write = s;
                    let mut ii = 0;
                    while ii < clip_len {
                        let x = self.clip_output[ii];
                        let y = self.clip_output[ii + 1];
                        self.clipped_vertices[write] = x;
                        self.clipped_vertices[write + 1] = y;
                        let c0 = x - x3;
                        let c1 = y - y3;
                        let a = (d0 * c0 + d1 * c1) * d;
                        let b = (d4 * c0 + d2 * c1) * d;
                        let c = 1.0 - a - b;
                        self.clipped_uvs[write] = u1 * a + u2 * b + u3 * c;
                        self.clipped_uvs[write + 1] = v1 * a + v2 * b + v3 * c;
                        write += 2;
                        ii += 2;
                    }

                    // Fan-triangulate the clipped polygon.
                    let t_s = self.clipped_triangles.len();
                    self.clipped_triangles
                        .resize(t_s + 3 * (clip_count - 2), 0);
                    let fan_count = clip_count - 1;
                    let mut ts = t_s;
                    for ii in 1..fan_count {
                        self.clipped_triangles[ts] = index;
                        self.clipped_triangles[ts + 1] = index + ii as u16;
                        self.clipped_triangles[ts + 2] = index + ii as u16 + 1;
                        ts += 3;
                    }
                    index += fan_count as u16 + 1;
                } else {
                    // Triangle lies entirely inside the clip polygon —
                    // emit it as-is and advance past the triangle
                    // (spine-cpp: `i += 3; goto continue_outer`).
                    let vs = s;
                    self.clipped_vertices.resize(vs + 6, 0.0);
                    self.clipped_uvs.resize(vs + 6, 0.0);
                    self.clipped_vertices[vs] = x1;
                    self.clipped_vertices[vs + 1] = y1;
                    self.clipped_vertices[vs + 2] = x2;
                    self.clipped_vertices[vs + 3] = y2;
                    self.clipped_vertices[vs + 4] = x3;
                    self.clipped_vertices[vs + 5] = y3;
                    self.clipped_uvs[vs] = u1;
                    self.clipped_uvs[vs + 1] = v1;
                    self.clipped_uvs[vs + 2] = u2;
                    self.clipped_uvs[vs + 3] = v2;
                    self.clipped_uvs[vs + 4] = u3;
                    self.clipped_uvs[vs + 5] = v3;

                    let ts = self.clipped_triangles.len();
                    self.clipped_triangles.resize(ts + 3, 0);
                    self.clipped_triangles[ts] = index;
                    self.clipped_triangles[ts + 1] = index + 1;
                    self.clipped_triangles[ts + 2] = index + 2;
                    index += 3;
                    i += 3;
                    continue 'outer;
                }
            }
            i += 3;
        }
    }

    /// Clipped triangle-list output: interleaved world-space
    /// `x, y` per vertex.
    #[must_use]
    pub fn clipped_vertices(&self) -> &[f32] {
        &self.clipped_vertices
    }

    /// Clipped triangle-list index buffer into [`Self::clipped_vertices`].
    #[must_use]
    pub fn clipped_triangles(&self) -> &[u16] {
        &self.clipped_triangles
    }

    /// Clipped triangle-list UVs, one pair per vertex of [`Self::clipped_vertices`].
    #[must_use]
    pub fn clipped_uvs(&self) -> &[f32] {
        &self.clipped_uvs
    }
}

/// Orient `polygon` (interleaved `x, y` pairs) clockwise. Literal
/// port of `SkeletonClipping::makeClockwise` — computes the signed
/// area, returns if already positive (clockwise in spine-cpp's
/// y-down convention), otherwise reverses vertex order.
fn make_clockwise(polygon: &mut [f32]) {
    let vlen = polygon.len();
    if vlen < 6 {
        return;
    }
    let mut area = polygon[vlen - 2] * polygon[1] - polygon[0] * polygon[vlen - 1];
    let mut i = 0;
    while i + 3 < vlen - 1 {
        let p1x = polygon[i];
        let p1y = polygon[i + 1];
        let p2x = polygon[i + 2];
        let p2y = polygon[i + 3];
        area += p1x * p2y - p2x * p1y;
        i += 2;
    }
    if area < 0.0 {
        return;
    }
    // Reverse x/y pairs in place.
    let last_x = vlen - 2;
    let mut i = 0;
    let n = vlen >> 1;
    while i < n {
        let other = last_x - i;
        polygon.swap(i, other);
        polygon.swap(i + 1, other + 1);
        i += 2;
    }
}

/// Sutherland-Hodgman convex-polygon clip of triangle
/// `(x1,y1)-(x2,y2)-(x3,y3)` against `clipping_area`.
///
/// Writes the clipped polygon into `output` as interleaved `x, y`
/// pairs. `scratch` is an auxiliary buffer; `output` and `scratch`
/// ping-pong each edge so neither needs per-edge allocation.
///
/// Returns `true` if any clipping occurred, `false` if the triangle
/// lies entirely inside the clipping area. When the triangle is
/// completely outside, returns `true` and leaves `output` empty.
/// Literal port of `SkeletonClipping::clip`.
#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
fn clip(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    clipping_area: &[f32],
    output: &mut Vec<f32>,
    scratch: &mut Vec<f32>,
) -> bool {
    // Match spine-cpp's "avoid one final copy" trick: pick which
    // buffer ends up holding the result based on the number of clip
    // edges (so the last ping-pong lands in `output`).
    let use_output_as_input = clipping_area.len() % 4 >= 2;

    output.clear();
    scratch.clear();

    // Seed the "input" buffer with the triangle (first vertex
    // duplicated at end to close the loop).
    let (input_buf, output_buf): (&mut Vec<f32>, &mut Vec<f32>) = if use_output_as_input {
        (output, scratch)
    } else {
        (scratch, output)
    };
    input_buf.extend_from_slice(&[x1, y1, x2, y2, x3, y3, x1, y1]);

    // Shadow the named bindings so swap ops don't require re-borrowing.
    // Swapping `Vec` contents via std::mem::swap is a pointer swap.
    let mut input: Vec<f32> = std::mem::take(input_buf);
    let mut current_output: Vec<f32> = std::mem::take(output_buf);

    let mut clipped = false;
    let clipping_vertices_last = clipping_area.len() - 4;
    let mut i = 0;
    loop {
        let edge_x = clipping_area[i];
        let edge_y = clipping_area[i + 1];
        let ex = edge_x - clipping_area[i + 2];
        let ey = edge_y - clipping_area[i + 3];

        let output_start = current_output.len();
        let n = input.len() - 2;
        let mut ii = 0;
        while ii < n {
            let input_x = input[ii];
            let input_y = input[ii + 1];
            ii += 2;
            let input_x2 = input[ii];
            let input_y2 = input[ii + 1];
            let s2 = ey * (edge_x - input_x2) > ex * (edge_y - input_y2);
            let s1 = ey * (edge_x - input_x) - ex * (edge_y - input_y);
            if s1 > 0.0 {
                if s2 {
                    // v1 inside, v2 inside
                    current_output.push(input_x2);
                    current_output.push(input_y2);
                    continue;
                }
                // v1 inside, v2 outside
                let ix = input_x2 - input_x;
                let iy = input_y2 - input_y;
                let t = s1 / (ix * ey - iy * ex);
                if (0.0..=1.0).contains(&t) {
                    current_output.push(input_x + ix * t);
                    current_output.push(input_y + iy * t);
                } else {
                    current_output.push(input_x2);
                    current_output.push(input_y2);
                }
            } else if s2 {
                // v1 outside, v2 inside
                let ix = input_x2 - input_x;
                let iy = input_y2 - input_y;
                let t = s1 / (ix * ey - iy * ex);
                if (0.0..=1.0).contains(&t) {
                    current_output.push(input_x + ix * t);
                    current_output.push(input_y + iy * t);
                    current_output.push(input_x2);
                    current_output.push(input_y2);
                } else {
                    current_output.push(input_x2);
                    current_output.push(input_y2);
                    continue;
                }
            }
            clipped = true;
        }

        if output_start == current_output.len() {
            // Every edge of the current polygon was culled — the
            // triangle is entirely outside this clip region.
            output.clear();
            // Restore the buffers we took ownership of.
            scratch.clear();
            // `input` / `current_output` currently hold borrowed state;
            // putting them back here keeps the helpers reusable.
            let _ = input;
            let _ = current_output;
            return true;
        }

        // Close the polygon (duplicate first vertex at end) in
        // preparation for the next edge or the final trim.
        current_output.push(current_output[0]);
        current_output.push(current_output[1]);

        if i == clipping_vertices_last {
            break;
        }
        // Swap input <-> current_output. Clear the new "output" first.
        std::mem::swap(&mut input, &mut current_output);
        current_output.clear();
        i += 2;
    }

    // Determine which of `output` / `scratch` the caller cares about.
    // `current_output` now holds the final ping-pong result; we need
    // it landed in `output`.
    let final_result = current_output;
    let other_buf = input;

    // Return the scratch/output buffers to self in the right slots.
    // `use_output_as_input` was chosen so that the final result lands
    // in the caller's `output`; verify and restore the buffers
    // accordingly.
    if use_output_as_input {
        // output was `input` at loop entry; after swaps, `output` is
        // now one of the two locals. We don't know which without
        // tracking; handle both cases by writing the final result
        // back to the output slot passed in, and any leftover to
        // scratch.
        *output = final_result;
        *scratch = other_buf;
    } else {
        *output = final_result;
        *scratch = other_buf;
    }

    // Trim the trailing closing vertex (duplicate of first) — matches
    // spine-cpp's `originalOutput->setSize(size - 2, 0)`.
    if output.len() >= 2 {
        output.truncate(output.len() - 2);
    }

    if output.len() < 6 {
        output.clear();
        return false;
    }
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipper_is_idle_by_default() {
        let c = SkeletonClipping::new();
        assert!(!c.is_clipping());
        assert!(c.clipped_vertices().is_empty());
    }

    #[test]
    fn make_clockwise_invariant_round_trip() {
        // Apply make_clockwise twice — whatever the first pass did,
        // the second pass must be a no-op (the polygon is already
        // oriented as spine expects).
        let mut poly = vec![0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0];
        make_clockwise(&mut poly);
        let after = poly.clone();
        make_clockwise(&mut poly);
        assert_eq!(poly, after, "make_clockwise must be idempotent");
    }

    // The clip() algorithm and orientation-specific behaviour of
    // make_clockwise are validated by the render_smoke + render goldens
    // (Phase 6g). Unit-testing them in isolation requires mirroring
    // spine-cpp's y-down CW convention precisely — easier to diff
    // against captured output than to re-derive the invariant here.
}
