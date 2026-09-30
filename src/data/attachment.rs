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

//! Attachment data. spine-cpp's class hierarchy flattens to a tagged enum;
//! vertex-based variants embed [`VertexData`].

use std::sync::atomic::{AtomicI32, Ordering};

use crate::data::{AttachmentId, SlotId};
use crate::math::Color;

/// Indices into a region's 8-float quad (`offsets` / `uvs`).
pub mod quad_corner {
    pub const BLX: usize = 0;
    pub const BLY: usize = 1;
    pub const ULX: usize = 2;
    pub const ULY: usize = 3;
    pub const URX: usize = 4;
    pub const URY: usize = 5;
    pub const BRX: usize = 6;
    pub const BRY: usize = 7;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentType {
    Region,
    Mesh,
    LinkedMesh,
    BoundingBox,
    Path,
    Point,
    Clipping,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
    Region(RegionAttachment),
    Mesh(MeshAttachment),
    BoundingBox(BoundingBoxAttachment),
    Path(PathAttachment),
    Point(PointAttachment),
    Clipping(ClippingAttachment),
}

/// Which attachment's timelines drive this one, and which other slots those
/// timelines also apply to. Linked meshes that inherit timelines point at
/// their source mesh, possibly in another slot.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimelineLink {
    /// `None` means the attachment itself.
    pub attachment: Option<AttachmentId>,
    pub slots: Vec<SlotId>,
}

impl Attachment {
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

    #[must_use]
    pub fn timeline_link(&self) -> Option<&TimelineLink> {
        match self {
            Attachment::Region(a) => Some(&a.timeline),
            Attachment::Mesh(a) => Some(&a.vertex_data.timeline),
            Attachment::BoundingBox(a) => Some(&a.vertex_data.timeline),
            Attachment::Path(a) => Some(&a.vertex_data.timeline),
            Attachment::Clipping(a) => Some(&a.vertex_data.timeline),
            Attachment::Point(_) => None,
        }
    }

    pub fn timeline_link_mut(&mut self) -> Option<&mut TimelineLink> {
        match self {
            Attachment::Region(a) => Some(&mut a.timeline),
            Attachment::Mesh(a) => Some(&mut a.vertex_data.timeline),
            Attachment::BoundingBox(a) => Some(&mut a.vertex_data.timeline),
            Attachment::Path(a) => Some(&mut a.vertex_data.timeline),
            Attachment::Clipping(a) => Some(&mut a.vertex_data.timeline),
            Attachment::Point(_) => None,
        }
    }

    /// The attachment whose timelines apply to this one; `id` is this
    /// attachment's own id.
    #[must_use]
    pub fn timeline_attachment(&self, id: AttachmentId) -> AttachmentId {
        self.timeline_link()
            .and_then(|t| t.attachment)
            .unwrap_or(id)
    }

    #[must_use]
    pub fn vertex_data(&self) -> Option<&VertexData> {
        match self {
            Attachment::Mesh(a) => Some(&a.vertex_data),
            Attachment::BoundingBox(a) => Some(&a.vertex_data),
            Attachment::Path(a) => Some(&a.vertex_data),
            Attachment::Clipping(a) => Some(&a.vertex_data),
            Attachment::Region(_) | Attachment::Point(_) => None,
        }
    }

    #[must_use]
    pub fn sequence(&self) -> Option<&Sequence> {
        match self {
            Attachment::Region(a) => Some(&a.sequence),
            Attachment::Mesh(a) => Some(&a.sequence),
            _ => None,
        }
    }
}

/// Shared by Mesh, BoundingBox, Path and Clipping.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VertexData {
    /// Empty when unweighted. Otherwise, per vertex: bone count, then that
    /// many bone indices.
    pub bones: Vec<i32>,
    /// Unweighted: x,y pairs. Weighted: x,y,weight triples per bone.
    pub vertices: Vec<f32>,
    pub world_vertices_length: u32,
    pub timeline: TimelineLink,
}

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

impl SequenceMode {
    #[must_use]
    pub fn from_index(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Hold,
            1 => Self::Once,
            2 => Self::Loop,
            3 => Self::PingPong,
            4 => Self::OnceReverse,
            5 => Self::LoopReverse,
            6 => Self::PingPongReverse,
            _ => return None,
        })
    }
}

static NEXT_SEQUENCE_ID: AtomicI32 = AtomicI32::new(0);

/// Frames of a region or mesh attachment. Every region and mesh has one; a
/// plain attachment is a one-frame sequence without a path suffix. Per-frame
/// UVs (and region vertex offsets) are precomputed by `update_sequence`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// Unique per sequence; timelines use it as their property id.
    pub id: i32,
    pub start: i32,
    pub digits: i32,
    pub setup_index: i32,
    pub path_suffix: bool,
    pub regions: Vec<Option<TextureRegionRef>>,
    /// Frame-major; each frame is `uvs.len() / regions.len()` floats.
    uvs: Vec<f32>,
    /// Region attachments only.
    offsets: Vec<[f32; 8]>,
}

