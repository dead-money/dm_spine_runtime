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

//! Ear-clipping triangulation plus convex-polygon decomposition, ported from
//! `spine-cpp/src/spine/Triangulator.cpp`.
//!
//! Input polygons are interleaved `[x0, y0, x1, y1, …]` flat `f32` slices. The
//! output triangle indices are vertex indices into that polygon (0-based,
//! unshifted — callers compute `idx * 2` to look up xy pairs). The struct holds
//! its scratch buffers so repeated calls can reuse allocations.
//!
//! # Winding convention
//!
//! Spine's `positiveArea` returns `true` when a triangle is **clockwise in
//! standard math coordinates** (y-up), equivalent to **counter-clockwise in
//! y-down screen coordinates**. Skeletons exported from the Spine editor are
//! y-up and polygons are authored CCW in that frame, which becomes CW when
//! rendered with the y-down convention Spine uses internally.
//!
//! Practically, callers should feed polygons in the same winding that Spine
//! itself uses — i.e. the native order of vertices read from a
//! `ClippingAttachment` or mesh. Reversing the winding will cause the
//! triangulator to misclassify convex and concave vertices.

// The ported algorithms use short math-style names (`p1x`, `p2y`, `x3`, `y3`,
// `winding0`) that match the spine-cpp source line-for-line. Breaking them up
// or renaming them would make side-by-side code review with the C++ much
// harder for a fidelity win of zero, so the relevant pedantic lints are
// silenced for this module.
#![allow(
    clippy::many_single_char_names,
    clippy::needless_range_loop,
    clippy::too_many_lines
)]

/// Stateful polygon triangulator with buffer reuse.
///
/// Call [`Self::triangulate`] to reduce a simple polygon to triangles, then
/// optionally [`Self::decompose`] to merge those triangles back into the
/// smallest set of convex polygons — used by `SkeletonClipping` to minimise
/// fragmentation when clipping against skeleton polygons.
#[derive(Default)]
pub struct Triangulator {
    // Scratch state, reused across calls to avoid reallocation.
    triangles: Vec<u16>,
    indices: Vec<u16>,
    is_concave: Vec<bool>,
    convex_polygons: Vec<Vec<f32>>,
    convex_polygons_indices: Vec<Vec<u16>>,
}

