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

//! `SkeletonRenderer` — walks a skeleton's draw-order and emits a
//! stream of [`RenderCommand`]s. Literal port of
//! `spine-cpp/src/spine/SkeletonRenderer.cpp`.
//!
//! Phase 6c lands the `RegionAttachment` emission path. Phase 6d
//! extends the walker to meshes; 6e hooks clipping in; 6g adds the
//! `batchCommands` merge pass.

use crate::data::attachment::TextureRegionRef;
use crate::data::{Attachment, MeshAttachment, RegionAttachment};
use crate::render::clipping::SkeletonClipping;
use crate::render::{pack_color, RenderCommand, TextureId};
use crate::skeleton::{Skeleton, Slot};

/// Stateful renderer: owns per-instance scratch buffers (world vertex
/// cache, quad index template) that would otherwise be re-allocated
/// every frame.
///
/// Reuse one `SkeletonRenderer` per skeleton instance (or share
/// across skeletons if you're rendering sequentially — the internal
/// buffers are reused, not accumulated).
#[derive(Debug)]
pub struct SkeletonRenderer {
    /// Per-attachment scratch: enough room for the largest attachment
    /// in the skeleton. Reused across attachments each frame.
    world_vertices: Vec<f32>,
    /// Canonical two-triangle quad index list `[0, 1, 2, 2, 3, 0]`
    /// reused for every `RegionAttachment`.
    quad_indices: [u16; 6],
    /// Accumulator for the per-slot commands emitted during a single
    /// `render` call. Cleared at the start of every call.
    render_commands: Vec<RenderCommand>,
    /// Triangle-polygon clipper. Stores per-region scratch state
    /// between `clip_start` and `clip_end`; active for every
    /// `ClippingAttachment`-bounded slot range (Phase 6e).
    clipping: SkeletonClipping,
}

impl SkeletonRenderer {
    /// Construct a fresh renderer with empty scratch buffers. Matches
    /// `SkeletonRenderer::SkeletonRenderer()` which also initialises
    /// the 6-element `_quadIndices` template.
    #[must_use]
    pub fn new() -> Self {
        Self {
            world_vertices: Vec::new(),
            quad_indices: [0, 1, 2, 2, 3, 0],
            render_commands: Vec::new(),
            clipping: SkeletonClipping::new(),
        }
    }

    /// Walk `skeleton`'s draw-order, emitting one [`RenderCommand`]
    /// per visible attachment. Skips slots with `alpha == 0`, inactive
    /// bones, or attachment kinds not yet supported.
    ///
    /// Literal port of `spine-cpp/src/spine/SkeletonRenderer.cpp` —
    /// the 6c phase covers the [`RegionAttachment`] branch; meshes
    /// (6d) and clipping (6e) follow.
    pub fn render(&mut self, skeleton: &Skeleton) -> &[RenderCommand] {
        self.render_commands.clear();
        self.world_vertices.clear();

        for &slot_id in &skeleton.draw_order {
            let slot = &skeleton.slots[slot_id.index()];
            let Some(attachment_id) = slot.attachment else {
                self.clipping.clip_end_on(slot_id);
                continue;
            };

            // Early-out: slot alpha == 0 OR bone inactive (unless
            // this is a `ClippingAttachment`, which spine-cpp lets
            // through so `clipEnd` / `clipStart` bookkeeping runs).
            let slot_bone = skeleton.data.slots[slot_id.index()].bone;
            let bone_active = skeleton.bones[slot_bone.index()].active;
            let attachment = &skeleton.data.attachments[attachment_id.index()];
            let is_clipping = matches!(attachment, Attachment::Clipping(_));
            if !is_clipping && (slot.color.a == 0.0 || !bone_active) {
                self.clipping.clip_end_on(slot_id);
                continue;
            }

            match attachment {
                Attachment::Region(region) => {
                    self.emit_region(skeleton, slot_id, slot, region);
                }
                Attachment::Mesh(mesh) => {
                    self.emit_mesh(skeleton, slot_id, slot, mesh);
                }
                Attachment::Clipping(_) => {
                    self.clipping.clip_start(skeleton, slot_id);
                    continue;
                }
                _ => {}
            }
            self.clipping.clip_end_on(slot_id);
        }
        self.clipping.clip_end();
        self.batch_commands();
        &self.render_commands
    }

