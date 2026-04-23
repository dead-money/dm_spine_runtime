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

//! Pluggable attachment construction during skeleton load.
//!
//! Ported from `spine-cpp/AttachmentLoader.h` and
//! `spine-cpp/AtlasAttachmentLoader.cpp`. The binary loader (Phase 1c) hands
//! each attachment description to the loader, which returns the runtime
//! [`Attachment`][crate::data::Attachment] value with region metadata
//! already resolved. Swapping the loader is how callers integrate with
//! custom atlas formats, on-demand texture streaming, or mock loaders for
//! unit tests.

use thiserror::Error;

use crate::atlas::Atlas;
use crate::data::SlotId;
use crate::data::attachment::{
    Attachment, BoundingBoxAttachment, ClippingAttachment, MeshAttachment, PathAttachment,
    PointAttachment, RegionAttachment, TextureRegionRef,
};

/// Errors surfaced by an [`AttachmentLoader`]. Loaders can define richer
/// error types of their own; the binary loader wraps these variants into
/// its top-level error enum in Phase 1c.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttachmentLoaderError {
    #[error("atlas region not found: {path:?} (slot {slot:?}, attachment {attachment:?})")]
    RegionNotFound {
        path: String,
        slot: String,
        attachment: String,
    },

    /// Returned by loaders that want to signal skip-on-missing behaviour
    /// rather than hard failure.
    #[error("attachment {attachment:?} on slot {slot:?} is unsupported by this loader")]
    Unsupported { slot: String, attachment: String },
}

