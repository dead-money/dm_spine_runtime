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

//! Attachment data types.
//!
//! In spine-cpp these form a virtual class hierarchy rooted at `Attachment`
//! with a `VertexAttachment` base for mesh-like variants. We flatten to a
//! tagged enum: each variant owns its full data inline, and vertex-based
//! variants embed a common [`VertexData`] struct instead of inheriting from
//! one.

use crate::data::AttachmentId;
use crate::math::Color;

/// Kind tag for an [`Attachment`]. Useful when the concrete variant has
/// already been resolved elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentType {
    Region,
    Mesh,
    /// Only appears transiently during binary parsing; resolved to `Mesh`
    /// with `parent_mesh = Some(_)` by end of load.
    LinkedMesh,
    BoundingBox,
    Path,
    Point,
    Clipping,
}

/// Tagged attachment union. Every skin's attachment slot references an
/// [`AttachmentId`] which indexes into [`SkeletonData::attachments`].
///
/// [`SkeletonData::attachments`]: crate::data::SkeletonData::attachments
#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
    Region(RegionAttachment),
    Mesh(MeshAttachment),
    BoundingBox(BoundingBoxAttachment),
    Path(PathAttachment),
    Point(PointAttachment),
    Clipping(ClippingAttachment),
}

impl Attachment {
    /// The attachment's name (unique within its skin+slot scope).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Attachment::Region(a) => &a.name,
            Attachment::Mesh(a) => &a.name,
            Attachment::BoundingBox(a) => &a.name,
            Attachment::Path(a) => &a.name,
            Attachment::Point(a) => &a.name,
            Attachment::Clipping(a) => &a.name,
        }
    }

    /// Kind tag for `match`-free dispatch on the variant.
    #[must_use]
    pub fn kind(&self) -> AttachmentType {
        match self {
            Attachment::Region(_) => AttachmentType::Region,
            Attachment::Mesh(_) => AttachmentType::Mesh,
            Attachment::BoundingBox(_) => AttachmentType::BoundingBox,
            Attachment::Path(_) => AttachmentType::Path,
            Attachment::Point(_) => AttachmentType::Point,
            Attachment::Clipping(_) => AttachmentType::Clipping,
        }
    }
}

// --- VertexData (shared by Mesh / BoundingBox / Path / Clipping) -----------

/// Shared vertex storage layout for vertex-based attachments.
///
/// Mirrors spine-cpp's `VertexAttachment` fields. The encoding in `bones`
/// and `vertices` follows the same convention Spine uses for mesh deform:
///
/// - If `bones` is empty, the mesh is not weighted: `vertices` stores
///   `2 * N` floats as `(x, y)` pairs in local space.
/// - Otherwise the mesh is weighted. For each vertex, `bones` contains a
///   count `k` followed by `k` bone indices, and `vertices` contains `k`
///   tuples of `(bone-local x, bone-local y, weight)` — 4 floats per weight
///   entry when colocated.
///
/// This encoding is preserved on the wire because the animation `Deform`
/// timeline blends raw vertex arrays, and decoding would complicate that.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VertexData {
    /// When empty: unweighted vertices. When non-empty: flattened
    /// run-length-encoded `(count, bone_index, …)` stream.
    pub bones: Vec<i32>,
    /// Interleaved f32 values; interpretation depends on `bones` being
    /// empty vs weighted.
    pub vertices: Vec<f32>,
    /// Total number of `f32` values emitted by
    /// `computeWorldVertices`, which is always `2 * vertex_count`.
    pub world_vertices_length: u32,
    /// Link to another attachment that provides timeline data (used by
    /// linked meshes to share the parent mesh's deform keyframes).
    pub timeline_attachment: Option<AttachmentId>,
}

// --- Sequence (4.2) ---------------------------------------------------------

/// Per-sequence cycling mode. Determines which frame is displayed when an
/// animation's progress exceeds the sequence length.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SequenceMode {
    #[default]
    Hold,
    Once,
    Loop,
    PingPong,
    OnceReverse,
    LoopReverse,
    PingPongReverse,
}