    /// Merge adjacent commands that share `(texture, blend_mode,
    /// colors[0], dark_colors[0])` into a single command, as long as
    /// the merged index count stays under `0xffff` (`u16::MAX`).
    /// Literal port of `SkeletonRenderer::batchCommands`.
    ///
    /// Replaces `self.render_commands` with the batched set.
    #[allow(clippy::too_many_lines)]
    fn batch_commands(&mut self) {
        if self.render_commands.is_empty() {
            return;
        }
        let input = std::mem::take(&mut self.render_commands);
        let mut output: Vec<RenderCommand> = Vec::new();

        let mut start = 0usize;
        let mut i = 1usize;
        let mut num_vertices = input[0].num_vertices();
        let mut num_indices = input[0].num_indices();
        while i <= input.len() {
            let cmd = if i < input.len() {
                Some(&input[i])
            } else {
                None
            };

            if let Some(c) = cmd
                && c.num_vertices() == 0
                && c.num_indices() == 0
            {
                i += 1;
                continue;
            }

            let first = &input[start];
            let can_merge = cmd.is_some_and(|c| {
                c.texture == first.texture
                    && c.blend_mode == first.blend_mode
                    && c.colors.first() == first.colors.first()
                    && c.dark_colors.first() == first.dark_colors.first()
                    && (num_indices + c.num_indices()) < 0xffff
            });

            if can_merge {
                let c = cmd.unwrap();
                num_vertices += c.num_vertices();
                num_indices += c.num_indices();
            } else {
                output.push(batch_sub_commands(
                    &input,
                    start,
                    i - 1,
                    num_vertices,
                    num_indices,
                ));
                if i == input.len() {
                    break;
                }
                start = i;
                num_vertices = input[i].num_vertices();
                num_indices = input[i].num_indices();
            }
            i += 1;
        }
        self.render_commands = output;
    }

    /// Emit a `RegionAttachment` as a 4-vertex / 6-index quad.
    ///
    /// Mirrors the `RegionAttachment` branch of
    /// `SkeletonRenderer::render` + `RegionAttachment::computeWorldVertices`.
    #[allow(clippy::many_single_char_names)] // spine-cpp's a,b,c,d bone matrix + wx,wy.
    fn emit_region(
        &mut self,
        skeleton: &Skeleton,
        slot_id: crate::data::SlotId,
        slot: &Slot,
        region: &RegionAttachment,
    ) {
        if region.color.a == 0.0 {
            return;
        }
        let Some((texture, uvs)) = resolve_region_texture(region, slot.sequence_index) else {
            return;
        };

        let bone_id = skeleton.data.slots[slot_id.index()].bone;
        let bone = &skeleton.bones[bone_id.index()];
        let (wx, wy) = (bone.world_x, bone.world_y);
        let (a, b, c, d) = (bone.a, bone.b, bone.c, bone.d);
        let off = &region.vertex_offset;
        // spine-cpp emit order: BR, BL, UL, UR.
        let v = [
            off[BRX] * a + off[BRY] * b + wx,
            off[BRX] * c + off[BRY] * d + wy,
            off[BLX] * a + off[BLY] * b + wx,
            off[BLX] * c + off[BLY] * d + wy,
            off[ULX] * a + off[ULY] * b + wx,
            off[ULX] * c + off[ULY] * d + wy,
            off[URX] * a + off[URY] * b + wx,
            off[URX] * c + off[URY] * d + wy,
        ];

        let color = pack_slot_colors(skeleton, slot, region.color);
        let dark_color = pack_dark_color(slot);
        let blend = skeleton.data.slots[slot_id.index()].blend_mode;

        // Collect quad indices into a local buffer — can't borrow
        // `self.quad_indices` while calling `self.push_command`.
        let indices = [
            self.quad_indices[0],
            self.quad_indices[1],
            self.quad_indices[2],
            self.quad_indices[3],
            self.quad_indices[4],
            self.quad_indices[5],
        ];

        self.push_command(&v, &uvs, &indices, color, dark_color, blend, texture);
    }

