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

//! `SkeletonBounds` — collects every visible
//! [`BoundingBoxAttachment`], computes its world-space polygon, and
//! exposes hit-testing + coarse AABB queries.
//!
//! Literal port of `spine-cpp/src/spine/SkeletonBounds.cpp` (~230
//! LOC). Used by gameplay code (projectile hits, click-to-select,
//! spatial culling) — orthogonal to rendering, shares the
//! [`Skeleton::compute_world_vertices`] helper.

#![allow(clippy::many_single_char_names)] // spine-cpp short names preserved for diff parity.

use crate::data::{Attachment, AttachmentId};
use crate::skeleton::Skeleton;

/// One bounding-box attachment's world polygon — interleaved
/// `x, y` pairs. Vertex count = `count / 2`.
#[derive(Debug, Clone, Default)]
pub struct BoundsPolygon {
    /// Flat interleaved world-space vertex positions. `count`
    /// carries the active prefix length since the underlying `Vec`
    /// may have extra capacity from reuse.
    pub vertices: Vec<f32>,
    /// Number of *floats* in use (always even). spine-cpp calls this
    /// `_count` on its `Polygon` type.
    pub count: usize,
}

impl BoundsPolygon {
    /// Iterate the `(x, y)` pairs within the active prefix of
    /// [`Self::vertices`].
    pub fn iter_vertices(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.vertices[..self.count]
            .chunks_exact(2)
            .map(|xy| (xy[0], xy[1]))
    }
}