/// Describes a numbered sequence of atlas regions driven by a `SequenceTimeline`.
///
/// When a region or mesh attachment carries a `Sequence`, the renderer
/// swaps between atlas regions whose filenames follow a zero-padded numeric
/// naming pattern — `start`, `start+1`, …, `start+count-1`, each written
/// with `digits` digits.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// Unique ID assigned at load (monotonic counter; matches spine-cpp's
    /// `_id`).
    pub id: i32,
    /// First frame number in the sequence. Usually 0.
    pub start: i32,
    /// Number of zero-padded digits in frame filenames.
    pub digits: i32,
    /// Frame index shown in the setup pose.
    pub setup_index: i32,
    /// Number of frames in the sequence.
    pub count: i32,
    /// Resolved atlas regions, one per frame. Populated by the
    /// `AttachmentLoader` during skeleton load. Frames where the region
    /// couldn't be found are `None`.
    pub regions: Vec<Option<TextureRegionRef>>,
}

impl Sequence {
    #[must_use]
    pub fn new(count: i32) -> Self {
        Self {
            id: 0,
            start: 0,
            digits: 0,
            setup_index: 0,
            count,
            regions: Vec::new(),
        }
    }

    /// Format the atlas region name for frame `index` in this sequence —
    /// matches `spine-cpp/Sequence::getPath`.
    ///
    /// Returns `base_path` + `(start + index)` as a decimal, left-padded
    /// with zeros to at least `digits` characters.
    #[must_use]
    pub fn frame_path(&self, base_path: &str, index: i32) -> String {
        let frame = (self.start + index).to_string();
        let digits = self.digits.max(0) as usize;
        let pad = digits.saturating_sub(frame.len());
        let mut out = String::with_capacity(base_path.len() + pad + frame.len());
        out.push_str(base_path);
        for _ in 0..pad {
            out.push('0');
        }
        out.push_str(&frame);
        out
    }
}

// --- RegionAttachment -------------------------------------------------------

/// Rectangular textured quad pinned to a slot. Four corner positions are
/// derived from `x/y/rotation/scale/width/height` and written into
/// `offsets[0..8]` during `update_region`.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionAttachment {
    pub name: String,
    /// Atlas region name (often equal to `name`, but can be overridden e.g.
    /// when a skin replaces the default region).
    pub path: String,
    pub color: Color,

    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub width: f32,
    pub height: f32,

    /// World-space corner offsets, recomputed by `update_region`. Layout:
    /// BLX, BLY, ULX, ULY, URX, URY, BRX, BRY.
    pub vertex_offset: [f32; 8],
    /// Per-vertex UVs in atlas space. Recomputed when the attached region
    /// changes rotation / bounds.
    pub uvs: [f32; 8],

    /// Optional sequence-driven frame cycling.
    pub sequence: Option<Sequence>,

    /// Resolved atlas region reference.
    pub region: Option<TextureRegionRef>,
}

impl RegionAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: String::new(),
            color: Color::WHITE,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            width: 0.0,
            height: 0.0,
            vertex_offset: [0.0; 8],
            uvs: [0.0; 8],
            sequence: None,
            region: None,
        }
    }
}

// --- MeshAttachment ---------------------------------------------------------

/// Triangle-mesh attachment. Vertices are either local-space xy pairs (when
/// unweighted) or weighted bone-space offsets (see [`VertexData`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshAttachment {
    pub name: String,
    pub path: String,
    pub color: Color,

    pub vertex_data: VertexData,

    /// Per-vertex UVs as stored in the mesh (before remapping into atlas
    /// space to account for region rotation or atlas packing).
    pub region_uvs: Vec<f32>,
    /// Per-vertex UVs remapped into atlas space. Recomputed whenever the
    /// resolved region changes.
    pub uvs: Vec<f32>,

    pub triangles: Vec<u16>,
    pub hull_length: u32,

    // Non-essential data (only present when nonessential flag was set on export).
    pub edges: Vec<u16>,
    pub width: f32,
    pub height: f32,

    /// If set, this mesh reuses another mesh's vertex data. Spine's linked-
    /// mesh feature lets skins swap textures without duplicating geometry.
    pub parent_mesh: Option<AttachmentId>,

    pub sequence: Option<Sequence>,
    pub region: Option<TextureRegionRef>,
}