    /// Emit a `MeshAttachment` — N-vertex / M-index triangle list.
    ///
    /// Mirrors the `MeshAttachment` branch of
    /// `SkeletonRenderer::render`. World vertices come from the shared
    /// `Skeleton::compute_world_vertices` helper (ported in 6a);
    /// uvs/triangles come straight from the attachment.
    ///
    /// Sequence-driven region cycling is deferred to a later sub-phase
    /// — meshes with a non-setup `sequence_index` fall back to the
    /// attachment's resolved region and setup-pose UVs.
    fn emit_mesh(
        &mut self,
        skeleton: &Skeleton,
        slot_id: crate::data::SlotId,
        slot: &Slot,
        mesh: &MeshAttachment,
    ) {
        if mesh.color.a == 0.0 {
            return;
        }
        let Some(region) = mesh.region.as_ref() else {
            return;
        };

        let world_len = mesh.vertex_data.world_vertices_length as usize;
        if world_len == 0 || mesh.triangles.is_empty() || mesh.uvs.is_empty() {
            return;
        }
        self.world_vertices.resize(world_len.max(self.world_vertices.len()), 0.0);
        skeleton.compute_world_vertices(
            &mesh.vertex_data,
            slot_id,
            0,
            world_len,
            &mut self.world_vertices,
            0,
            2,
        );

        let color = pack_slot_colors(skeleton, slot, mesh.color);
        let dark_color = pack_dark_color(slot);
        let blend = skeleton.data.slots[slot_id.index()].blend_mode;

        // Re-borrow world_vertices as a fresh slice to satisfy the
        // borrow checker (push_command takes &mut self).
        let world_slice = self.world_vertices[..world_len].to_vec();
        let triangles = mesh.triangles.clone();
        let uvs = mesh.uvs.clone();
        self.push_command(
            &world_slice,
            &uvs,
            &triangles,
            color,
            dark_color,
            blend,
            TextureId(region.page_index),
        );
    }

    /// Route a triangle list + uvs through the clipper (if active)
    /// and emit one `RenderCommand` with the final geometry.
    #[allow(clippy::too_many_arguments)]
    fn push_command(
        &mut self,
        positions: &[f32],
        uvs: &[f32],
        indices: &[u16],
        color: u32,
        dark_color: u32,
        blend: crate::data::BlendMode,
        texture: TextureId,
    ) {
        let (positions, uvs, indices) = if self.clipping.is_clipping() {
            self.clipping.clip_triangles(positions, indices, uvs, 2);
            if self.clipping.clipped_vertices().is_empty() {
                return;
            }
            (
                self.clipping.clipped_vertices(),
                self.clipping.clipped_uvs(),
                self.clipping.clipped_triangles(),
            )
        } else {
            (positions, uvs, indices)
        };
        let num_vertices = positions.len() / 2;
        let num_indices = indices.len();
        if num_vertices == 0 || num_indices == 0 {
            return;
        }
        let mut cmd = RenderCommand::with_capacity(num_vertices, num_indices, blend, texture);
        cmd.positions.copy_from_slice(positions);
        cmd.uvs.copy_from_slice(uvs);
        for c in &mut cmd.colors {
            *c = color;
        }
        for c in &mut cmd.dark_colors {
            *c = dark_color;
        }
        cmd.indices.copy_from_slice(indices);
        self.render_commands.push(cmd);
    }

    /// Capacity of the internal world-vertex scratch buffer, in
    /// `f32`s. Useful to sanity-check scratch reuse across frames.
    #[must_use]
    pub fn world_vertex_capacity(&self) -> usize {
        self.world_vertices.capacity()
    }

    /// Reference to the canonical quad index template used for
    /// `RegionAttachment` emission.
    #[must_use]
    pub fn quad_indices(&self) -> &[u16; 6] {
        &self.quad_indices
    }
}

