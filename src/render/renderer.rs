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

//! [`SkeletonRenderer`]: walks the applied draw order and emits batched
//! [`RenderCommand`]s, clipping as it goes.
//!
//! Adjacent slots sharing texture, blend mode, color and dark color are
//! merged while the batch stays under 65535 indices, as spine-cpp's
//! `batchCommands` does; [`RenderOptions::merge_colors`] drops the color
//! condition. Commands and their buffers are reused between calls, so
//! steady-state rendering doesn't allocate.

use crate::data::attachment::quad_corner::{BLX, BLY, BRX, BRY, ULX, ULY, URX, URY};
use crate::data::{Attachment, BlendMode};
use crate::math::Color;
use crate::render::clipping::SkeletonClipping;
use crate::render::{RenderCommand, RenderOptions, TextureId, pack_color};
use crate::skeleton::Skeleton;

const QUAD_INDICES: [u16; 6] = [0, 1, 2, 2, 3, 0];

/// Turns a posed skeleton into [`RenderCommand`]s. Reuse one across frames;
/// its buffers persist between calls.
///
/// Only region and mesh attachments draw. A slot is skipped when its color
/// or its attachment's color has alpha 0, when its bone is inactive, or when
/// the attachment has no resolved texture region.
#[derive(Debug, Default)]
pub struct SkeletonRenderer {
    commands: Vec<RenderCommand>,
    len: usize,
    world_vertices: Vec<f32>,
    clipping: SkeletonClipping,
    options: RenderOptions,
}

/// One slot's contribution to a command.
struct Draw<'a> {
    positions: &'a [f32],
    uvs: &'a [f32],
    indices: &'a [u16],
    color: u32,
    dark_color: u32,
    blend_mode: BlendMode,
    texture: TextureId,
    slot: u16,
    tag: u32,
}

impl SkeletonRenderer {
    /// A renderer with default [`RenderOptions`], which match spine-cpp.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A renderer with the given options.
    #[must_use]
    pub fn with_options(options: RenderOptions) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn options(&self) -> RenderOptions {
        self.options
    }

    /// Takes effect on the next render.
    pub fn set_options(&mut self, options: RenderOptions) {
        self.options = options;
    }

    /// Commands from the last [`Self::render`] or [`Self::render_unbatched`].
    #[must_use]
    pub fn commands(&self) -> &[RenderCommand] {
        &self.commands[..self.len]
    }

    /// Emits commands from the skeleton's current world transforms, so call
    /// it after [`Skeleton::update_world_transform`]. The returned slice is
    /// valid until the next render call.
    pub fn render(&mut self, skeleton: &Skeleton) -> &[RenderCommand] {
        self.render_with(skeleton, true)
    }

    /// As [`Self::render`], but with one command per drawn slot, for
    /// debugging.
    pub fn render_unbatched(&mut self, skeleton: &Skeleton) -> &[RenderCommand] {
        self.render_with(skeleton, false)
    }

