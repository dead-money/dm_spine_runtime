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

//! Setup-pose data shared by every [`Skeleton`] of one asset.
//!
//! A loader in [`crate::load`] reads an export into one [`SkeletonData`],
//! which is then wrapped in an `Arc` and shared. Its parts:
//!
//! - [`BoneData`], [`SlotData`] and [`ConstraintData`]: the setup pose.
//! - [`Skin`]: attachments keyed by slot and placeholder name.
//! - [`Attachment`]: regions, meshes, clipping polygons, paths, points and
//!   bounding boxes.
//! - [`Animation`]: [`Timeline`]s that [`crate::animation`] applies.
//! - [`EventData`]: events that animations fire.
//!
//! Mutable per-instance state lives on [`Skeleton`]. Cross-references are
//! typed indices into the `SkeletonData` vectors ([`BoneId`], [`SlotId`],
//! ...), not pointers.
//!
//! [`Skeleton`]: crate::skeleton::Skeleton

pub mod animation;
pub mod attachment;
pub mod bone;
pub mod constraint;
pub mod event;
pub mod skeleton;
pub mod skin;
pub mod slot;

pub use animation::{Animation, AnimationEvent, CurveFrames, PhysicsProperty, Timeline};
pub use attachment::{
    Attachment, AttachmentType, BoundingBoxAttachment, ClippingAttachment, MeshAttachment,
    PathAttachment, PointAttachment, RegionAttachment, Sequence, SequenceMode, TextureRegionRef,
    TimelineLink, VertexData,
};
pub use bone::{BoneData, BoneLocal, Inherit};
pub use constraint::{
    ConstraintData, FromProperty, IkConstraintData, IkConstraintPose, PathConstraintData,
    PathConstraintPose, PhysicsConstraintData, PhysicsConstraintPose, PositionMode, RotateMode,
    ScaleYMode, SliderData, SliderPose, SliderProperty, SpacingMode, ToProperty,
    TransformConstraintData, TransformConstraintPose, TransformProperty,
};
pub use event::EventData;
pub use skeleton::SkeletonData;
pub use skin::{AttachmentRef, Skin, SkinKeys};
pub use slot::{BlendMode, SlotData};

/// Defines a typed index newtype, so a `SlotId` can't be passed where a
/// `BoneId` is expected.
macro_rules! define_id {
    ($(#[$meta:meta])* $vis:vis $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name(pub $inner);

        impl $name {
            /// The id as a `usize`, for indexing.
            #[inline]
            #[must_use]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl From<$name> for usize {
            #[inline]
            fn from(id: $name) -> usize {
                id.0 as usize
            }
        }
    };
}

define_id!(
    /// Index into [`SkeletonData::bones`].
    pub BoneId(u16)
);
define_id!(
    /// Index into [`SkeletonData::slots`].
    pub SlotId(u16)
);
define_id!(
    /// Index into [`SkeletonData::skins`].
    pub SkinId(u16)
);
define_id!(
    /// Index into [`SkeletonData::skin_keys`]: one `(slot, placeholder)` pair.
    pub SkinKey(u32)
);
define_id!(
    /// Index into [`SkeletonData::events`].
    pub EventId(u16)
);
define_id!(
    /// Index into [`SkeletonData::constraints`], which is also update order.
    pub ConstraintId(u16)
);
define_id!(
    /// Index into [`SkeletonData::animations`].
    pub AnimationId(u16)
);
define_id!(
    /// Index into [`SkeletonData::attachments`]. 32-bit because large rigs
    /// with many skins and sequence frames exceed `u16`.
    pub AttachmentId(u32)
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_convert_to_usize() {
        assert_eq!(BoneId(5).index(), 5);
        assert_eq!(usize::from(SlotId(42)), 42);
    }

    #[test]
    fn ids_are_distinct_types() {
        // `BoneId(0) == SlotId(0)` must not compile; nothing here checks it.
        fn _would_not_compile() {}
    }
}
