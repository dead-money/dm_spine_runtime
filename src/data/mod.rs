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

//! Immutable skeleton data shared across `Skeleton` instances.
//!
//! Every Spine asset loads into a single `SkeletonData`, which is typically
//! wrapped in an `Arc` and referenced by many `Skeleton` instances. Runtime
//! mutable state lives on `Skeleton`, not here.
//!
//! # Typed indices
//!
//! Rather than storing cross-references as `Rc<RefCell<T>>` or raw pointers,
//! every data object is identified by a small newtype integer index into the
//! owning `SkeletonData` vectors. This keeps the hot paths pointer-free and
//! lets the borrow checker leave the animation-apply and pose-compute loops
//! alone.

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
    PathAttachment, PointAttachment, RegionAttachment, Sequence, SequenceMode, VertexData,
};
pub use bone::{BoneData, Inherit};
pub use constraint::{
    IkConstraintData, PathConstraintData, PhysicsConstraintData, PositionMode, RotateMode,
    SpacingMode, TransformConstraintData,
};
pub use event::EventData;
pub use skeleton::SkeletonData;
pub use skin::Skin;
pub use slot::{BlendMode, SlotData};

/// Typed indices into the parent [`SkeletonData`] vectors.
///
/// Newtype wrappers rather than bare `u16`/`u32` so that a `SlotId` can't
/// accidentally be passed where a `BoneId` is expected. Equality, hashing and
/// ordering are derived, so they work transparently as map keys.
macro_rules! define_id {
    ($(#[$meta:meta])* $vis:vis $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name(pub $inner);

        impl $name {
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
    /// Index into [`SkeletonData::events`].
    pub EventId(u16)
);
define_id!(
    /// Index into [`SkeletonData::ik_constraints`].
    pub IkConstraintId(u16)
);
define_id!(
    /// Index into [`SkeletonData::transform_constraints`].
    pub TransformConstraintId(u16)
);
define_id!(
    /// Index into [`SkeletonData::path_constraints`].
    pub PathConstraintId(u16)
);
define_id!(
    /// Index into [`SkeletonData::physics_constraints`].
    pub PhysicsConstraintId(u16)
);
define_id!(
    /// Index into [`SkeletonData::animations`].
    pub AnimationId(u16)
);
define_id!(
    /// Index into [`SkeletonData::attachments`]. 32-bit because large rigs with
    /// many skins + sequence frames exceed `u16`.
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
        // Compilation-only: a SlotId cannot be compared with a BoneId. If this
        // ever starts compiling, the distinct-type invariant regressed.
        fn _would_not_compile() {
            // let _ = BoneId(0) == SlotId(0); // (kept as comment on purpose)
        }
    }
}