impl Default for SkeletonRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Concatenate `commands[first..=last]` into a single batched
/// `RenderCommand`. Rebases each source command's index buffer by
/// the running vertex offset so the merged indices stay valid.
/// Literal port of `SkeletonRenderer::batchSubCommands`.
fn batch_sub_commands(
    commands: &[RenderCommand],
    first: usize,
    last: usize,
    num_vertices: usize,
    num_indices: usize,
) -> RenderCommand {
    let f = &commands[first];
    let mut out = RenderCommand::with_capacity(num_vertices, num_indices, f.blend_mode, f.texture);

    let mut pos_w = 0;
    let mut col_w = 0;
    let mut idx_w = 0;
    let mut indices_offset: u16 = 0;

    for cmd in &commands[first..=last] {
        let n = cmd.num_vertices();
        let nf = n * 2;
        out.positions[pos_w..pos_w + nf].copy_from_slice(&cmd.positions);
        out.uvs[pos_w..pos_w + nf].copy_from_slice(&cmd.uvs);
        pos_w += nf;
        out.colors[col_w..col_w + n].copy_from_slice(&cmd.colors);
        out.dark_colors[col_w..col_w + n].copy_from_slice(&cmd.dark_colors);
        col_w += n;
        for (k, &ix) in cmd.indices.iter().enumerate() {
            out.indices[idx_w + k] = ix + indices_offset;
        }
        idx_w += cmd.num_indices();
        indices_offset += n as u16;
    }
    out
}

/// Pack the skeleton × slot × attachment-color blend into a single
/// `u32`. Literal port of the `SkeletonRenderer` color math.
fn pack_slot_colors(skeleton: &Skeleton, slot: &Slot, attachment_color: crate::math::Color) -> u32 {
    let sc = skeleton.color;
    let scol = slot.color;
    let acol = attachment_color;
    pack_color(
        sc.r * scol.r * acol.r,
        sc.g * scol.g * acol.g,
        sc.b * scol.b * acol.b,
        sc.a * scol.a * acol.a,
    )
}

/// Pack the slot's dark-color (tint-black). Fixed alpha = 1.0 per
/// spine-cpp. Defaults to `0xff00_0000` when the slot carries no
/// dark color.
fn pack_dark_color(slot: &Slot) -> u32 {
    if let Some(dark) = slot.dark_color {
        pack_color(dark.r, dark.g, dark.b, 1.0)
    } else {
        0xff00_0000
    }
}

// --- Corner offset / UV indices ---------------------------------------------
// spine-cpp const ints — reproduced here so the vertex code matches the
// reference visually. Order: BL, UL, UR, BR (low-to-high in memory).
const BLX: usize = 0;
const BLY: usize = 1;
const ULX: usize = 2;
const ULY: usize = 3;
const URX: usize = 4;
const URY: usize = 5;
const BRX: usize = 6;
const BRY: usize = 7;

/// Resolve the active `(TextureId, UVs)` for a region attachment.
///
/// Without a `Sequence`, returns the attachment's stored region /
/// UVs directly. With a sequence, picks `regions[sequence_index]`
/// (or `setup_index` when the slot's index is `-1`), recomputes the
/// UV corners from the chosen region, and returns those.
///
/// Returns `None` when no region can be resolved — the walker skips
/// those slots.
fn resolve_region_texture(
    region: &RegionAttachment,
    slot_sequence_index: i32,
) -> Option<(TextureId, [f32; 8])> {
    if let Some(seq) = region.sequence.as_ref() {
        // Pick the active sequence frame.
        let mut idx = if slot_sequence_index == -1 {
            seq.setup_index
        } else {
            slot_sequence_index
        };
        let n = seq.regions.len() as i32;
        if idx >= n {
            idx = n - 1;
        }
        if idx < 0 {
            return None;
        }
        let Some(Some(r)) = seq.regions.get(idx as usize) else {
            return None;
        };
        let uvs = region_uvs_for(r);
        return Some((TextureId(r.page_index), uvs));
    }
    // No sequence: use the attachment's resolved region + stored UVs.
    let r = region.region.as_ref()?;
    let uvs = region.uvs;
    Some((TextureId(r.page_index), uvs))
}