impl MeshAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: String::new(),
            color: Color::WHITE,
            vertex_data: VertexData::default(),
            region_uvs: Vec::new(),
            uvs: Vec::new(),
            triangles: Vec::new(),
            hull_length: 0,
            edges: Vec::new(),
            width: 0.0,
            height: 0.0,
            parent_mesh: None,
            sequence: None,
            region: None,
        }
    }

    /// Recompute atlas-space [`Self::uvs`] from the attachment's
    /// mesh-local [`Self::region_uvs`] and its currently resolved
    /// [`Self::region`]. Literal port of
    /// `spine-cpp/src/spine/MeshAttachment.cpp::updateRegion` — handles
    /// the four `degrees` cases (0 / 90 / 180 / 270) and the
    /// atlas-packing offset/crop math exactly.
    ///
    /// Called at load time (so stored `uvs` are valid for the initial
    /// region) and will be called again by the renderer when a
    /// `Sequence` cycles the active region (Phase 6d/f).
    pub fn update_region(&mut self) {
        if self.uvs.len() != self.region_uvs.len() {
            self.uvs.resize(self.region_uvs.len(), 0.0);
        }
        let Some(region) = self.region.as_ref() else {
            return;
        };

        let n = self.region_uvs.len();
        let u = region.u;
        let v = region.v;

        match region.degrees {
            90 => {
                let texture_width = region.height / (region.u2 - region.u);
                let texture_height = region.width / (region.v2 - region.v);
                let u = u
                    - (region.original_height - region.offset_y - region.height) / texture_width;
                let v = v
                    - (region.original_width - region.offset_x - region.width) / texture_height;
                let width = region.original_height / texture_width;
                let height = region.original_width / texture_height;
                let mut i = 0;
                while i < n {
                    self.uvs[i] = u + self.region_uvs[i + 1] * width;
                    self.uvs[i + 1] = v + (1.0 - self.region_uvs[i]) * height;
                    i += 2;
                }
            }
            180 => {
                let texture_width = region.width / (region.u2 - region.u);
                let texture_height = region.height / (region.v2 - region.v);
                let u = u - (region.original_width - region.offset_x - region.width) / texture_width;
                let v = v - region.offset_y / texture_height;
                let width = region.original_width / texture_width;
                let height = region.original_height / texture_height;
                let mut i = 0;
                while i < n {
                    self.uvs[i] = u + (1.0 - self.region_uvs[i]) * width;
                    self.uvs[i + 1] = v + (1.0 - self.region_uvs[i + 1]) * height;
                    i += 2;
                }
            }
            270 => {
                let texture_height = region.height / (region.v2 - region.v);
                let texture_width = region.width / (region.u2 - region.u);
                let u = u - region.offset_y / texture_width;
                let v = v - region.offset_x / texture_height;
                let width = region.original_height / texture_width;
                let height = region.original_width / texture_height;
                let mut i = 0;
                while i < n {
                    self.uvs[i] = u + (1.0 - self.region_uvs[i + 1]) * width;
                    self.uvs[i + 1] = v + self.region_uvs[i] * height;
                    i += 2;
                }
            }
            _ => {
                let texture_width = region.width / (region.u2 - region.u);
                let texture_height = region.height / (region.v2 - region.v);
                let u = u - region.offset_x / texture_width;
                let v = v
                    - (region.original_height - region.offset_y - region.height) / texture_height;
                let width = region.original_width / texture_width;
                let height = region.original_height / texture_height;
                let mut i = 0;
                while i < n {
                    self.uvs[i] = u + self.region_uvs[i] * width;
                    self.uvs[i + 1] = v + self.region_uvs[i + 1] * height;
                    i += 2;
                }
            }
        }
    }
}

// --- BoundingBoxAttachment --------------------------------------------------

/// Non-rendered polygon used for hit-testing or gameplay queries.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBoxAttachment {
    pub name: String,
    pub vertex_data: VertexData,
    pub color: Color,
}

impl BoundingBoxAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vertex_data: VertexData::default(),
            color: Color::WHITE,
        }
    }
}

