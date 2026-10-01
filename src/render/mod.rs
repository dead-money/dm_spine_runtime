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

//! Render-command emission, the boundary between the runtime and a renderer.
//!
//! [`SkeletonRenderer`] walks a skeleton's draw order and emits
//! [`RenderCommand`]s: plain vertex, color and index buffers tagged with a
//! blend mode and an opaque [`TextureId`] (the atlas page index). The
//! renderer maps that id to its own GPU texture. Clipping attachments are
//! applied on the CPU by [`SkeletonClipping`], so commands need no stencil.
//!
//! ```no_run
//! use spine_runtime::render::SkeletonRenderer;
//! use spine_runtime::skeleton::{Physics, Skeleton};
//!
//! fn draw(skeleton: &mut Skeleton, renderer: &mut SkeletonRenderer) {
//!     skeleton.update_world_transform(Physics::Update);
//!     for cmd in renderer.render(skeleton) {
//!         // Upload cmd.positions, cmd.uvs, cmd.colors and cmd.indices, then
//!         // draw with the texture for cmd.texture and cmd.blend_mode.
//!         let _ = (cmd.num_vertices(), cmd.texture, cmd.blend_mode);
//!     }
//! }
//! ```

use crate::data::BlendMode;

pub mod clipping;
pub mod renderer;

pub use clipping::SkeletonClipping;
pub use renderer::SkeletonRenderer;

/// Texture of a [`RenderCommand`]: the atlas page index
/// ([`AtlasPage::index`](crate::atlas::AtlasPage::index), carried on
/// [`TextureRegionRef::page_index`](crate::data::attachment::TextureRegionRef::page_index)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

impl TextureId {
    /// Sentinel for "no texture". [`SkeletonRenderer`] never emits it; it
    /// skips attachments that have no resolved region.
    pub const MISSING: TextureId = TextureId(u32::MAX);
}

/// A batch of triangles that share one texture and one blend mode.
///
/// ## Buffer layout
///
/// | Field        | Length          | Layout                                      |
/// |--------------|-----------------|---------------------------------------------|
/// | `positions`  | `2 * vertices`  | Interleaved `x, y` in skeleton world space  |
/// | `uvs`        | `2 * vertices`  | Interleaved `u, v` in atlas space (0..1)    |
/// | `colors`     | `vertices`      | Packed `0xAARRGGBB`, not premultiplied      |
/// | `dark_colors`| `vertices`      | Packed `0xFFRRGGBB` for tint-black; black without one |
/// | `indices`    | `indices`       | Triangle-list `u16` into this command       |
/// | `slots`      | `vertices`      | Slot index; only with [`RenderOptions::vertex_ids`] |
/// | `tags`       | `vertices`      | Attachment [`tag`](crate::data::Attachment::tag); only with `vertex_ids` |
///
/// Each slot's color is repeated across its vertices. By default only slots
/// with the same colors share a command, as in spine-cpp, so every vertex in
/// a command has the same color and dark color.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderCommand {
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub colors: Vec<u32>,
    pub dark_colors: Vec<u32>,
    pub indices: Vec<u16>,
    pub slots: Vec<u16>,
    pub tags: Vec<u32>,
    pub blend_mode: BlendMode,
    pub texture: TextureId,
}

/// [`SkeletonRenderer`] behavior beyond spine-cpp's. Both default off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    /// Fill [`RenderCommand::slots`] and [`RenderCommand::tags`], so a
    /// shader can tell slots apart inside a merged command.
    pub vertex_ids: bool,
    /// Merge adjacent slots whose colors differ, so commands break only on
    /// texture, blend mode and index count. Colors stay per vertex.
    pub merge_colors: bool,
}

impl RenderCommand {
    /// Vertex count, `positions.len() / 2`.
    #[must_use]
    #[inline]
    pub fn num_vertices(&self) -> usize {
        self.positions.len() / 2
    }

    /// Index count, a multiple of 3.
    #[must_use]
    #[inline]
    pub fn num_indices(&self) -> usize {
        self.indices.len()
    }

    /// World-space bounding box of the vertices as
    /// `(x_min, x_max, y_min, y_max)`, or `None` for an empty command.
    #[must_use]
    pub fn position_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let n = self.num_vertices();
        if n == 0 {
            return None;
        }
        let mut xmin = f32::INFINITY;
        let mut xmax = f32::NEG_INFINITY;
        let mut ymin = f32::INFINITY;
        let mut ymax = f32::NEG_INFINITY;
        for i in 0..n {
            let x = self.positions[i * 2];
            let y = self.positions[i * 2 + 1];
            xmin = xmin.min(x);
            xmax = xmax.max(x);
            ymin = ymin.min(y);
            ymax = ymax.max(y);
        }
        Some((xmin, xmax, ymin, ymax))
    }
}

/// Packs four 0..1 floats into `0xAARRGGBB`. Truncates rather than rounds,
/// as spine-cpp's `static_cast<uint8_t>` does, so render goldens match exactly.
#[allow(dead_code)]
#[must_use]
#[inline]
pub(crate) fn pack_color(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let ri = (r * 255.0) as u32 & 0xff;
    let gi = (g * 255.0) as u32 & 0xff;
    let bi = (b * 255.0) as u32 & 0xff;
    let ai = (a * 255.0) as u32 & 0xff;
    (ai << 24) | (ri << 16) | (gi << 8) | bi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_color_matches_spine_cpp_layout() {
        assert_eq!(pack_color(1.0, 1.0, 1.0, 1.0), 0xffff_ffff);
        assert_eq!(pack_color(0.0, 0.0, 0.0, 1.0), 0xff00_0000);
        assert_eq!(pack_color(1.0, 1.0, 1.0, 0.0), 0x00ff_ffff);
        // 0.5 * 255 = 127.5 truncates to 127.
        assert_eq!(pack_color(1.0, 0.0, 0.0, 0.5), 0x7fff_0000);
    }

    #[test]
    fn texture_id_missing_is_u32_max() {
        assert_eq!(TextureId::MISSING, TextureId(u32::MAX));
    }
}
