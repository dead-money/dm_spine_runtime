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

//! Ear-clipping triangulation and convex decomposition, used to split
//! concave clipping polygons.

#![allow(clippy::many_single_char_names)]

/// Reusable scratch for triangulating and decomposing polygons.
#[derive(Debug, Default, Clone)]
pub struct Triangulator {
    indices: Vec<i32>,
    is_concave: Vec<bool>,
    triangles: Vec<u16>,
    convex_polygons: Vec<Vec<f32>>,
    convex_polygon_indices: Vec<Vec<u16>>,
    polygon_pool: Vec<Vec<f32>>,
    indices_pool: Vec<Vec<u16>>,
}

impl Triangulator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Triangulates a simple polygon of `x, y` pairs. Returns vertex indices.
    pub fn triangulate(&mut self, vertices: &[f32]) -> &[u16] {
        let mut vertex_count = (vertices.len() >> 1) as i32;
        let indices = &mut self.indices;
        indices.clear();
        indices.extend(0..vertex_count);
        let is_concave = &mut self.is_concave;
        is_concave.clear();
        for i in 0..vertex_count {
            is_concave.push(concave(i, vertex_count, vertices, indices));
        }
        let triangles = &mut self.triangles;
        triangles.clear();
        triangles.reserve((vertex_count - 2).max(0) as usize * 3);

        while vertex_count > 3 {
            let mut previous = vertex_count - 1;
            let mut i = 0;
            let mut next = 1;
            loop {
                if !is_concave[i as usize] {
                    let p1 = (indices[previous as usize] << 1) as usize;
                    let p2 = (indices[i as usize] << 1) as usize;
                    let p3 = (indices[next as usize] << 1) as usize;
                    let (p1x, p1y) = (vertices[p1], vertices[p1 + 1]);
                    let (p2x, p2y) = (vertices[p2], vertices[p2 + 1]);
                    let (p3x, p3y) = (vertices[p3], vertices[p3 + 1]);
                    let mut ear = true;
                    let mut ii = if next + 1 < vertex_count { next + 1 } else { 0 };
                    while ii != previous {
                        if is_concave[ii as usize] {
                            let v = (indices[ii as usize] << 1) as usize;
                            let (vx, vy) = (vertices[v], vertices[v + 1]);
                            if positive_area(p3x, p3y, p1x, p1y, vx, vy)
                                && positive_area(p1x, p1y, p2x, p2y, vx, vy)
                                && positive_area(p2x, p2y, p3x, p3y, vx, vy)
                            {
                                ear = false;
                                break;
                            }
                        }
                        ii += 1;
                        if ii == vertex_count {
                            ii = 0;
                        }
                    }
                    if ear {
                        break;
                    }
                }
                if next == 0 {
                    loop {
                        if !is_concave[i as usize] {
                            break;
                        }
                        i -= 1;
                        if i <= 0 {
                            break;
                        }
                    }
                    previous = if i > 0 { i - 1 } else { vertex_count - 1 };
                    next = if i + 1 < vertex_count { i + 1 } else { 0 };
                    break;
                }
                previous = i;
                i = next;
                next += 1;
                if next == vertex_count {
                    next = 0;
                }
            }

            triangles.push(indices[previous as usize] as u16);
            triangles.push(indices[i as usize] as u16);
            triangles.push(indices[next as usize] as u16);
            indices.remove(i as usize);
            is_concave.remove(i as usize);
            vertex_count -= 1;

            let previous_index = if i > 0 { i - 1 } else { vertex_count - 1 };
            let next_index = if i < vertex_count { i } else { 0 };
            is_concave[previous_index as usize] =
                concave(previous_index, vertex_count, vertices, indices);
            is_concave[next_index as usize] = concave(next_index, vertex_count, vertices, indices);
        }
        if vertex_count == 3 {
            triangles.push(indices[2] as u16);
            triangles.push(indices[0] as u16);
            triangles.push(indices[1] as u16);
        }
        triangles
    }

    /// Merges triangles into convex polygons, each closed by repeating its
    /// first point.
    ///
    /// # Panics
    ///
    /// If `triangles` indexes past the end of `vertices`.
    pub fn decompose(&mut self, vertices: &[f32], triangles: &[u16]) -> &[Vec<f32>] {
        self.polygon_pool.append(&mut self.convex_polygons);
        self.indices_pool.append(&mut self.convex_polygon_indices);
        // Pooled buffers come back in varying order, so size each one for the
        // largest possible polygon to keep steady-state frames allocation-free.
        let (max_points, max_indices) = (vertices.len() + 2, vertices.len() / 2);
        let mut polygon = self.polygon_pool.pop().unwrap_or_default();
        polygon.clear();
        polygon.reserve(max_points);
        let mut polygon_indices = self.indices_pool.pop().unwrap_or_default();
        polygon_indices.clear();
        polygon_indices.reserve(max_indices);

        let mut fan_base_index: i32 = -1;
        let mut last_winding = 0;
        for tri in triangles.as_chunks::<3>().0 {
            let (t1, t2, t3) = (tri[0] << 1, tri[1] << 1, tri[2] << 1);
            let (x1, y1) = (vertices[t1 as usize], vertices[t1 as usize + 1]);
            let (x2, y2) = (vertices[t2 as usize], vertices[t2 as usize + 1]);
            let (x3, y3) = (vertices[t3 as usize], vertices[t3 as usize + 1]);

            if fan_base_index == i32::from(t1) {
                let o = polygon.len() - 4;
                let p = &polygon;
                if winding(p[o], p[o + 1], p[o + 2], p[o + 3], x3, y3) == last_winding
                    && winding(x3, y3, p[0], p[1], p[2], p[3]) == last_winding
                {
                    polygon.push(x3);
                    polygon.push(y3);
                    polygon_indices.push(t3);
                    continue;
                }
            }
            if !polygon.is_empty() {
                self.convex_polygons.push(polygon);
                self.convex_polygon_indices.push(polygon_indices);
                polygon = self.polygon_pool.pop().unwrap_or_default();
                polygon.clear();
                polygon.reserve(max_points);
                polygon_indices = self.indices_pool.pop().unwrap_or_default();
                polygon_indices.clear();
                polygon_indices.reserve(max_indices);
            }
            polygon.clear();
            polygon.extend_from_slice(&[x1, y1, x2, y2, x3, y3]);
            polygon_indices.clear();
            polygon_indices.extend_from_slice(&[t1, t2, t3]);
            last_winding = winding(x1, y1, x2, y2, x3, y3);
            fan_base_index = i32::from(t1);
        }
        if polygon.is_empty() {
            self.polygon_pool.push(polygon);
            self.indices_pool.push(polygon_indices);
        } else {
            self.convex_polygons.push(polygon);
            self.convex_polygon_indices.push(polygon_indices);
        }

        // Merge remaining triangles into the fans they extend.
        let n = self.convex_polygons.len();
        for i in 0..n {
            if self.convex_polygon_indices[i].is_empty() {
                continue;
            }
            let first_index = self.convex_polygon_indices[i][0];
            let mut last_index = *self.convex_polygon_indices[i].last().expect("non-empty");
            let p = &self.convex_polygons[i];
            let o = p.len() - 4;
            let (mut prev_prev_x, mut prev_prev_y) = (p[o], p[o + 1]);
            let (mut prev_x, mut prev_y) = (p[o + 2], p[o + 3]);
            let (first_x, first_y) = (p[0], p[1]);
            let (second_x, second_y) = (p[2], p[3]);
            let polygon_winding =
                winding(prev_prev_x, prev_prev_y, prev_x, prev_y, first_x, first_y);

            let mut ii = 0;
            while ii < n {
                if ii == i {
                    ii += 1;
                    continue;
                }
                let other = &self.convex_polygon_indices[ii];
                if other.len() != 3 || other[0] != first_index || other[1] != last_index {
                    ii += 1;
                    continue;
                }
                let other_last_index = other[2];
                let other_poly = &self.convex_polygons[ii];
                let (x3, y3) = (
                    other_poly[other_poly.len() - 2],
                    other_poly[other_poly.len() - 1],
                );
                if winding(prev_prev_x, prev_prev_y, prev_x, prev_y, x3, y3) == polygon_winding
                    && winding(x3, y3, first_x, first_y, second_x, second_y) == polygon_winding
                {
                    self.convex_polygons[ii].clear();
                    self.convex_polygon_indices[ii].clear();
                    self.convex_polygons[i].push(x3);
                    self.convex_polygons[i].push(y3);
                    self.convex_polygon_indices[i].push(other_last_index);
                    last_index = other_last_index;
                    prev_prev_x = prev_x;
                    prev_prev_y = prev_y;
                    prev_x = x3;
                    prev_y = y3;
                    ii = 0;
                    continue;
                }
                ii += 1;
            }
        }

        let mut i = self.convex_polygons.len();
        while i > 0 {
            i -= 1;
            if self.convex_polygons[i].is_empty() {
                self.polygon_pool.push(self.convex_polygons.remove(i));
                self.indices_pool
                    .push(self.convex_polygon_indices.remove(i));
            } else {
                let p = &mut self.convex_polygons[i];
                let (x, y) = (p[0], p[1]);
                p.push(x);
                p.push(y);
            }
        }
        &self.convex_polygons
    }

    /// [`Self::triangulate`] then [`Self::decompose`] without copying the
    /// triangles out.
    pub fn triangulate_convex(&mut self, vertices: &[f32]) -> &[Vec<f32>] {
        self.triangulate(vertices);
        let triangles = std::mem::take(&mut self.triangles);
        self.decompose(vertices, &triangles);
        self.triangles = triangles;
        &self.convex_polygons
    }

    /// Vertex offsets (index × 2) of each polygon from the last
    /// [`Self::decompose`].
    #[must_use]
    pub fn convex_polygon_indices(&self) -> &[Vec<u16>] {
        &self.convex_polygon_indices
    }
}