// --- PathAttachment ---------------------------------------------------------

/// Cubic bezier path used as a target by path constraints.
#[derive(Debug, Clone, PartialEq)]
pub struct PathAttachment {
    pub name: String,
    pub vertex_data: VertexData,
    pub color: Color,

    /// True when the last cubic segment loops back to the first control
    /// point.
    pub closed: bool,
    /// When true, constrained bones advance at a constant speed along the
    /// path regardless of curvature.
    pub constant_speed: bool,
    /// Per-cubic-segment arc lengths (one value per cubic).
    pub lengths: Vec<f32>,
}

impl PathAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vertex_data: VertexData::default(),
            color: Color::WHITE,
            closed: false,
            constant_speed: true,
            lengths: Vec::new(),
        }
    }
}

// --- PointAttachment --------------------------------------------------------

/// Single point + rotation, useful for spawning effects or marking hitboxes.
#[derive(Debug, Clone, PartialEq)]
pub struct PointAttachment {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub color: Color,
}

impl PointAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            color: Color::WHITE,
        }
    }
}

// --- ClippingAttachment -----------------------------------------------------

/// Polygonal mask. While a clipping attachment is active on its slot, all
/// subsequent slots in draw order are clipped against the polygon until the
/// `end_slot` is reached.
#[derive(Debug, Clone, PartialEq)]
pub struct ClippingAttachment {
    pub name: String,
    pub vertex_data: VertexData,
    pub color: Color,
    /// Slot whose rendering ends the clipping scope. When rendering reaches
    /// this slot, the clip is popped.
    pub end_slot: crate::data::SlotId,
}

impl ClippingAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>, end_slot: crate::data::SlotId) -> Self {
        Self {
            name: name.into(),
            vertex_data: VertexData::default(),
            color: Color::WHITE,
            end_slot,
        }
    }
}

// --- Resolved texture region reference -------------------------------------

/// Atlas region metadata copied out of [`crate::atlas::AtlasRegion`] during
/// attachment loading. We copy rather than reference because skeleton data
/// and atlas data often have different lifetimes and we want
/// [`SkeletonData`][crate::data::SkeletonData] to be self-contained after
/// load.
///
/// Bevy integration resolves `page_index` to a `Handle<Image>` via a parallel
/// atlas-managed asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRegionRef {
    pub page_index: u32,
    pub u: f32,
    pub v: f32,
    pub u2: f32,
    pub v2: f32,
    pub width: f32,
    pub height: f32,
    pub original_width: f32,
    pub original_height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub degrees: i32,
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Literal default comparisons only.
mod tests {
    use super::*;
    use crate::data::SlotId;

    #[test]
    fn enum_dispatch_by_kind() {
        let a = Attachment::Region(RegionAttachment::new("foo"));
        assert_eq!(a.kind(), AttachmentType::Region);
        assert_eq!(a.name(), "foo");

        let m = Attachment::Mesh(MeshAttachment::new("mesh"));
        assert_eq!(m.kind(), AttachmentType::Mesh);

        let c = Attachment::Clipping(ClippingAttachment::new("clip", SlotId(5)));
        assert_eq!(c.kind(), AttachmentType::Clipping);
    }

    #[test]
    fn region_defaults_unit_scale() {
        let r = RegionAttachment::new("x");
        assert_eq!(r.scale_x, 1.0);
        assert_eq!(r.scale_y, 1.0);
        assert_eq!(r.color, Color::WHITE);
        assert!(r.region.is_none());
    }

    #[test]
    fn mesh_defaults_empty_vertex_data() {
        let m = MeshAttachment::new("m");
        assert!(m.vertex_data.bones.is_empty());
        assert!(m.vertex_data.vertices.is_empty());
        assert!(m.triangles.is_empty());
        assert!(m.parent_mesh.is_none());
    }

    #[test]
    fn sequence_defaults() {
        let s = Sequence::new(8);
        assert_eq!(s.count, 8);
        assert_eq!(s.setup_index, 0);
        assert_eq!(s.start, 0);
        assert_eq!(s.digits, 0);
        assert_eq!(SequenceMode::default(), SequenceMode::Hold);
    }
}