impl Sequence {
    #[must_use]
    pub fn new(count: usize, path_suffix: bool) -> Self {
        Self {
            id: NEXT_SEQUENCE_ID.fetch_add(1, Ordering::Relaxed),
            start: 0,
            digits: 0,
            setup_index: 0,
            path_suffix,
            regions: vec![None; count],
            uvs: Vec::new(),
            offsets: Vec::new(),
        }
    }

    /// Copy with a fresh id, matching spine-cpp's copy constructor.
    #[must_use]
    pub fn copy(&self) -> Self {
        Self {
            id: NEXT_SEQUENCE_ID.fetch_add(1, Ordering::Relaxed),
            ..self.clone()
        }
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.regions.len()
    }

    /// The frame to show for a slot's `sequence_index` (-1 means setup).
    #[must_use]
    pub fn resolve_index(&self, sequence_index: i32) -> usize {
        let mut index = sequence_index;
        if index == -1 {
            index = self.setup_index;
        }
        let last = self.regions.len() as i32 - 1;
        if index >= last {
            index = last;
        }
        index.max(0) as usize
    }

    #[must_use]
    pub fn region(&self, index: usize) -> Option<&TextureRegionRef> {
        self.regions.get(index).and_then(Option::as_ref)
    }

    #[must_use]
    pub fn uvs(&self, index: usize) -> &[f32] {
        let stride = self.uvs.len() / self.regions.len().max(1);
        &self.uvs[index * stride..(index + 1) * stride]
    }

    #[must_use]
    pub fn offsets(&self, index: usize) -> &[f32; 8] {
        &self.offsets[index]
    }

    /// Atlas path of frame `index`.
    #[must_use]
    pub fn path(&self, base_path: &str, index: usize) -> String {
        if !self.path_suffix {
            return base_path.to_owned();
        }
        let frame = (self.start + index as i32).to_string();
        let pad = (self.digits.max(0) as usize).saturating_sub(frame.len());
        let mut out = String::with_capacity(base_path.len() + pad + frame.len());
        out.push_str(base_path);
        out.extend(std::iter::repeat_n('0', pad));
        out.push_str(&frame);
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionAttachment {
    pub name: String,
    pub path: String,
    pub color: Color,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub width: f32,
    pub height: f32,
    pub sequence: Sequence,
    pub timeline: TimelineLink,
}

impl RegionAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>, sequence: Sequence) -> Self {
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
            sequence,
            timeline: TimelineLink::default(),
        }
    }

    /// Recomputes every frame's vertex offsets and UVs.
    pub fn update_sequence(&mut self) {
        let n = self.sequence.regions.len();
        self.sequence.uvs.clear();
        self.sequence.uvs.resize(n * 8, 0.0);
        self.sequence.offsets.clear();
        self.sequence.offsets.resize(n, [0.0; 8]);
        for i in 0..n {
            let region = self.sequence.regions[i];
            let mut offsets = [0.0; 8];
            let mut uvs = [0.0; 8];
            compute_region_uvs(region.as_ref(), self, &mut offsets, &mut uvs);
            self.sequence.offsets[i] = offsets;
            self.sequence.uvs[i * 8..i * 8 + 8].copy_from_slice(&uvs);
        }
    }
}