fn concave(index: i32, vertex_count: i32, vertices: &[f32], indices: &[i32]) -> bool {
    let previous = (indices[if index > 0 {
        index - 1
    } else {
        vertex_count - 1
    } as usize]
        << 1) as usize;
    let current = (indices[index as usize] << 1) as usize;
    let next = (indices[if index + 1 < vertex_count {
        index + 1
    } else {
        0
    } as usize]
        << 1) as usize;
    !positive_area(
        vertices[previous],
        vertices[previous + 1],
        vertices[current],
        vertices[current + 1],
        vertices[next],
        vertices[next + 1],
    )
}

#[inline]
pub(crate) fn positive_area(p1x: f32, p1y: f32, p2x: f32, p2y: f32, p3x: f32, p3y: f32) -> bool {
    p1x * (p3y - p2y) + p2x * (p1y - p3y) + p3x * (p2y - p1y) >= 0.0
}

#[inline]
fn winding(p1x: f32, p1y: f32, p2x: f32, p2y: f32, p3x: f32, p3y: f32) -> i32 {
    if p1x * (p3y - p2y) + p2x * (p1y - p3y) + p3x * (p2y - p1y) >= 0.0 {
        1
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    /// Shoelace signed area of interleaved `x, y` vertices.
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
        for chunk in tris.as_chunks::<3>().0 {
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
        for chunk in tris.as_chunks::<3>().0 {
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
        // One convex quad, closed by repeating its first point.
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].len(), 10);
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
            assert_eq!(p.len() % 2, 0);
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