/// Pluggable interface for turning attachment descriptions into runtime
/// [`Attachment`] values.
///
/// Spine exports do not inline atlas data into the skeleton; they reference
/// regions by name, and the loader is responsible for resolving those names
/// when the binary file is parsed. The default [`AtlasAttachmentLoader`]
/// looks names up in an [`Atlas`], but callers can provide alternatives
/// (a second atlas for hot-swapping skins, a deferred loader that records
/// names and resolves later, etc).
///
/// Loader methods take `&str` rather than `String` so that callers reading
/// from the binary stream don't need to allocate for the lookup.
pub trait AttachmentLoader {
    /// Build a region attachment. `path` defaults to `name` when the
    /// skeleton didn't override it.
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::RegionNotFound`] if `path` cannot be
    /// resolved against the loader's atlas (or other backing store), or
    /// [`AttachmentLoaderError::Unsupported`] if the loader explicitly
    /// declines this attachment type.
    fn new_region_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a mesh attachment with resolved region UVs. The caller fills
    /// in vertex / triangle / uv data afterwards.
    ///
    /// # Errors
    /// Same conditions as [`Self::new_region_attachment`].
    fn new_mesh_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a bounding-box attachment. Loaders typically return this
    /// unchanged — bounding boxes carry no texture data.
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::Unsupported`] if the loader declines
    /// this attachment type.
    fn new_bounding_box_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a path attachment (cubic bezier target for path constraints).
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::Unsupported`] if the loader declines
    /// this attachment type.
    fn new_path_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a point attachment (single oriented point).
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::Unsupported`] if the loader declines
    /// this attachment type.
    fn new_point_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a clipping attachment. `end_slot` is the slot index where the
    /// clip region ends — clipping scope is bounded by two slots.
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::Unsupported`] if the loader declines
    /// this attachment type.
    fn new_clipping_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        end_slot: SlotId,
    ) -> Result<Attachment, AttachmentLoaderError>;
}

/// Default [`AttachmentLoader`] that resolves region paths against a
/// provided [`Atlas`]. Wraps the atlas by reference to avoid committing to a
/// particular ownership pattern at the loader level — the binary loader can
/// borrow an atlas it doesn't own.
pub struct AtlasAttachmentLoader<'atlas> {
    atlas: &'atlas Atlas,
}

impl<'atlas> AtlasAttachmentLoader<'atlas> {
    #[must_use]
    pub fn new(atlas: &'atlas Atlas) -> Self {
        Self { atlas }
    }

    /// Look up `path` in the backing atlas and build a [`TextureRegionRef`]
    /// snapshot — or fail with a diagnostic including the slot + attachment
    /// that requested the region.
    fn resolve_region(
        &self,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
    ) -> Result<TextureRegionRef, AttachmentLoaderError> {
        let r =
            self.atlas
                .find_region(path)
                .ok_or_else(|| AttachmentLoaderError::RegionNotFound {
                    path: path.to_string(),
                    slot: slot_name.to_string(),
                    attachment: attachment_name.to_string(),
                })?;
        let page = &self.atlas.pages[r.page as usize];
        Ok(TextureRegionRef {
            page_index: page.index,
            u: r.u,
            v: r.v,
            u2: r.u2,
            v2: r.v2,
            width: r.width as f32,
            height: r.height as f32,
            original_width: r.original_width as f32,
            original_height: r.original_height as f32,
            offset_x: r.offset_x,
            offset_y: r.offset_y,
            degrees: r.degrees,
        })
    }
}

impl AttachmentLoader for AtlasAttachmentLoader<'_> {
    fn new_region_attachment(
        &mut self,
        _skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
    ) -> Result<Attachment, AttachmentLoaderError> {
        let region = self.resolve_region(slot_name, attachment_name, path)?;
        let mut r = RegionAttachment::new(attachment_name);
        r.path = path.to_string();
        r.region = Some(region);
        Ok(Attachment::Region(r))
    }

    fn new_mesh_attachment(
        &mut self,
        _skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
    ) -> Result<Attachment, AttachmentLoaderError> {
        let region = self.resolve_region(slot_name, attachment_name, path)?;
        let mut m = MeshAttachment::new(attachment_name);
        m.path = path.to_string();
        m.region = Some(region);
        Ok(Attachment::Mesh(m))
    }

    fn new_bounding_box_attachment(
        &mut self,
        _skin_name: &str,
        _slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError> {
        Ok(Attachment::BoundingBox(BoundingBoxAttachment::new(
            attachment_name,
        )))
    }

    fn new_path_attachment(
        &mut self,
        _skin_name: &str,
        _slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError> {
        Ok(Attachment::Path(PathAttachment::new(attachment_name)))
    }

    fn new_point_attachment(
        &mut self,
        _skin_name: &str,
        _slot_name: &str,
        attachment_name: &str,
    ) -> Result<Attachment, AttachmentLoaderError> {
        Ok(Attachment::Point(PointAttachment::new(attachment_name)))
    }

    fn new_clipping_attachment(
        &mut self,
        _skin_name: &str,
        _slot_name: &str,
        attachment_name: &str,
        end_slot: SlotId,
    ) -> Result<Attachment, AttachmentLoaderError> {
        Ok(Attachment::Clipping(ClippingAttachment::new(
            attachment_name,
            end_slot,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_atlas() -> Atlas {
        Atlas::parse(
            "page.png
\tsize: 64, 64
region-a
\tbounds: 0, 0, 32, 32
",
        )
        .unwrap()
    }

    #[test]
    fn region_attachment_resolves_region() {
        let atlas = sample_atlas();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let attachment = loader
            .new_region_attachment("default", "slot", "region-a", "region-a")
            .unwrap();
        let Attachment::Region(r) = attachment else {
            panic!("expected Region");
        };
        assert_eq!(r.name, "region-a");
        assert_eq!(r.path, "region-a");
        let region = r.region.expect("region should be resolved");
        assert_eq!(region.page_index, 0);
        approx::assert_abs_diff_eq!(region.u, 0.0);
        approx::assert_abs_diff_eq!(region.u2, 32.0 / 64.0);
    }

    #[test]
    fn missing_region_surfaces_descriptive_error() {
        let atlas = sample_atlas();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let err = loader
            .new_region_attachment("default", "slot", "missing", "missing")
            .unwrap_err();
        assert!(matches!(err, AttachmentLoaderError::RegionNotFound { .. }));
        if let AttachmentLoaderError::RegionNotFound {
            path,
            slot,
            attachment,
        } = err
        {
            assert_eq!(path, "missing");
            assert_eq!(slot, "slot");
            assert_eq!(attachment, "missing");
        }
    }

    #[test]
    fn non_region_attachments_dont_touch_atlas() {
        // Loader can produce bounding-box / path / point / clipping without
        // requiring any atlas region (they carry no texture data).
        let atlas = Atlas::default();
        let mut loader = AtlasAttachmentLoader::new(&atlas);

        assert!(matches!(
            loader.new_bounding_box_attachment("d", "s", "bb").unwrap(),
            Attachment::BoundingBox(_)
        ));
        assert!(matches!(
            loader.new_path_attachment("d", "s", "p").unwrap(),
            Attachment::Path(_)
        ));
        assert!(matches!(
            loader.new_point_attachment("d", "s", "pt").unwrap(),
            Attachment::Point(_)
        ));
        assert!(matches!(
            loader
                .new_clipping_attachment("d", "s", "c", SlotId(2))
                .unwrap(),
            Attachment::Clipping(_)
        ));
    }
}