impl Triangulator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Triangulate a simple (non-self-intersecting) polygon by ear clipping.
    ///
    /// `vertices` is an interleaved flat array of length `2 * n` where `n` is
    /// the vertex count. The returned slice contains `3 * (n - 2)` triangle
    /// vertex indices in the range `[0, n)`.
    ///
    /// See the [module docs](self) for the expected winding convention.
    ///
    /// # Panics
    /// Panics if `vertices.len()` is odd or if the vertex count exceeds
    /// `u16::MAX`.
    pub fn triangulate(&mut self, vertices: &[f32]) -> &[u16] {
        assert!(
            vertices.len().is_multiple_of(2),
            "vertices must have even length"
        );
        let mut vertex_count = vertices.len() / 2;
        assert!(
            u16::try_from(vertex_count).is_ok(),
            "vertex count exceeds u16::MAX"
        );

        let indices = &mut self.indices;
        indices.clear();
        indices.reserve(vertex_count);
        for i in 0..vertex_count {
            indices.push(i as u16);
        }

        let is_concave = &mut self.is_concave;
        is_concave.clear();
        is_concave.resize(vertex_count, false);
        for i in 0..vertex_count {
            is_concave[i] = is_concave_at(i, vertex_count, vertices, indices);
        }

        let triangles = &mut self.triangles;
        triangles.clear();
        triangles.reserve(vertex_count.saturating_sub(2) * 3);

        while vertex_count > 3 {
            // Find an ear tip.
            let mut previous: usize = vertex_count - 1;
            let mut i: usize = 0;
            let mut next: usize = 1;

            'outer: loop {
                if !is_concave[i] {
                    // Candidate ear at `i`. Check that no concave vertex
                    // lies inside triangle (previous, i, next).
                    let p1 = indices[previous] as usize * 2;
                    let p2 = indices[i] as usize * 2;
                    let p3 = indices[next] as usize * 2;
                    let p1x = vertices[p1];
                    let p1y = vertices[p1 + 1];
                    let p2x = vertices[p2];
                    let p2y = vertices[p2 + 1];
                    let p3x = vertices[p3];
                    let p3y = vertices[p3 + 1];

                    let mut ear_blocked = false;
                    let mut ii = (next + 1) % vertex_count;
                    while ii != previous {
                        if is_concave[ii] {
                            let v = indices[ii] as usize * 2;
                            let vx = vertices[v];
                            let vy = vertices[v + 1];
                            if positive_area(p3x, p3y, p1x, p1y, vx, vy)
                                && positive_area(p1x, p1y, p2x, p2y, vx, vy)
                                && positive_area(p2x, p2y, p3x, p3y, vx, vy)
                            {
                                ear_blocked = true;
                                break;
                            }
                        }
                        ii = (ii + 1) % vertex_count;
                    }

                    if !ear_blocked {
                        break 'outer;
                    }
                }

                // Either vertex was concave or candidate ear was blocked.
                // Advance. If we've wrapped back to the start, walk `i` down
                // to find a non-concave vertex and bail out — this matches
                // the spine-cpp fallback path for degenerate geometry.
                if next == 0 {
                    loop {
                        if !is_concave[i] {
                            break;
                        }
                        if i == 0 {
                            break;
                        }
                        i -= 1;
                    }
                    break 'outer;
                }

                previous = i;
                i = next;
                next = (next + 1) % vertex_count;
            }

            // Cut ear tip at `i`.
            let prev_idx = indices[(vertex_count + i - 1) % vertex_count];
            let i_idx = indices[i];
            let next_idx = indices[(i + 1) % vertex_count];
            triangles.push(prev_idx);
            triangles.push(i_idx);
            triangles.push(next_idx);
            indices.remove(i);
            is_concave.remove(i);
            vertex_count -= 1;

            // Neighbours of the removed vertex may have flipped convexity.
            let previous_index = (vertex_count + i - 1) % vertex_count;
            let next_index = if i == vertex_count { 0 } else { i };
            is_concave[previous_index] =
                is_concave_at(previous_index, vertex_count, vertices, indices);
            is_concave[next_index] = is_concave_at(next_index, vertex_count, vertices, indices);
        }

        if vertex_count == 3 {
            // Note: spine-cpp emits the final triangle as (indices[2], indices[0], indices[1]).
            triangles.push(indices[2]);
            triangles.push(indices[0]);
            triangles.push(indices[1]);
        }

        triangles
    }

    /// Merge adjacent triangles sharing an edge and winding into convex
    /// polygons. Returns a slice of convex polygons, each in flat `[x, y, …]`
    /// form. Use [`Self::convex_polygon_indices`] for the corresponding vertex
    /// indices.
    ///
    /// `vertices` is the same flat `[x, y, …]` array passed to `triangulate`;
    /// `triangles` is the output of `triangulate` (or any compatible triangle
    /// list with indices into `vertices`).
    pub fn decompose(&mut self, vertices: &[f32], triangles: &[u16]) -> &[Vec<f32>] {
        self.decompose_both(vertices, triangles);
        &self.convex_polygons
    }

    /// Vertex indices corresponding to the polygons returned by the most recent
    /// call to [`Self::decompose`]. Each inner `Vec<u16>` has the same length
    /// as the corresponding polygon's vertex count (half the flat f32 length).
    ///
    /// **Note:** Unlike the spine-cpp field of the same name, these are plain
    /// vertex indices — not left-shifted by one. Callers that need the
    /// byte-pair offset should multiply by 2.
    #[must_use]
    pub fn convex_polygon_indices(&self) -> &[Vec<u16>] {
        &self.convex_polygons_indices
    }

    fn decompose_both(&mut self, vertices: &[f32], triangles: &[u16]) {
        self.convex_polygons.clear();
        self.convex_polygons_indices.clear();

        let mut polygon: Vec<f32> = Vec::new();
        let mut polygon_indices: Vec<u16> = Vec::new();

        // `fan_base_index` stores the base vertex index of the fan currently
        // being built. `None` means no fan yet — equivalent to the `-1`
        // sentinel in spine-cpp.
        let mut fan_base_index: Option<u16> = None;
        let mut last_winding_sign: i32 = 0;

        let mut i = 0;
        while i + 2 < triangles.len() {
            let t1_vi = triangles[i];
            let t2_vi = triangles[i + 1];
            let t3_vi = triangles[i + 2];
            let t1 = t1_vi as usize * 2;
            let t2 = t2_vi as usize * 2;
            let t3 = t3_vi as usize * 2;

            let x1 = vertices[t1];
            let y1 = vertices[t1 + 1];
            let x2 = vertices[t2];
            let y2 = vertices[t2 + 1];
            let x3 = vertices[t3];
            let y3 = vertices[t3 + 1];

            // If this triangle shares its base (first vertex) with the current
            // fan, test whether extending the fan keeps it convex.
            let mut merged = false;
            if fan_base_index == Some(t1_vi) && polygon.len() >= 4 {
                let o = polygon.len() - 4;
                let w1 = winding_sign(
                    polygon[o],
                    polygon[o + 1],
                    polygon[o + 2],
                    polygon[o + 3],
                    x3,
                    y3,
                );
                let w2 = winding_sign(x3, y3, polygon[0], polygon[1], polygon[2], polygon[3]);
                if w1 == last_winding_sign && w2 == last_winding_sign {
                    polygon.push(x3);
                    polygon.push(y3);
                    polygon_indices.push(t3_vi);
                    merged = true;
                }
            }

            if !merged {
                // Flush the current fan and start a new one with this triangle.
                if !polygon.is_empty() {
                    self.convex_polygons.push(std::mem::take(&mut polygon));
                    self.convex_polygons_indices
                        .push(std::mem::take(&mut polygon_indices));
                }
                polygon.extend_from_slice(&[x1, y1, x2, y2, x3, y3]);
                polygon_indices.extend_from_slice(&[t1_vi, t2_vi, t3_vi]);
                last_winding_sign = winding_sign(x1, y1, x2, y2, x3, y3);
                fan_base_index = Some(t1_vi);
            }

            i += 3;
        }

        if !polygon.is_empty() {
            self.convex_polygons.push(polygon);
            self.convex_polygons_indices.push(polygon_indices);
        }

        // Second pass: try to merge any remaining stand-alone triangles with
        // existing fans.
        let mut i = 0;
        while i < self.convex_polygons.len() {
            if self.convex_polygons_indices[i].is_empty() {
                i += 1;
                continue;
            }

            let first_index = self.convex_polygons_indices[i][0];
            let last_index = *self.convex_polygons_indices[i].last().unwrap();

            let (
                mut prev_prev_x,
                mut prev_prev_y,
                mut prev_x,
                mut prev_y,
                first_x,
                first_y,
                second_x,
                second_y,
            );
            {
                let p = &self.convex_polygons[i];
                let o = p.len() - 4;
                prev_prev_x = p[o];
                prev_prev_y = p[o + 1];
                prev_x = p[o + 2];
                prev_y = p[o + 3];
                first_x = p[0];
                first_y = p[1];
                second_x = p[2];
                second_y = p[3];
            }
            let winding0 = winding_sign(prev_prev_x, prev_prev_y, prev_x, prev_y, first_x, first_y);

            let mut ii = 0;
            while ii < self.convex_polygons.len() {
                if ii == i {
                    ii += 1;
                    continue;
                }
                if self.convex_polygons_indices[ii].len() != 3 {
                    ii += 1;
                    continue;
                }

                let other_first = self.convex_polygons_indices[ii][0];
                let other_second = self.convex_polygons_indices[ii][1];
                let other_last = self.convex_polygons_indices[ii][2];

                if other_first != first_index || other_second != last_index {
                    ii += 1;
                    continue;
                }

                let (x3, y3) = {
                    let op = &self.convex_polygons[ii];
                    (op[op.len() - 2], op[op.len() - 1])
                };

                let w1 = winding_sign(prev_prev_x, prev_prev_y, prev_x, prev_y, x3, y3);
                let w2 = winding_sign(x3, y3, first_x, first_y, second_x, second_y);

                if w1 == winding0 && w2 == winding0 {
                    self.convex_polygons[ii].clear();
                    self.convex_polygons_indices[ii].clear();
                    self.convex_polygons[i].push(x3);
                    self.convex_polygons[i].push(y3);
                    self.convex_polygons_indices[i].push(other_last);
                    prev_prev_x = prev_x;
                    prev_prev_y = prev_y;
                    prev_x = x3;
                    prev_y = y3;
                    ii = 0;
                    continue;
                }

                ii += 1;
            }

            i += 1;
        }

        // Remove any polygons cleared by merging above.
        let mut i = self.convex_polygons.len();
        while i > 0 {
            i -= 1;
            if self.convex_polygons[i].is_empty() {
                self.convex_polygons.remove(i);
                self.convex_polygons_indices.remove(i);
            }
        }
    }
}