    fn render_with(&mut self, skeleton: &Skeleton, batch: bool) -> &[RenderCommand] {
        self.len = 0;
        let sd = skeleton.data();
        for &slot_id in skeleton.draw_order.applied() {
            let slot = &skeleton.slots[slot_id.index()];
            let pose = slot.applied();
            let Some(attachment_id) = pose.attachment else {
                self.clipping.clip_end_slot(slot_id);
                continue;
            };
            let attachment = skeleton.attachment(attachment_id);
            let is_clip = matches!(attachment, Attachment::Clipping(_));
            if (pose.color.a == 0.0 || !skeleton.bones[slot.bone.index()].active) && !is_clip {
                self.clipping.clip_end_slot(slot_id);
                continue;
            }

            let (vertex_count, attachment_color, texture, uvs, triangles): (
                usize,
                Color,
                _,
                &[f32],
                &[u16],
            );
            match attachment {
                Attachment::Region(region) => {
                    if region.color.a == 0.0 {
                        self.clipping.clip_end_slot(slot_id);
                        continue;
                    }
                    let index = region.sequence.resolve_index(pose.sequence_index);
                    let Some(r) = region.sequence.region(index) else {
                        self.clipping.clip_end_slot(slot_id);
                        continue;
                    };
                    let off = region.sequence.offsets(index);
                    let b = skeleton.bones[slot.bone.index()].applied();
                    let (a, bb, c, d, wx, wy) = (b.a, b.b, b.c, b.d, b.world_x, b.world_y);
                    self.world_vertices.clear();
                    for (ox, oy) in [(BRX, BRY), (BLX, BLY), (ULX, ULY), (URX, URY)] {
                        self.world_vertices.push(off[ox] * a + off[oy] * bb + wx);
                        self.world_vertices.push(off[ox] * c + off[oy] * d + wy);
                    }
                    vertex_count = 4;
                    attachment_color = region.color;
                    texture = TextureId(r.page_index);
                    uvs = region.sequence.uvs(index);
                    triangles = &QUAD_INDICES;
                }
                Attachment::Mesh(mesh) => {
                    if mesh.color.a == 0.0 {
                        self.clipping.clip_end_slot(slot_id);
                        continue;
                    }
                    let index = mesh.sequence.resolve_index(pose.sequence_index);
                    let Some(r) = mesh.sequence.region(index) else {
                        self.clipping.clip_end_slot(slot_id);
                        continue;
                    };
                    let n = mesh.vertex_data.world_vertices_length as usize;
                    self.world_vertices.clear();
                    self.world_vertices.resize(n, 0.0);
                    skeleton.compute_world_vertices(
                        &mesh.vertex_data,
                        slot_id,
                        0,
                        n,
                        &mut self.world_vertices,
                        0,
                        2,
                    );
                    vertex_count = n >> 1;
                    attachment_color = mesh.color;
                    texture = TextureId(r.page_index);
                    uvs = mesh.sequence.uvs(index);
                    triangles = &mesh.triangles;
                }
                Attachment::Clipping(clip) => {
                    self.clipping.clip_start(skeleton, slot_id, clip);
                    continue;
                }
                _ => continue,
            }

            let sc = skeleton.color;
            let color = pack_color(
                sc.r * pose.color.r * attachment_color.r,
                sc.g * pose.color.g * attachment_color.g,
                sc.b * pose.color.b * attachment_color.b,
                sc.a * pose.color.a * attachment_color.a,
            );
            let dark_color = if pose.has_dark_color {
                let dc = pose.dark_color;
                0xff00_0000 | (pack_color(dc.r, dc.g, dc.b, 0.0) & 0x00ff_ffff)
            } else {
                0xff00_0000
            };
            let blend_mode = sd.slots[slot_id.index()].blend_mode;
            let (slot, tag) = (slot_id.0, attachment.tag());

            let draw = if self.clipping.is_clipping() {
                self.clipping
                    .clip_triangles(&self.world_vertices, triangles, uvs, 2);
                let clipped_vertices = self.clipping.clipped_vertices().len() >> 1;
                Draw {
                    positions: &self.clipping.clipped_vertices()[..clipped_vertices * 2],
                    uvs: self.clipping.clipped_uvs(),
                    indices: self.clipping.clipped_triangles(),
                    color,
                    dark_color,
                    blend_mode,
                    texture,
                    slot,
                    tag,
                }
            } else {
                Draw {
                    positions: &self.world_vertices[..vertex_count * 2],
                    uvs: &uvs[..vertex_count * 2],
                    indices: triangles,
                    color,
                    dark_color,
                    blend_mode,
                    texture,
                    slot,
                    tag,
                }
            };
            push(
                &mut self.commands,
                &mut self.len,
                batch,
                self.options,
                &draw,
            );
            self.clipping.clip_end_slot(slot_id);
        }
        self.clipping.clip_end();
        &self.commands[..self.len]
    }
}

/// Appends one slot's geometry, merging into the current command when the
/// batch key matches.
fn push(
    commands: &mut Vec<RenderCommand>,
    len: &mut usize,
    batch: bool,
    options: RenderOptions,
    draw: &Draw<'_>,
) {
    if draw.positions.is_empty() && draw.indices.is_empty() {
        return;
    }
    let merge = batch && *len > 0 && {
        let last = &commands[*len - 1];
        last.texture == draw.texture
            && last.blend_mode == draw.blend_mode
            && (options.merge_colors
                || last.colors.first() == Some(&draw.color)
                    && last.dark_colors.first() == Some(&draw.dark_color))
            && last.indices.len() + draw.indices.len() < 0xffff
    };
    if !merge {
        if *len == commands.len() {
            commands.push(RenderCommand {
                positions: Vec::new(),
                uvs: Vec::new(),
                colors: Vec::new(),
                dark_colors: Vec::new(),
                indices: Vec::new(),
                slots: Vec::new(),
                tags: Vec::new(),
                blend_mode: draw.blend_mode,
                texture: draw.texture,
            });
        }
        let cmd = &mut commands[*len];
        cmd.positions.clear();
        cmd.uvs.clear();
        cmd.colors.clear();
        cmd.dark_colors.clear();
        cmd.indices.clear();
        cmd.slots.clear();
        cmd.tags.clear();
        cmd.blend_mode = draw.blend_mode;
        cmd.texture = draw.texture;
        *len += 1;
    }
    let cmd = &mut commands[*len - 1];
    let base = (cmd.positions.len() / 2) as u16;
    let vertex_count = draw.positions.len() / 2;
    cmd.positions.extend_from_slice(draw.positions);
    cmd.uvs.extend_from_slice(&draw.uvs[..vertex_count * 2]);
    cmd.colors
        .extend(std::iter::repeat_n(draw.color, vertex_count));
    cmd.dark_colors
        .extend(std::iter::repeat_n(draw.dark_color, vertex_count));
    cmd.indices.extend(draw.indices.iter().map(|&i| i + base));
    if options.vertex_ids {
        cmd.slots
            .extend(std::iter::repeat_n(draw.slot, vertex_count));
        cmd.tags.extend(std::iter::repeat_n(draw.tag, vertex_count));
    }
}
