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

//! Shared `VertexAttachment::computeWorldVertices` helper — transforms
//! an attachment's stored local vertices into world space using the
//! slot's bone (unweighted) or a weighted sum over multiple bones
//! (weighted), optionally adding the slot's per-vertex deform offsets.
//!
//! Used by mesh rendering, path constraints, bounding-box hit tests, and
//! clipping-polygon construction. Literal port of
//! `spine-cpp/src/spine/VertexAttachment.cpp::computeWorldVertices`.

use crate::data::{SlotId, VertexData};
use crate::skeleton::Skeleton;

impl Skeleton {
    /// Transform `vertex_data`'s local vertices into world space, writing
    /// interleaved `x, y` pairs into `world[offset..]` at `stride`
    /// intervals. `start` and `count` are indexed in **local floats**
    /// (consistent with spine-cpp — they're `2 * vertex_index` and
    /// `2 * vertex_count` respectively).
    ///
    /// If the slot has a non-empty deform buffer, its values are added
    /// on top of the local vertices before world transformation.
    // spine-cpp signature is the same 7-arg shape (slot + start/count/buf/offset/stride);
    // matching it is non-negotiable for 1:1 port parity.
    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    pub(crate) fn compute_world_vertices(
        &self,
        vertex_data: &VertexData,
        slot_id: SlotId,
        start: usize,
        count: usize,
        world: &mut [f32],
        offset: usize,
        stride: usize,
    ) {
        let end = offset + (count >> 1) * stride;
        let bones = &vertex_data.bones;
        let vertices = &vertex_data.vertices;
        let deform = &self.slots[slot_id.index()].deform;

        if bones.is_empty() {
            let verts: &[f32] = if deform.is_empty() { vertices } else { deform };
            let slot_bone = self.data.slots[slot_id.index()].bone;
            let b = &self.bones[slot_bone.index()];
            let (x, y, a, bb, c, d) = (b.world_x, b.world_y, b.a, b.b, b.c, b.d);
            let mut vv = start;
            let mut w = offset;
            while w < end {
                let vx = verts[vv];
                let vy = verts[vv + 1];
                world[w] = vx * a + vy * bb + x;
                world[w + 1] = vx * c + vy * d + y;
                vv += 2;
                w += stride;
            }
            return;
        }

        // Weighted — skip past the first `start / 2` weighted groups to
        // find the initial `v` and `b` cursors.
        let mut v = 0_usize;
        let mut skip = 0_usize;
        let mut i = 0_usize;
        while i < start {
            let n = bones[v] as usize;
            v += n + 1;
            skip += n;
            i += 2;
        }

        if deform.is_empty() {
            let mut w = offset;
            let mut b_idx = skip * 3;
            while w < end {
                let mut wx = 0.0_f32;
                let mut wy = 0.0_f32;
                let n_here = bones[v] as usize;
                v += 1;
                let n_end = v + n_here;
                while v < n_end {
                    let bone_index = bones[v] as usize;
                    let bone = &self.bones[bone_index];
                    let vx = vertices[b_idx];
                    let vy = vertices[b_idx + 1];
                    let weight = vertices[b_idx + 2];
                    wx += (vx * bone.a + vy * bone.b + bone.world_x) * weight;
                    wy += (vx * bone.c + vy * bone.d + bone.world_y) * weight;
                    v += 1;
                    b_idx += 3;
                }
                world[w] = wx;
                world[w + 1] = wy;
                w += stride;
            }
        } else {
            let mut w = offset;
            let mut b_idx = skip * 3;
            let mut f = skip << 1;
            while w < end {
                let mut wx = 0.0_f32;
                let mut wy = 0.0_f32;
                let n_here = bones[v] as usize;
                v += 1;
                let n_end = v + n_here;
                while v < n_end {
                    let bone_index = bones[v] as usize;
                    let bone = &self.bones[bone_index];
                    let vx = vertices[b_idx] + deform[f];
                    let vy = vertices[b_idx + 1] + deform[f + 1];
                    let weight = vertices[b_idx + 2];
                    wx += (vx * bone.a + vy * bone.b + bone.world_x) * weight;
                    wy += (vx * bone.c + vy * bone.d + bone.world_y) * weight;
                    v += 1;
                    b_idx += 3;
                    f += 2;
                }
                world[w] = wx;
                world[w + 1] = wy;
                w += stride;
            }
        }
    }
}
