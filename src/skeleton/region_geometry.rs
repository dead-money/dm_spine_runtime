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

//! Maps points given in a region's own 0..1 space onto a slot's posed
//! region or mesh, for effects that follow a drawn image (weapon trails).

use crate::data::attachment::quad_corner::{BLX, BLY, BRX, BRY, ULX, ULY, URX, URY};
use crate::data::{Attachment, SlotId};
use crate::skeleton::Skeleton;

/// Region-space corners in the order the quad's world vertices are
/// written: BR, BL, UL, UR.
const QUAD_REGION_UVS: [f32; 8] = [1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0];
const QUAD_TRIANGLES: [u16; 6] = [0, 1, 2, 2, 3, 0];

/// A slot's posed geometry in world space with each vertex's region-space
/// coordinate. Reuse one across queries; [`Self::update`] doesn't allocate
/// once its buffers have grown.
#[derive(Debug, Default, Clone)]
pub struct RegionGeometry {
    world: Vec<f32>,
    region_uvs: Vec<f32>,
    triangles: Vec<u16>,
}

impl RegionGeometry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Captures the slot's applied region or mesh, with any deform, using
    /// bone world transforms from the last
    /// [`Skeleton::update_world_transform`]. Returns
    /// `false`, leaving nothing to map, if the slot shows no region or mesh.
    pub fn update(&mut self, skeleton: &Skeleton, slot: SlotId) -> bool {
        self.world.clear();
        self.region_uvs.clear();
        self.triangles.clear();
        let s = &skeleton.slots[slot.index()];
        let pose = s.applied();
        let Some(attachment) = pose.attachment else {
            return false;
        };
        match skeleton.attachment(attachment) {
            Attachment::Region(region) => {
                let off = region
                    .sequence
                    .offsets(region.sequence.resolve_index(pose.sequence_index));
                let b = skeleton.bones[s.bone.index()].applied();
                for (x, y) in [(BRX, BRY), (BLX, BLY), (ULX, ULY), (URX, URY)] {
                    self.world.push(off[x] * b.a + off[y] * b.b + b.world_x);
                    self.world.push(off[x] * b.c + off[y] * b.d + b.world_y);
                }
                self.region_uvs.extend_from_slice(&QUAD_REGION_UVS);
                self.triangles.extend_from_slice(&QUAD_TRIANGLES);
            }
            Attachment::Mesh(mesh) => {
                let n = mesh.vertex_data.world_vertices_length as usize;
                if mesh.region_uvs.len() < n || mesh.triangles.len() < 3 {
                    return false;
                }
                self.world.resize(n, 0.0);
                skeleton.compute_world_vertices(
                    &mesh.vertex_data,
                    slot,
                    0,
                    n,
                    &mut self.world,
                    0,
                    2,
                );
                self.region_uvs.extend_from_slice(&mesh.region_uvs[..n]);
                self.triangles.extend_from_slice(&mesh.triangles);
            }
            _ => return false,
        }
        true
    }

    /// World `x, y` per vertex from the last [`Self::update`].
    #[must_use]
    pub fn world_vertices(&self) -> &[f32] {
        &self.world
    }

    /// Region-space `u, v` per vertex, parallel to [`Self::world_vertices`].
    #[must_use]
    pub fn region_uvs(&self) -> &[f32] {
        &self.region_uvs
    }

    /// World position of region-space point `(u, v)`, interpolated in the
    /// triangle containing it, or in the triangle it lies nearest outside.
    /// `None` if there is no geometry or every triangle is degenerate in
    /// region space.
    #[must_use]
    pub fn map(&self, u: f32, v: f32) -> Option<(f32, f32)> {
        let (uvs, world) = (&self.region_uvs, &self.world);
        let mut best = None;
        let mut best_min = f32::MIN;
        for t in self.triangles.as_chunks::<3>().0 {
            let (i0, i1, i2) = (usize::from(t[0]), usize::from(t[1]), usize::from(t[2]));
            let (u0, v0) = (uvs[i0 * 2], uvs[i0 * 2 + 1]);
            let (u1, v1) = (uvs[i1 * 2], uvs[i1 * 2 + 1]);
            let (u2, v2) = (uvs[i2 * 2], uvs[i2 * 2 + 1]);
            let det = (u1 - u0) * (v2 - v0) - (u2 - u0) * (v1 - v0);
            if det.abs() < 1e-12 {
                continue;
            }
            let l1 = ((u - u0) * (v2 - v0) - (u2 - u0) * (v - v0)) / det;
            let l2 = ((u1 - u0) * (v - v0) - (u - u0) * (v1 - v0)) / det;
            let l0 = 1.0 - l1 - l2;
            let m = l0.min(l1).min(l2);
            if m > best_min {
                best_min = m;
                best = Some((
                    l0 * world[i0 * 2] + l1 * world[i1 * 2] + l2 * world[i2 * 2],
                    l0 * world[i0 * 2 + 1] + l1 * world[i1 * 2 + 1] + l2 * world[i2 * 2 + 1],
                ));
            }
            if m >= 0.0 {
                break;
            }
        }
        best
    }
}
