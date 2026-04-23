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
//! The walker itself lives in sub-phase 6c; 6e hooks clipping in; 6g
//! adds the `batchCommands` merge pass. For now this file carries only
//! the `SkeletonRenderer` type and its scratch buffers, which 6a fixes
//! as the public API shape so the downstream Bevy crate can depend on
//! it immediately.

use crate::render::RenderCommand;
use crate::skeleton::Skeleton;

/// Stateful renderer: owns per-instance scratch buffers (world vertex
/// cache, quad index template, clipping scratch) that would otherwise
/// be re-allocated every frame.
///
/// Reuse one `SkeletonRenderer` per skeleton instance (or share across
/// skeletons if you're rendering sequentially — the internal buffers
/// are reused, not accumulated).
#[derive(Debug)]
pub struct SkeletonRenderer {
    /// Per-attachment scratch: enough room for the largest attachment
    /// in the skeleton. Reused across attachments each frame.
    world_vertices: Vec<f32>,
    /// Canonical two-triangle quad index list `[0, 1, 2, 2, 3, 0]`
    /// reused for every `RegionAttachment`.
    quad_indices: [u16; 6],
    /// Accumulator for the per-slot commands emitted during a single
    /// `render` call. Drained into the returned `Vec<RenderCommand>`
    /// and cleared at the start of every call.
    render_commands: Vec<RenderCommand>,
    // TODO(6e): `clipping: SkeletonClipping` scratch buffers land
    // here once the clipper is ported.
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
        }
    }

    /// Walk `skeleton`'s draw-order, emitting one [`RenderCommand`]
    /// per visible attachment (merged into batched runs by the
    /// batching pass in sub-phase 6g).
    ///
    /// **Stub until sub-phase 6c lands** — currently returns an empty
    /// command stream so downstream crates can type-check against the
    /// API. Implementation follows `SkeletonRenderer::render()`
    /// literally.
    pub fn render(&mut self, _skeleton: &Skeleton) -> &[RenderCommand] {
        self.render_commands.clear();
        // Scratch buffers retain their capacity between frames —
        // `clear()` is deliberate.
        self.world_vertices.clear();

        // TODO(6c): iterate `skeleton.draw_order`, resolve each slot's
        // attachment to geometry, emit RegionAttachment and
        // MeshAttachment commands.
        // TODO(6e): call clipper.clipTriangles between geometry
        // computation and emission.
        // TODO(6g): final batchCommands pass.

        &self.render_commands
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
}