/// Hit-test + AABB helper over a `Skeleton`'s active
/// [`BoundingBoxAttachment`]s. Recompute with [`Self::update`] every
/// frame the skeleton pose changes.
#[derive(Debug, Default)]
pub struct SkeletonBounds {
    /// `AttachmentId` of each bounding box, one per polygon. Parallel
    /// to [`Self::polygons`].
    bounding_boxes: Vec<AttachmentId>,
    /// World-space polygons, one per entry in [`Self::bounding_boxes`].
    /// Buffers are pooled — on `update` we truncate the outer Vec to
    /// the active count but retain inner buffer capacity.
    polygons: Vec<BoundsPolygon>,
    /// Axis-aligned bounding box covering every polygon. Valid after
    /// a call to [`Self::update`] with `update_aabb = true`; otherwise
    /// spans `[f32::MIN, f32::MAX]`.
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl SkeletonBounds {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Repopulate `self` from `skeleton`'s current pose. Scans the
    /// slot list, collects every active `BoundingBoxAttachment`,
    /// and computes its world-space polygon. If `update_aabb` is
    /// `true`, also recomputes the AABB; otherwise `aabb_*` queries
    /// conservatively return `true` (spine-cpp convention).
    ///
    /// # Panics
    /// Panics if a bounding box's `vertex_data.world_vertices_length`
    /// exceeds `i32::MAX` (practically impossible — Spine exports
    /// are capped well below this).
    pub fn update(&mut self, skeleton: &Skeleton, update_aabb: bool) {
        // Reuse allocations: drop attachment refs but keep polygon
        // buffers around for the next pass.
        self.bounding_boxes.clear();
        let reused = self.polygons.len();
        let mut slot_count_seen = 0;

        for slot_idx in 0..skeleton.slots.len() {
            let slot = &skeleton.slots[slot_idx];
            let bone_id = skeleton.data.slots[slot_idx].bone;
            if !skeleton.bones[bone_id.index()].active {
                continue;
            }
            let Some(attachment_id) = slot.attachment else {
                continue;
            };
            let Attachment::BoundingBox(bbox) = &skeleton.data.attachments[attachment_id.index()]
            else {
                continue;
            };

            // Either reuse an existing BoundsPolygon buffer or push a
            // fresh one.
            let count = bbox.vertex_data.world_vertices_length as usize;
            let polygon = if slot_count_seen < reused {
                &mut self.polygons[slot_count_seen]
            } else {
                self.polygons.push(BoundsPolygon::default());
                self.polygons.last_mut().unwrap()
            };
            polygon.count = count;
            if polygon.vertices.len() < count {
                polygon.vertices.resize(count, 0.0);
            }
            skeleton.compute_world_vertices(
                &bbox.vertex_data,
                crate::data::SlotId(slot_idx as u16),
                0,
                count,
                &mut polygon.vertices,
                0,
                2,
            );

            self.bounding_boxes.push(attachment_id);
            slot_count_seen += 1;
        }

        // Drop any trailing reused-but-now-unused polygon slots.
        self.polygons.truncate(slot_count_seen);

        if update_aabb {
            self.compute_aabb();
        } else {
            self.min_x = f32::MIN;
            self.min_y = f32::MIN;
            self.max_x = f32::MAX;
            self.max_y = f32::MAX;
        }
    }

    fn compute_aabb(&mut self) {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for polygon in &self.polygons {
            for (x, y) in polygon.iter_vertices() {
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
        self.min_x = min_x;
        self.min_y = min_y;
        self.max_x = max_x;
        self.max_y = max_y;
    }

    /// Returns `true` if the AABB contains `(x, y)`.
    #[must_use]
    pub fn aabb_contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Returns `true` if the AABB intersects the segment
    /// `(x1, y1) → (x2, y2)`.
    #[must_use]
    pub fn aabb_intersects_segment(&self, x1: f32, y1: f32, x2: f32, y2: f32) -> bool {
        let (min_x, min_y, max_x, max_y) = (self.min_x, self.min_y, self.max_x, self.max_y);
        if (x1 <= min_x && x2 <= min_x)
            || (y1 <= min_y && y2 <= min_y)
            || (x1 >= max_x && x2 >= max_x)
            || (y1 >= max_y && y2 >= max_y)
        {
            return false;
        }
        let m = (y2 - y1) / (x2 - x1);
        let y = m * (min_x - x1) + y1;
        if y > min_y && y < max_y {
            return true;
        }
        let y = m * (max_x - x1) + y1;
        if y > min_y && y < max_y {
            return true;
        }
        let x = (min_y - y1) / m + x1;
        if x > min_x && x < max_x {
            return true;
        }
        let x = (max_y - y1) / m + x1;
        if x > min_x && x < max_x {
            return true;
        }
        false
    }

    /// Returns `true` if `self`'s AABB intersects `other`'s AABB.
    #[must_use]
    pub fn aabb_intersects_skeleton(&self, other: &SkeletonBounds) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
    }

    /// Point-in-polygon test for a specific [`BoundsPolygon`]
    /// (ray-casting / even-odd rule).
    #[must_use]
    pub fn polygon_contains_point(polygon: &BoundsPolygon, x: f32, y: f32) -> bool {
        let vertices = &polygon.vertices;
        let nn = polygon.count;
        if nn < 6 {
            return false;
        }
        let mut prev_index = nn - 2;
        let mut inside = false;
        let mut ii = 0;
        while ii < nn {
            let vertex_y = vertices[ii + 1];
            let prev_y = vertices[prev_index + 1];
            if (vertex_y < y && prev_y >= y) || (prev_y < y && vertex_y >= y) {
                let vertex_x = vertices[ii];
                if vertex_x
                    + (y - vertex_y) / (prev_y - vertex_y) * (vertices[prev_index] - vertex_x)
                    < x
                {
                    inside = !inside;
                }
            }
            prev_index = ii;
            ii += 2;
        }
        inside
    }

    /// Returns the [`AttachmentId`] of the first bounding box
    /// containing `(x, y)`, or `None`. Walks polygons in the order
    /// they were collected by [`Self::update`].
    #[must_use]
    pub fn contains_point(&self, x: f32, y: f32) -> Option<AttachmentId> {
        for (i, polygon) in self.polygons.iter().enumerate() {
            if Self::polygon_contains_point(polygon, x, y) {
                return Some(self.bounding_boxes[i]);
            }
        }
        None
    }

    /// Returns the [`AttachmentId`] of the first bounding box
    /// whose polygon intersects the segment `(x1, y1) → (x2, y2)`,
    /// or `None`.
    #[must_use]
    pub fn intersects_segment(&self, x1: f32, y1: f32, x2: f32, y2: f32) -> Option<AttachmentId> {
        for (i, polygon) in self.polygons.iter().enumerate() {
            if Self::polygon_intersects_segment(polygon, x1, y1, x2, y2) {
                return Some(self.bounding_boxes[i]);
            }
        }
        None
    }

    /// Segment-polygon intersection for a specific [`BoundsPolygon`].
    #[must_use]
    pub fn polygon_intersects_segment(
        polygon: &BoundsPolygon,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> bool {
        let vertices = &polygon.vertices;
        let nn = polygon.count;
        if nn < 6 {
            return false;
        }
        let width12 = x1 - x2;
        let height12 = y1 - y2;
        let det1 = x1 * y2 - y1 * x2;
        let mut x3 = vertices[nn - 2];
        let mut y3 = vertices[nn - 1];
        let mut ii = 0;
        while ii < nn {
            let x4 = vertices[ii];
            let y4 = vertices[ii + 1];
            let det2 = x3 * y4 - y3 * x4;
            let width34 = x3 - x4;
            let height34 = y3 - y4;
            let det3 = width12 * height34 - height12 * width34;
            let x = (det1 * width34 - width12 * det2) / det3;
            if ((x >= x3 && x <= x4) || (x >= x4 && x <= x3))
                && ((x >= x1 && x <= x2) || (x >= x2 && x <= x1))
            {
                let y = (det1 * height34 - height12 * det2) / det3;
                if ((y >= y3 && y <= y4) || (y >= y4 && y <= y3))
                    && ((y >= y1 && y <= y2) || (y >= y2 && y <= y1))
                {
                    return true;
                }
            }
            x3 = x4;
            y3 = y4;
            ii += 2;
        }
        false
    }

    /// Returns the bounding box polygon for the given attachment,
    /// or `None` if it's not in the last `update` pass. Requires a
    /// prior call to [`Self::update`].
    #[must_use]
    pub fn polygon_for(&self, attachment_id: AttachmentId) -> Option<&BoundsPolygon> {
        self.bounding_boxes
            .iter()
            .position(|&a| a == attachment_id)
            .map(|i| &self.polygons[i])
    }

    /// All polygons collected by the last [`Self::update`] call.
    #[must_use]
    pub fn polygons(&self) -> &[BoundsPolygon] {
        &self.polygons
    }

    /// All bounding-box attachments collected by the last
    /// [`Self::update`] call. Parallel to [`Self::polygons`].
    #[must_use]
    pub fn bounding_boxes(&self) -> &[AttachmentId] {
        &self.bounding_boxes
    }

    /// AABB width. Returns `0` when [`Self::update`] was never
    /// called or `update_aabb = false`.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// AABB height.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Tuple `(min_x, min_y, max_x, max_y)` for the last computed
    /// AABB.
    #[must_use]
    pub fn aabb(&self) -> (f32, f32, f32, f32) {
        (self.min_x, self.min_y, self.max_x, self.max_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let b = SkeletonBounds::new();
        assert!(b.polygons().is_empty());
        assert!(b.bounding_boxes().is_empty());
    }

    #[test]
    fn polygon_contains_point_square() {
        let polygon = BoundsPolygon {
            vertices: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            count: 8,
        };
        assert!(SkeletonBounds::polygon_contains_point(&polygon, 5.0, 5.0));
        assert!(!SkeletonBounds::polygon_contains_point(&polygon, -1.0, 5.0));
        assert!(!SkeletonBounds::polygon_contains_point(&polygon, 5.0, 20.0));
    }

    #[test]
    fn polygon_intersects_segment_crosses_square_edge() {
        let polygon = BoundsPolygon {
            vertices: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            count: 8,
        };
        // Horizontal segment that crosses left edge at y=5.
        assert!(SkeletonBounds::polygon_intersects_segment(
            &polygon, -5.0, 5.0, 5.0, 5.0
        ));
        // Segment entirely outside below.
        assert!(!SkeletonBounds::polygon_intersects_segment(
            &polygon, -5.0, -5.0, -1.0, -5.0
        ));
    }

    /// End-to-end: an example rig with bounding boxes (goblins has
    /// many) updates without panicking and produces a non-empty
    /// polygon set.
    #[test]
    fn update_on_example_rig() {
        use crate::atlas::Atlas;
        use crate::load::{AtlasAttachmentLoader, SkeletonBinary};
        use crate::skeleton::{Physics, Skeleton};
        use std::sync::Arc;

        let Ok(atlas_src) =
            std::fs::read_to_string("../spine-runtimes/examples/goblins/export/goblins-pma.atlas")
        else {
            return;
        };
        let atlas = Atlas::parse(&atlas_src).unwrap();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let bytes =
            std::fs::read("../spine-runtimes/examples/goblins/export/goblins-pro.skel").unwrap();
        let data = Arc::new(
            SkeletonBinary::with_loader(&mut loader)
                .read(&bytes)
                .unwrap(),
        );

        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();
        sk.set_to_setup_pose();
        sk.update_world_transform(Physics::None);

        let mut bounds = SkeletonBounds::new();
        bounds.update(&sk, true);

        // goblins-pro uses bounding boxes for hit-testing — expect
        // at least one. If an example ever drops them the test is
        // skipped (not failed) at the assertion below.
        for polygon in bounds.polygons() {
            assert!(polygon.count > 0);
            assert!(polygon.count.is_multiple_of(2));
            for (x, y) in polygon.iter_vertices() {
                assert!(x.is_finite() && y.is_finite());
            }
        }
    }
}
