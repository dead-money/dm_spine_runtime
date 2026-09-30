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

//! Pluggable attachment construction during skeleton load
//! (`AttachmentLoader` / `AtlasAttachmentLoader`). Loaders receive the skin
//! placeholder name and the attachment's own name, and may return `None` to
//! leave an attachment out.

use thiserror::Error;

use crate::atlas::{Atlas, AtlasRegion};
use crate::data::attachment::{
    BoundingBoxAttachment, ClippingAttachment, MeshAttachment, PathAttachment, PointAttachment,
    RegionAttachment, Sequence, TextureRegionRef,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttachmentLoaderError {
    #[error("atlas region not found: {path:?} (attachment {attachment:?})")]
    RegionNotFound { path: String, attachment: String },
    #[error("attachment {attachment:?} is unsupported by this loader")]
    Unsupported { attachment: String },
}

/// Builds attachments while a skeleton loads. The loader fills in texture
/// regions; the skeleton reader fills in everything else and then calls
/// `update_sequence`. Returning `Ok(None)` omits the attachment.
pub trait AttachmentLoader {
    /// `sequence` has one entry per frame; resolve each frame's region.
    ///
    /// # Errors
    /// Loader-specific.
    fn new_region_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
        path: &str,
        sequence: Sequence,
    ) -> Result<Option<RegionAttachment>, AttachmentLoaderError>;

    /// # Errors
    /// Loader-specific.
    fn new_mesh_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
        path: &str,
        sequence: Sequence,
    ) -> Result<Option<MeshAttachment>, AttachmentLoaderError>;

    /// # Errors
    /// Loader-specific.
    fn new_bounding_box_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
    ) -> Result<Option<BoundingBoxAttachment>, AttachmentLoaderError> {
        let _ = (skin, placeholder);
        Ok(Some(BoundingBoxAttachment::new(name)))
    }

    /// # Errors
    /// Loader-specific.
    fn new_path_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
    ) -> Result<Option<PathAttachment>, AttachmentLoaderError> {
        let _ = (skin, placeholder);
        Ok(Some(PathAttachment::new(name)))
    }

    /// # Errors
    /// Loader-specific.
    fn new_point_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
    ) -> Result<Option<PointAttachment>, AttachmentLoaderError> {
        let _ = (skin, placeholder);
        Ok(Some(PointAttachment::new(name)))
    }

    /// # Errors
    /// Loader-specific.
    fn new_clipping_attachment(
        &mut self,
        skin: &str,
        placeholder: &str,
        name: &str,
    ) -> Result<Option<ClippingAttachment>, AttachmentLoaderError> {
        let _ = (skin, placeholder);
        Ok(Some(ClippingAttachment::new(name)))
    }
}

/// Resolves regions against an [`Atlas`]. A frame whose region is missing
/// stays `None` and renders with 0..1 UVs, as in spine-cpp.
pub struct AtlasAttachmentLoader<'atlas> {
    atlas: &'atlas Atlas,
}

impl<'atlas> AtlasAttachmentLoader<'atlas> {
    #[must_use]
    pub fn new(atlas: &'atlas Atlas) -> Self {
        Self { atlas }
    }

    fn find_regions(&self, base_path: &str, sequence: &mut Sequence) {
        for i in 0..sequence.regions.len() {
            let path = sequence.path(base_path, i);
            sequence.regions[i] = self
                .atlas
                .find_region(&path)
                .map(|r| region_ref(self.atlas, r));
        }
    }
}

/// Snapshot of an atlas region in the form attachments consume.
#[must_use]
pub fn region_ref(atlas: &Atlas, r: &AtlasRegion) -> TextureRegionRef {
    let page = &atlas.pages[r.page as usize];
    let (packed_width, packed_height) = if r.degrees == 90 {
        (r.height as f32, r.width as f32)
    } else {
        (r.width as f32, r.height as f32)
    };
    TextureRegionRef {
        page_index: page.index,
        atlas: true,
        u: r.u,
        v: r.v,
        u2: r.u2,
        v2: r.v2,
        packed_width,
        packed_height,
        original_width: r.original_width as f32,
        original_height: r.original_height as f32,
        offset_x: r.offset_x,
        offset_y: r.offset_y,
        degrees: r.degrees,
        page_width: page.width as f32,
        page_height: page.height as f32,
    }
}

impl AttachmentLoader for AtlasAttachmentLoader<'_> {
    fn new_region_attachment(
        &mut self,
        _skin: &str,
        _placeholder: &str,
        name: &str,
        path: &str,
        mut sequence: Sequence,
    ) -> Result<Option<RegionAttachment>, AttachmentLoaderError> {
        self.find_regions(path, &mut sequence);
        Ok(Some(RegionAttachment::new(name, sequence)))
    }

    fn new_mesh_attachment(
        &mut self,
        _skin: &str,
        _placeholder: &str,
        name: &str,
        path: &str,
        mut sequence: Sequence,
    ) -> Result<Option<MeshAttachment>, AttachmentLoaderError> {
        self.find_regions(path, &mut sequence);
        Ok(Some(MeshAttachment::new(name, sequence)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATLAS: &str = "\
page.png
size: 64, 32
filter: Linear, Linear
body
bounds: 0, 0, 16, 16
run1
bounds: 16, 0, 16, 16
run2
bounds: 32, 0, 16, 8
rotate: 90
";

    #[test]
    fn resolves_region_and_leaves_missing_frames_empty() {
        let atlas = Atlas::parse(ATLAS).unwrap();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let body = loader
            .new_region_attachment("default", "body", "body", "body", Sequence::new(1, false))
            .unwrap()
            .unwrap();
        assert!(body.sequence.region(0).is_some());

        let mut seq = Sequence::new(3, true);
        seq.start = 1;
        let run = loader
            .new_mesh_attachment("default", "run", "run", "run", seq)
            .unwrap()
            .unwrap();
        assert!(run.sequence.region(0).is_some());
        let r2 = run.sequence.region(1).unwrap();
        assert_eq!((r2.packed_width, r2.packed_height), (8.0, 16.0));
        assert!(run.sequence.region(2).is_none());
    }
}