/// Compute the four-corner UVs from a region reference, accounting for
/// the `degrees` rotation flag (spine-cpp does this in
/// `RegionAttachment::updateRegion`).
fn region_uvs_for(r: &TextureRegionRef) -> [f32; 8] {
    if r.degrees == 90 {
        // Rotated region layout (matches spine-cpp updateRegion's
        // `degrees == 90` branch).
        [
            r.u,  r.v2, // BL
            r.u,  r.v,  // UL
            r.u2, r.v,  // UR
            r.u2, r.v2, // BR
        ]
    } else {
        [
            r.u,  r.v2, // BL
            r.u2, r.v2, // UL
            r.u2, r.v,  // UR
            r.u,  r.v,  // BR
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_canonical_quad_indices() {
        let r = SkeletonRenderer::new();
        assert_eq!(r.quad_indices(), &[0, 1, 2, 2, 3, 0]);
    }

    #[test]
    fn default_and_new_match() {
        let a = SkeletonRenderer::default();
        let b = SkeletonRenderer::new();
        assert_eq!(a.quad_indices(), b.quad_indices());
        assert_eq!(a.world_vertex_capacity(), b.world_vertex_capacity());
    }

    /// End-to-end: load spineboy, render setup pose, confirm we emit
    /// at least one region command whose vertex layout matches the
    /// expected BR/BL/UL/UR shape.
    #[test]
    fn renders_spineboy_setup_pose() {
        use crate::atlas::Atlas;
        use crate::load::{AtlasAttachmentLoader, SkeletonBinary};
        use crate::skeleton::{Physics, Skeleton};
        use std::sync::Arc;

        let Ok(atlas_src) = std::fs::read_to_string(
            "../spine-runtimes/examples/spineboy/export/spineboy.atlas",
        ) else {
            return; // test-only best-effort; skip if examples aren't present
        };
        let atlas = Atlas::parse(&atlas_src).unwrap();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let bytes = std::fs::read(
            "../spine-runtimes/examples/spineboy/export/spineboy-pro.skel",
        )
        .unwrap();
        let data = Arc::new(
            SkeletonBinary::with_loader(&mut loader)
                .read(&bytes)
                .unwrap(),
        );

        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();
        sk.set_to_setup_pose();
        sk.update_world_transform(Physics::None);

        let mut renderer = SkeletonRenderer::new();
        let cmds = renderer.render(&sk);

        assert!(
            !cmds.is_empty(),
            "spineboy setup pose should emit at least one render command"
        );
        for (i, cmd) in cmds.iter().enumerate() {
            assert_eq!(
                cmd.positions.len() % 2,
                0,
                "command[{i}]: positions must be even-length (interleaved xy)"
            );
            assert_eq!(
                cmd.uvs.len(),
                cmd.positions.len(),
                "command[{i}]: uvs and positions must have matching length"
            );
            assert_eq!(cmd.colors.len(), cmd.num_vertices());
            assert_eq!(cmd.dark_colors.len(), cmd.num_vertices());
            // Non-zero geometry in every emitted command.
            assert!(cmd.num_vertices() > 0, "command[{i}]: zero vertices");
            assert!(
                cmd.num_indices() % 3 == 0,
                "command[{i}]: indices must be a multiple of 3 (triangle list)"
            );
            // No non-finite world coordinates.
            for (k, v) in cmd.positions.iter().enumerate() {
                assert!(
                    v.is_finite(),
                    "command[{i}].positions[{k}] = {v} is not finite"
                );
            }
        }
    }

    #[test]
    fn region_uvs_for_unrotated() {
        let r = TextureRegionRef {
            page_index: 0,
            u: 0.1,
            v: 0.2,
            u2: 0.9,
            v2: 0.8,
            width: 10.0,
            height: 10.0,
            original_width: 10.0,
            original_height: 10.0,
            offset_x: 0.0,
            offset_y: 0.0,
            degrees: 0,
        };
        let uvs = region_uvs_for(&r);
        // BL, UL, UR, BR
        assert_eq!(&uvs[0..2], &[0.1, 0.8]);
        assert_eq!(&uvs[2..4], &[0.9, 0.8]);
        assert_eq!(&uvs[4..6], &[0.9, 0.2]);
        assert_eq!(&uvs[6..8], &[0.1, 0.2]);
    }
}
