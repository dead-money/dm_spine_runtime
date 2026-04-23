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

//! Named collection of attachments plus skin-specific bone / constraint
//! membership. A skeleton applies one skin at a time (Spine supports merging
//! multiple skins into a "virtual" skin at runtime — that composition will
//! happen on the `Skeleton` side in Phase 2).

use std::collections::HashMap;

use crate::data::{
    AttachmentId, BoneId, IkConstraintId, PathConstraintId, PhysicsConstraintId, SlotId,
    TransformConstraintId,
};

/// Setup-pose skin.
///
/// Maintains a `(slot_index, attachment_name) -> AttachmentId` lookup so the
/// runtime can resolve which attachment is active on a given slot when the
/// skin is applied.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Skin {
    pub name: String,

    /// Bones that this skin contributes to the skeleton. Bones with
    /// `skin_required = true` are only included in `updateCache` when a skin
    /// that lists them is active.
    pub bones: Vec<BoneId>,

    /// Constraints brought in by this skin. Mirrors spine-cpp's flat
    /// `Vector<ConstraintData*>`; we split by constraint kind to keep the
    /// struct-of-arrays layout intact.
    pub ik_constraints: Vec<IkConstraintId>,
    pub transform_constraints: Vec<TransformConstraintId>,
    pub path_constraints: Vec<PathConstraintId>,
    pub physics_constraints: Vec<PhysicsConstraintId>,

    /// `(slot_index, attachment_name) -> attachment_id` map.
    ///
    /// spine-cpp nests this as `slotIndex -> (name -> Attachment*)`. A flat
    /// hash of tuple keys is simpler in Rust and equivalent for the lookup
    /// patterns we care about (always a two-part key).
    attachments: HashMap<AttachmentKey, AttachmentId>,
}

/// Key for [`Skin::attachments`]. Wrapped in a struct so the hash map has a
/// single typed key rather than a tuple; makes debug output clearer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AttachmentKey {
    slot: SlotId,
    name: String,
}

impl Skin {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Record that `attachment` is named `name` on `slot` for this skin.
    pub fn set_attachment(
        &mut self,
        slot: SlotId,
        name: impl Into<String>,
        attachment: AttachmentId,
    ) {
        self.attachments.insert(
            AttachmentKey {
                slot,
                name: name.into(),
            },
            attachment,
        );
    }

    /// Look up an attachment by slot index and name.
    #[must_use]
    pub fn get_attachment(&self, slot: SlotId, name: &str) -> Option<AttachmentId> {
        // Avoid allocating a `String` for the lookup by using a trait object
        // key type. Simpler form: build a temporary key. The HashMap has a
        // `get` that would take `&dyn KeyTrait`, but String-key lookup in
        // stable Rust requires constructing the key. Skin sets are small so
        // the extra alloc is fine.
        self.attachments
            .get(&AttachmentKey {
                slot,
                name: name.to_string(),
            })
            .copied()
    }

    /// Number of attachment entries in this skin.
    #[must_use]
    pub fn attachment_count(&self) -> usize {
        self.attachments.len()
    }

    /// Iterate all attachment entries: `((slot, name), attachment_id)`.
    pub fn attachments(&self) -> impl Iterator<Item = (SlotId, &str, AttachmentId)> + '_ {
        self.attachments
            .iter()
            .map(|(k, v)| (k.slot, k.name.as_str(), *v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_set_get_round_trip() {
        let mut skin = Skin::new("default");
        skin.set_attachment(SlotId(0), "body", AttachmentId(7));
        skin.set_attachment(SlotId(1), "head", AttachmentId(8));
        skin.set_attachment(SlotId(0), "body-alt", AttachmentId(9));

        assert_eq!(
            skin.get_attachment(SlotId(0), "body"),
            Some(AttachmentId(7))
        );
        assert_eq!(
            skin.get_attachment(SlotId(1), "head"),
            Some(AttachmentId(8))
        );
        assert_eq!(
            skin.get_attachment(SlotId(0), "body-alt"),
            Some(AttachmentId(9))
        );
        assert_eq!(skin.get_attachment(SlotId(0), "missing"), None);
        assert_eq!(skin.get_attachment(SlotId(2), "body"), None);
        assert_eq!(skin.attachment_count(), 3);
    }

    #[test]
    fn duplicate_key_overwrites() {
        let mut skin = Skin::new("default");
        skin.set_attachment(SlotId(0), "body", AttachmentId(1));
        skin.set_attachment(SlotId(0), "body", AttachmentId(2));
        assert_eq!(
            skin.get_attachment(SlotId(0), "body"),
            Some(AttachmentId(2))
        );
        assert_eq!(skin.attachment_count(), 1);
    }
}