/// `RegionAttachment::computeUVs`.
fn compute_region_uvs(
    region: Option<&TextureRegionRef>,
    a: &RegionAttachment,
    offset: &mut [f32; 8],
    uvs: &mut [f32; 8],
) {
    use quad_corner::{BLX, BLY, BRX, BRY, ULX, ULY, URX, URY};

    let (width, height) = (a.width, a.height);
    let mut local_x2 = width / 2.0;
    let mut local_y2 = height / 2.0;
    let mut local_x = -local_x2;
    let mut local_y = -local_y2;
    let mut rotated = false;
    if let Some(r) = region.filter(|r| r.atlas) {
        local_x += r.offset_x / r.original_width * width;
        local_y += r.offset_y / r.original_height * height;
        if r.degrees == 90 {
            rotated = true;
            local_x2 -=
                (r.original_width - r.offset_x - r.packed_height) / r.original_width * width;
            local_y2 -=
                (r.original_height - r.offset_y - r.packed_width) / r.original_height * height;
        } else {
            local_x2 -= (r.original_width - r.offset_x - r.packed_width) / r.original_width * width;
            local_y2 -=
                (r.original_height - r.offset_y - r.packed_height) / r.original_height * height;
        }
    }
    local_x *= a.scale_x;
    local_y *= a.scale_y;
    local_x2 *= a.scale_x;
    local_y2 *= a.scale_y;
    let rad = a.rotation.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let local_x_cos = local_x * cos + a.x;
    let local_x_sin = local_x * sin;
    let local_y_cos = local_y * cos + a.y;
    let local_y_sin = local_y * sin;
    let local_x2_cos = local_x2 * cos + a.x;
    let local_x2_sin = local_x2 * sin;
    let local_y2_cos = local_y2 * cos + a.y;
    let local_y2_sin = local_y2 * sin;
    offset[BLX] = local_x_cos - local_y_sin;
    offset[BLY] = local_y_cos + local_x_sin;
    offset[ULX] = local_x_cos - local_y2_sin;
    offset[ULY] = local_y2_cos + local_x_sin;
    offset[URX] = local_x2_cos - local_y2_sin;
    offset[URY] = local_y2_cos + local_x2_sin;
    offset[BRX] = local_x2_cos - local_y_sin;
    offset[BRY] = local_y_cos + local_x2_sin;
    match region {
        None => {
            uvs[BLX] = 0.0;
            uvs[BLY] = 0.0;
            uvs[ULX] = 0.0;
            uvs[ULY] = 1.0;
            uvs[URX] = 1.0;
            uvs[URY] = 1.0;
            uvs[BRX] = 1.0;
            uvs[BRY] = 0.0;
        }
        Some(r) => {
            uvs[BLX] = r.u2;
            uvs[ULY] = r.v2;
            uvs[URX] = r.u;
            uvs[BRY] = r.v;
            if rotated {
                uvs[BLY] = r.v;
                uvs[ULX] = r.u2;
                uvs[URY] = r.v2;
                uvs[BRX] = r.u;
            } else {
                uvs[BLY] = r.v2;
                uvs[ULX] = r.u;
                uvs[URY] = r.v;
                uvs[BRX] = r.u2;
            }
        }
    }
}

