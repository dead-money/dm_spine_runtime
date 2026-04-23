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

//! Render-command emission — the boundary between the core Spine
//! runtime and any downstream renderer (e.g. [`dm_spine_bevy`]).
//!
//! This module owns two things:
//!
//! - The [`RenderCommand`] data layout: interleaved positions + UVs,
//!   packed `u32` colors, `u16` indices, blend mode, and an opaque
//!   [`TextureId`]. Matches `spine-cpp`'s `RenderCommand` struct
//!   exactly so the golden-capture harness can dump byte-comparable
//!   output.
//! - The [`SkeletonRenderer`] walker (sub-phase 6c) that iterates a
//!   `Skeleton`'s draw-order, resolves each attachment's geometry, and
//!   emits one or more `RenderCommand`s — with clipping (6e) and
//!   batching (6g) wired in as those sub-phases land.
//!
//! **No GPU / windowing deps.** `TextureId` is a newtype over `u32`
//! (the atlas page index); the downstream renderer maps it to whatever
//! GPU-side texture handle it owns. Vertex/index/color buffers are
//! plain `Vec<_>` — the consumer copies or re-wraps as needed.
//!
//! [`dm_spine_bevy`]: https://github.com/deadmoney/dm_spine_bevy

use crate::data::BlendMode;

pub mod clipping;
pub mod renderer;

pub use clipping::SkeletonClipping;
pub use renderer::SkeletonRenderer;

/// Opaque texture identifier emitted on every [`RenderCommand`]. Wraps
/// the atlas page index (`AtlasRegion::page_index` / `TextureRegionRef::page_index`).
/// Downstream renderers resolve this to their own GPU handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

impl TextureId {
    /// Sentinel used when an attachment has no resolved texture region
    /// (e.g. the loader failed to find its atlas entry). Renderers may
    /// treat commands with this id as invisible or flag them as errors.
    pub const MISSING: TextureId = TextureId(u32::MAX);
}

/// A batch of drawable geometry that shares one texture, one blend
/// mode, one tint, and one dark-color tint. Literal port of
/// `spine-cpp`'s `RenderCommand` struct, with owning `Vec`s in place
/// of spine-cpp's block-allocator slices.
///
/// ## Buffer layout
///
/// | Field        | Length          | Layout                                      |
/// |--------------|-----------------|---------------------------------------------|
/// | `positions`  | `2 * vertices`  | Interleaved `x, y` in skeleton world space  |
/// | `uvs`        | `2 * vertices`  | Interleaved `u, v` in atlas space (0..1)    |
/// | `colors`     | `vertices`      | Packed `0xAARRGGBB` (premultiplied alpha)   |
/// | `dark_colors`| `vertices`      | Packed `0xAARRGGBB` for tint-black          |
/// | `indices`    | `indices`       | Triangle-list `u16` into this command       |
///
/// `vertices = positions.len() / 2` and `indices = indices.len()`; the
/// numeric counts are not stored separately.
///
/// The same `color` / `dark_color` value is duplicated across every
/// vertex. spine-cpp does this so the batcher can compare commands in
/// O(1); we mirror the layout to keep golden-capture diffs exact.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderCommand {
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub colors: Vec<u32>,
    pub dark_colors: Vec<u32>,
    pub indices: Vec<u16>,
    pub blend_mode: BlendMode,
    pub texture: TextureId,
}

impl RenderCommand {
    /// Number of vertices in this command (`positions.len() / 2`).
    #[must_use]
    #[inline]
    pub fn num_vertices(&self) -> usize {
        self.positions.len() / 2
    }

    /// Number of indices in this command (equals `indices.len()`;
    /// exposed for symmetry with [`Self::num_vertices`]).
    #[must_use]
    #[inline]
    pub fn num_indices(&self) -> usize {
        self.indices.len()
    }

    /// Empty command with the given `blend_mode` / `texture` and
    /// `num_vertices` / `num_indices` reserved but uninitialised.
    /// Used by the walker to allocate command storage before filling.
    #[allow(dead_code)] // Used by 6c walker; unit-tested here in 6a.
    #[must_use]
    pub(crate) fn with_capacity(
        num_vertices: usize,
        num_indices: usize,
        blend_mode: BlendMode,
        texture: TextureId,
    ) -> Self {
        Self {
            positions: vec![0.0; num_vertices * 2],
            uvs: vec![0.0; num_vertices * 2],
            colors: vec![0; num_vertices],
            dark_colors: vec![0; num_vertices],
            indices: vec![0; num_indices],
            blend_mode,
            texture,
        }
    }
}

/// Pack four 0..1 floats into `0xAARRGGBB`. Matches spine-cpp's
/// `SkeletonRenderer` color packing exactly — including the
/// truncating `f32 -> u8` cast (no round-to-nearest, matches C++
/// `static_cast<uint8_t>`).
///
/// Used by the `SkeletonRenderer` walker (6c) to produce
/// `RenderCommand::colors` and `RenderCommand::dark_colors`.
#[allow(dead_code)] // Used by 6c walker; unit-tested here in 6a.
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
        // White, opaque
        assert_eq!(pack_color(1.0, 1.0, 1.0, 1.0), 0xffff_ffff);
        // Opaque black
        assert_eq!(pack_color(0.0, 0.0, 0.0, 1.0), 0xff00_0000);
        // Fully transparent
        assert_eq!(pack_color(1.0, 1.0, 1.0, 0.0), 0x00ff_ffff);
        // Pure red, half alpha
        // 0.5 * 255 = 127.5 → truncates to 127 (matching C++ static_cast).
        assert_eq!(pack_color(1.0, 0.0, 0.0, 0.5), 0x7fff_0000);
    }

    #[test]
    fn texture_id_missing_is_u32_max() {
        assert_eq!(TextureId::MISSING, TextureId(u32::MAX));
    }

    #[test]
    fn with_capacity_allocates_zeroed_buffers() {
        let c = RenderCommand::with_capacity(4, 6, BlendMode::Normal, TextureId(0));
        assert_eq!(c.positions.len(), 8);
        assert_eq!(c.uvs.len(), 8);
        assert_eq!(c.colors.len(), 4);
        assert_eq!(c.dark_colors.len(), 4);
        assert_eq!(c.indices.len(), 6);
        assert_eq!(c.num_vertices(), 4);
        assert_eq!(c.num_indices(), 6);
    }
}