/// True iff the vertex at `index` in the current `indices` ring is concave —
/// i.e. the signed area of (prev, current, next) is negative.
fn is_concave_at(index: usize, vertex_count: usize, vertices: &[f32], indices: &[u16]) -> bool {
    let previous = indices[(vertex_count + index - 1) % vertex_count] as usize * 2;
    let current = indices[index] as usize * 2;
    let next = indices[(index + 1) % vertex_count] as usize * 2;

    !positive_area(
        vertices[previous],
        vertices[previous + 1],
        vertices[current],
        vertices[current + 1],
        vertices[next],
        vertices[next + 1],
    )
}

/// `2 * signed_area(p1, p2, p3) >= 0` — true when (p1, p2, p3) is CCW or
/// colinear.
fn positive_area(p1x: f32, p1y: f32, p2x: f32, p2y: f32, p3x: f32, p3y: f32) -> bool {
    p1x * (p3y - p2y) + p2x * (p1y - p3y) + p3x * (p2y - p1y) >= 0.0
}

/// Sign (+1 / -1) of the cross product of p1->p2 and p1->p3 — gives the
/// winding direction of triangle (p1, p2, p3).
fn winding_sign(p1x: f32, p1y: f32, p2x: f32, p2y: f32, p3x: f32, p3y: f32) -> i32 {
    let px = p2x - p1x;
    let py = p2y - p1y;
    if p3x * py - p3y * px + px * p1y - p1x * py >= 0.0 {
        1
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Shoelace formula — returns signed area of polygon from interleaved verts.
    fn signed_area(vertices: &[f32]) -> f32 {
        let n = vertices.len() / 2;
        let mut area = 0.0;
        for i in 0..n {
            let j = (i + 1) % n;
            area += vertices[i * 2] * vertices[j * 2 + 1];
            area -= vertices[j * 2] * vertices[i * 2 + 1];
        }
        area * 0.5
    }

    fn triangle_area(v: &[f32], a: u16, b: u16, c: u16) -> f32 {
        let ax = v[a as usize * 2];
        let ay = v[a as usize * 2 + 1];
        let bx = v[b as usize * 2];
        let by = v[b as usize * 2 + 1];
        let cx = v[c as usize * 2];
        let cy = v[c as usize * 2 + 1];
        ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)) * 0.5
    }

    #[test]
    fn positive_area_matches_spine_convention() {
        // A triangle that is CCW in math coords (positive math-signed-area) must
        // yield `positive_area == false` under Spine's inverted convention.
        assert!(!positive_area(0.0, 0.0, 1.0, 0.0, 0.0, 1.0));
        // The reverse (CW in math) yields true.
        assert!(positive_area(0.0, 0.0, 0.0, 1.0, 1.0, 0.0));
    }

    #[test]
    fn triangle_is_untouched() {
        let mut t = Triangulator::new();
        // n=3 passes through unchanged regardless of winding.
        let verts = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let tris = t.triangulate(&verts).to_vec();
        assert_eq!(tris.len(), 3);
        // Spine's final-triangle branch emits (2, 0, 1).
        assert_eq!(tris, vec![2, 0, 1]);
    }

    #[test]
    fn square_produces_two_triangles() {
        let mut t = Triangulator::new();
        // Math-CW square (= y-down-CCW): (0,0) (0,1) (1,1) (1,0).
        let verts = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let expected_area = signed_area(&verts);
        assert!(expected_area < 0.0, "fixture must be math-CW");

        let tris = t.triangulate(&verts).to_vec();
        assert_eq!(tris.len(), 6);

        let mut total = 0.0f32;
        for chunk in tris.chunks_exact(3) {
            let (a, b, c) = (chunk[0], chunk[1], chunk[2]);
            assert!(a < 4 && b < 4 && c < 4);
            assert!(a != b && b != c && a != c);
            let area = triangle_area(&verts, a, b, c);
            // Each output triangle inherits the polygon's math-CW winding.
            assert!(
                area < 0.0,
                "triangle winding flipped: {chunk:?} area {area}"
            );
            total += area;
        }
        assert_abs_diff_eq!(total, expected_area, epsilon = 1e-6);
    }

    #[test]
    fn concave_l_shape() {
        let mut t = Triangulator::new();
        // L-shape in the winding Spine expects (math-CW, y-down-CCW):
        //
        //   (0,2)---(1,2)
        //     |       |
        //     |     (1,1)---(2,1)
        //     |               |
        //     +---------------+
        //   (0,0)           (2,0)
        //
        // Traversal: (0,0) -> (0,2) -> (1,2) -> (1,1) -> (2,1) -> (2,0).
        // Concave vertex is index 3 = (1,1).
        let verts = [0.0, 0.0, 0.0, 2.0, 1.0, 2.0, 1.0, 1.0, 2.0, 1.0, 2.0, 0.0];
        let expected_area = signed_area(&verts);
        assert!(expected_area < 0.0, "fixture must be math-CW");

        let tris = t.triangulate(&verts).to_vec();
        // 6-vertex polygon -> 4 triangles -> 12 indices.
        assert_eq!(tris.len(), 12);

        let mut total = 0.0f32;
        for chunk in tris.chunks_exact(3) {
            let area = triangle_area(&verts, chunk[0], chunk[1], chunk[2]);
            assert!(
                area < 0.0,
                "triangle winding flipped: {chunk:?} area {area}"
            );
            total += area;
        }
        assert_abs_diff_eq!(total, expected_area, epsilon = 1e-5);
    }

    #[test]
    fn decompose_square_is_single_quad() {
        let mut t = Triangulator::new();
        let verts = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let tris = t.triangulate(&verts).to_vec();
        let polygons = t.decompose(&verts, &tris);
        // The two triangles form a single convex quad.
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].len(), 8);
    }

    #[test]
    fn decompose_concave_produces_multiple_convex_parts() {
        let mut t = Triangulator::new();
        let verts = [0.0, 0.0, 0.0, 2.0, 1.0, 2.0, 1.0, 1.0, 2.0, 1.0, 2.0, 0.0];
        let tris = t.triangulate(&verts).to_vec();
        let polygons = t.decompose(&verts, &tris).to_vec();
        // The L-shape cannot be expressed as a single convex polygon.
        assert!(
            polygons.len() >= 2,
            "expected >= 2 convex parts, got {}",
            polygons.len()
        );
        for p in &polygons {
            assert!(p.len() % 2 == 0);
            assert!(p.len() >= 6);
        }
        assert_eq!(t.convex_polygon_indices().len(), polygons.len());
    }

    #[test]
    fn reuses_buffers_across_calls() {
        let mut t = Triangulator::new();
        let verts1 = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let _ = t.triangulate(&verts1);
        let verts2 = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let tris2 = t.triangulate(&verts2).to_vec();
        assert_eq!(tris2.len(), 6);
    }
}