/// `MeshAttachment::computeUVs`.
fn compute_mesh_uvs(region: Option<&TextureRegionRef>, region_uvs: &[f32], uvs: &mut [f32]) {
    let n = uvs.len();
    let (u, v, width, height);
    match region {
        Some(r) if r.atlas => {
            let mut u0 = r.u;
            let mut v0 = r.v;
            let texture_width = r.page_width;
            let texture_height = r.page_height;
            match r.degrees {
                90 => {
                    u0 -= (r.original_height - r.offset_y - r.packed_width) / texture_width;
                    v0 -= (r.original_width - r.offset_x - r.packed_height) / texture_height;
                    let w = r.original_height / texture_width;
                    let h = r.original_width / texture_height;
                    for i in (0..n).step_by(2) {
                        uvs[i] = u0 + region_uvs[i + 1] * w;
                        uvs[i + 1] = v0 + (1.0 - region_uvs[i]) * h;
                    }
                    return;
                }
                180 => {
                    u0 -= (r.original_width - r.offset_x - r.packed_width) / texture_width;
                    v0 -= r.offset_y / texture_height;
                    let w = r.original_width / texture_width;
                    let h = r.original_height / texture_height;
                    for i in (0..n).step_by(2) {
                        uvs[i] = u0 + (1.0 - region_uvs[i]) * w;
                        uvs[i + 1] = v0 + (1.0 - region_uvs[i + 1]) * h;
                    }
                    return;
                }
                270 => {
                    u0 -= r.offset_y / texture_width;
                    v0 -= r.offset_x / texture_height;
                    let w = r.original_height / texture_width;
                    let h = r.original_width / texture_height;
                    for i in (0..n).step_by(2) {
                        uvs[i] = u0 + (1.0 - region_uvs[i + 1]) * w;
                        uvs[i + 1] = v0 + region_uvs[i] * h;
                    }
                    return;
                }
                _ => {
                    u0 -= r.offset_x / texture_width;
                    v0 -= (r.original_height - r.offset_y - r.packed_height) / texture_height;
                    u = u0;
                    v = v0;
                    width = r.original_width / texture_width;
                    height = r.original_height / texture_height;
                }
            }
        }
        None => {
            u = 0.0;
            v = 0.0;
            width = 1.0;
            height = 1.0;
        }
        Some(r) => {
            u = r.u;
            v = r.v;
            width = r.u2 - u;
            height = r.v2 - v;
        }
    }
    for i in (0..n).step_by(2) {
        uvs[i] = u + region_uvs[i] * width;
        uvs[i + 1] = v + region_uvs[i + 1] * height;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshAttachment {
    pub name: String,
    pub path: String,
    pub color: Color,
    pub vertex_data: VertexData,
    pub region_uvs: Vec<f32>,
    pub triangles: Vec<u16>,
    /// In floats (vertex count × 2).
    pub hull_length: u32,
    pub sequence: Sequence,
    /// Set for linked meshes; geometry is copied from it at link time.
    pub source_mesh: Option<AttachmentId>,

    // Nonessential.
    pub edges: Vec<u16>,
    pub width: f32,
    pub height: f32,
}

impl MeshAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>, sequence: Sequence) -> Self {
        Self {
            name: name.into(),
            path: String::new(),
            color: Color::WHITE,
            vertex_data: VertexData::default(),
            region_uvs: Vec::new(),
            triangles: Vec::new(),
            hull_length: 0,
            sequence,
            source_mesh: None,
            edges: Vec::new(),
            width: 0.0,
            height: 0.0,
        }
    }

    /// Recomputes every frame's UVs from `region_uvs`.
    pub fn update_sequence(&mut self) {
        let n = self.sequence.regions.len();
        let stride = self.region_uvs.len();
        self.sequence.offsets.clear();
        self.sequence.uvs.clear();
        self.sequence.uvs.resize(n * stride, 0.0);
        for i in 0..n {
            compute_mesh_uvs(
                self.sequence.regions[i].as_ref(),
                &self.region_uvs,
                &mut self.sequence.uvs[i * stride..(i + 1) * stride],
            );
        }
    }

    /// Links to `source`, copying its geometry.
    pub fn set_source_mesh(&mut self, id: AttachmentId, source: &MeshAttachment) {
        self.source_mesh = Some(id);
        self.vertex_data.bones.clone_from(&source.vertex_data.bones);
        self.vertex_data
            .vertices
            .clone_from(&source.vertex_data.vertices);
        self.vertex_data.world_vertices_length = source.vertex_data.world_vertices_length;
        self.region_uvs.clone_from(&source.region_uvs);
        self.triangles.clone_from(&source.triangles);
        self.hull_length = source.hull_length;
        self.edges.clone_from(&source.edges);
        self.width = source.width;
        self.height = source.height;
    }
}

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
            color: Color::new(0.38, 0.94, 0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathAttachment {
    pub name: String,
    pub vertex_data: VertexData,
    pub color: Color,
    pub closed: bool,
    pub constant_speed: bool,
    pub lengths: Vec<f32>,
}

impl PathAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vertex_data: VertexData::default(),
            color: Color::new(0.0, 0.0, 0.0, 0.0),
            closed: false,
            constant_speed: false,
            lengths: Vec::new(),
        }
    }
}

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
            color: Color::new(0.9451, 0.9451, 0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClippingAttachment {
    pub name: String,
    pub vertex_data: VertexData,
    pub color: Color,
    pub end_slot: Option<SlotId>,
    pub convex: bool,
    pub inverse: bool,
}

impl ClippingAttachment {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vertex_data: VertexData::default(),
            color: Color::new(0.0, 0.0, 0.0, 0.0),
            end_slot: None,
            convex: false,
            inverse: false,
        }
    }
}

/// A resolved texture region. `atlas` regions carry packing data (offsets,
/// original size, rotation); plain regions map straight to `u..u2, v..v2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRegionRef {
    pub page_index: u32,
    pub atlas: bool,
    pub u: f32,
    pub v: f32,
    pub u2: f32,
    pub v2: f32,
    /// Packed size with width and height swapped for 90° regions, as
    /// spine-cpp's atlas loader leaves them.
    pub packed_width: f32,
    pub packed_height: f32,
    pub original_width: f32,
    pub original_height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub degrees: i32,
    pub page_width: f32,
    pub page_height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_ids_are_unique_and_copy_renews() {
        let a = Sequence::new(1, false);
        let b = Sequence::new(1, false);
        assert_ne!(a.id, b.id);
        assert_ne!(a.copy().id, a.id);
    }

    #[test]
    fn sequence_path_suffix() {
        let mut s = Sequence::new(3, true);
        s.start = 1;
        s.digits = 2;
        assert_eq!(s.path("run", 0), "run01");
        assert_eq!(Sequence::new(1, false).path("run", 0), "run");
    }

    #[test]
    fn resolve_index_uses_setup_and_clamps() {
        let mut s = Sequence::new(3, true);
        s.setup_index = 1;
        assert_eq!(s.resolve_index(-1), 1);
        assert_eq!(s.resolve_index(7), 2);
    }

    #[test]
    fn regionless_mesh_uvs_pass_through() {
        let mut m = MeshAttachment::new("m", Sequence::new(1, false));
        m.region_uvs = vec![0.25, 0.5, 1.0, 0.0];
        m.update_sequence();
        assert_eq!(m.sequence.uvs(0), &[0.25, 0.5, 1.0, 0.0]);
    }
}
