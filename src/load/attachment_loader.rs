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
//! `spine-cpp/AtlasAttachmentLoader.cpp`. The binary loader hands each
//! attachment description to the loader, which returns the runtime
//! [`Attachment`][crate::data::Attachment] value with region metadata
//! already resolved. Swapping the loader is how callers integrate with
//! custom atlas formats, on-demand texture streaming, or mock loaders for
//! unit tests.

use thiserror::Error;

use crate::atlas::Atlas;
use crate::data::SlotId;
use crate::data::attachment::{
    Attachment, BoundingBoxAttachment, ClippingAttachment, MeshAttachment, PathAttachment,
    PointAttachment, RegionAttachment, Sequence, TextureRegionRef,
};

/// Errors surfaced by an [`AttachmentLoader`]. Loaders can define richer
/// error types of their own; the binary loader wraps these variants into
/// its top-level error enum.
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
    /// When `sequence` is `Some`, the loader should populate
    /// `sequence.regions` with per-frame atlas regions derived from
    /// `path` (via [`Sequence::frame_path`]) and seed the attachment's
    /// direct `region` with the first resolvable frame, so the initial
    /// setup pose has something to size and sample against before the
    /// sequence cycles. When `sequence` is `None`, the loader resolves
    /// `path` to a single region and stores it on the attachment.
    ///
    /// # Errors
    /// Returns [`AttachmentLoaderError::RegionNotFound`] if any required
    /// region cannot be resolved against the loader's atlas, or
    /// [`AttachmentLoaderError::Unsupported`] if the loader explicitly
    /// declines this attachment type.
    fn new_region_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
        sequence: Option<&mut Sequence>,
    ) -> Result<Attachment, AttachmentLoaderError>;

    /// Build a mesh attachment with resolved region UVs. The caller fills
    /// in vertex / triangle / uv data afterwards. Sequence semantics
    /// match [`Self::new_region_attachment`].
    ///
    /// # Errors
    /// Same conditions as [`Self::new_region_attachment`].
    fn new_mesh_attachment(
        &mut self,
        skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
        sequence: Option<&mut Sequence>,
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

impl AtlasAttachmentLoader<'_> {
    /// Populate a sequence's per-frame regions from the atlas. Every frame
    /// path is formed as `sequence.frame_path(base, i)` and looked up in
    /// the atlas. Missing regions are stored as `None` rather than failing,
    /// matching spine-cpp's permissive fallback (which would `return NULL`
    /// at the skeleton level but allows individual frames to be optional
    /// in practice — this is the same behaviour).
    fn load_sequence(&self, base_path: &str, sequence: &mut Sequence) {
        let frame_count = sequence.count.max(0) as usize;
        sequence.regions.clear();
        sequence.regions.reserve(frame_count);
        for i in 0..frame_count {
            // i is bounded by frame_count which comes from sequence.count (i32),
            // so this fits in i32 by construction.
            let path = sequence.frame_path(base_path, i32::try_from(i).unwrap_or(i32::MAX));
            let resolved = self.atlas.find_region(&path).map(|r| {
                let page = &self.atlas.pages[r.page as usize];
                TextureRegionRef {
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
                }
            });
            sequence.regions.push(resolved);
        }
    }
}

impl AttachmentLoader for AtlasAttachmentLoader<'_> {
    fn new_region_attachment(
        &mut self,
        _skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
        sequence: Option<&mut Sequence>,
    ) -> Result<Attachment, AttachmentLoaderError> {
        let mut r = RegionAttachment::new(attachment_name);
        r.path = path.to_string();
        if let Some(seq) = sequence {
            self.load_sequence(path, seq);
            // Seed the attachment with the first sequence frame so
            // `update_region` has something to size against. The runtime
            // overrides `region` when the sequence cycles at apply time.
            if let Some(Some(first)) = seq.regions.first() {
                r.region = Some(*first);
            }
        } else {
            r.region = Some(self.resolve_region(slot_name, attachment_name, path)?);
        }
        Ok(Attachment::Region(r))
    }

    fn new_mesh_attachment(
        &mut self,
        _skin_name: &str,
        slot_name: &str,
        attachment_name: &str,
        path: &str,
        sequence: Option<&mut Sequence>,
    ) -> Result<Attachment, AttachmentLoaderError> {
        let mut m = MeshAttachment::new(attachment_name);
        m.path = path.to_string();
        if let Some(seq) = sequence {
            self.load_sequence(path, seq);
            // Same seeding logic as new_region_attachment: gives
            // `update_region` / `compute_world_vertices` a reasonable
            // default UV frame for setup pose before the sequence cycles.
            if let Some(Some(first)) = seq.regions.first() {
                m.region = Some(*first);
            }
        } else {
            m.region = Some(self.resolve_region(slot_name, attachment_name, path)?);
        }
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
            .new_region_attachment("default", "slot", "region-a", "region-a", None)
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
            .new_region_attachment("default", "slot", "missing", "missing", None)
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
    fn sequence_populates_frame_regions_and_skips_base_lookup() {
        // Atlas with sequence-style regions. Spine's Sequence::frame_path
        // concatenates `base + zero_padded_frame_number` with no separator,
        // so for base="region" with digits=2 the atlas must contain
        // "region01", "region02" (no dash before the frame number).
        let atlas = Atlas::parse(
            "p.png
\tsize: 64, 64
region01
\tbounds: 0, 0, 16, 16
region02
\tbounds: 16, 0, 16, 16
",
        )
        .unwrap();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let mut sequence = Sequence::new(2);
        sequence.start = 1;
        sequence.digits = 2;
        sequence.setup_index = 0;

        // Pass the sequence in; the loader should populate its `regions`
        // AND seed the attachment's `region` with the first frame so
        // `update_region` has something to size against.
        let attachment = loader
            .new_region_attachment("default", "slot", "region", "region", Some(&mut sequence))
            .unwrap();
        let Attachment::Region(r) = attachment else {
            panic!("expected Region");
        };
        assert_eq!(sequence.regions.len(), 2);
        assert!(sequence.regions[0].is_some());
        assert!(sequence.regions[1].is_some());
        assert!(
            r.region.is_some(),
            "base region should be seeded from sequence.regions[0]"
        );
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
