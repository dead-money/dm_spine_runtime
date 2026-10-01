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

//! [`SkeletonData`], the loaded form of one Spine skeleton.

use std::sync::Arc;

use crate::data::{
    Animation, Attachment, AttachmentId, AttachmentRef, BoneData, ConstraintData, ConstraintId,
    EventData, Skin, SkinId, SkinKeys, SlotData, SlotId, Timeline,
};

/// The setup pose and all stateless data needed to animate a skeleton.
///
/// Load once, wrap in an `Arc`, and share across `Skeleton`s. Fields are
/// public; the loaders establish the invariants, nothing enforces them
/// afterwards.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkeletonData {
    pub name: String,
    pub version: String,
    pub hash: String,

    /// Parents precede children; the root bone is first.
    pub bones: Vec<BoneData>,
    /// Slots in setup-pose draw order.
    pub slots: Vec<SlotData>,
    /// Every skin, including the default skin. Shared so skeletons can wear
    /// them as is.
    pub skins: Vec<Arc<Skin>>,
    /// Every `(slot, placeholder)` pair the skins, setup pose and attachment
    /// timelines mention.
    pub skin_keys: SkinKeys,
    /// Index of the default skin in [`Self::skins`], if any.
    pub default_skin: Option<SkinId>,
    pub events: Vec<EventData>,
    pub animations: Vec<Animation>,

    /// Every constraint in update order.
    pub constraints: Vec<ConstraintData>,

    /// Every data [`Attachment`], indexed by [`AttachmentId`].
    pub attachments: Vec<Attachment>,

    /// Setup-pose bounding box, in skeleton space.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Skeleton units per meter, used by physics. Multiplied by the load
    /// scale; JSON defaults it to 100.
    pub reference_scale: f32,

    // Nonessential: populated only when exported with nonessential data.
    pub fps: f32,
    pub images_path: String,
    pub audio_path: String,
}

impl SkeletonData {
    /// Linear scan by name, as are the other `find_*` methods. Cache the
    /// result when calling repeatedly.
    #[must_use]
    pub fn find_bone(&self, name: &str) -> Option<&BoneData> {
        self.bones.iter().find(|b| b.name == name)
    }

    #[must_use]
    pub fn find_slot(&self, name: &str) -> Option<&SlotData> {
        self.slots.iter().find(|s| s.name == name)
    }

    #[must_use]
    pub fn find_skin(&self, name: &str) -> Option<&Arc<Skin>> {
        self.skins.iter().find(|s| s.name == name)
    }

    #[must_use]
    pub fn find_event(&self, name: &str) -> Option<&EventData> {
        self.events.iter().find(|e| e.name == name)
    }

    #[must_use]
    pub fn find_animation(&self, name: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.name == name)
    }

    #[must_use]
    pub fn find_constraint(&self, name: &str) -> Option<ConstraintId> {
        self.constraints
            .iter()
            .position(|c| c.name() == name)
            .map(|i| ConstraintId(i as u16))
    }

    /// The data attachment `skin` holds for a placeholder name. `None` if
    /// absent or owned by the skin.
    ///
    /// # Panics
    ///
    /// If `skin` is out of range.
    #[must_use]
    pub fn skin_attachment(
        &self,
        skin: SkinId,
        slot: SlotId,
        placeholder: &str,
    ) -> Option<AttachmentId> {
        match self.skins[skin.index()].get_named(&self.skin_keys, slot, placeholder)? {
            AttachmentRef::Data(id) => Some(id),
            AttachmentRef::Owned(_) => None,
        }
    }

    /// Interns setup attachment names and attachment timeline names. Loaders
    /// call it once the skins and animations are in.
    pub fn intern_attachment_keys(&mut self) {
        let keys = &mut self.skin_keys;
        let mut intern = |slot: SlotId, name: Option<&str>| {
            name.filter(|n| !n.is_empty()).map(|n| keys.intern(slot, n))
        };
        for slot in &mut self.slots {
            slot.attachment_key = intern(slot.index, slot.attachment_name.as_deref());
        }
        for animation in &mut self.animations {
            for timeline in &mut animation.timelines {
                if let Timeline::Attachment {
                    slot, names, keys, ..
                } = timeline
                {
                    *keys = names.iter().map(|n| intern(*slot, n.as_deref())).collect();
                }
            }
        }
    }

    /// The default skin, if any.
    #[must_use]
    pub fn default_skin(&self) -> Option<&Arc<Skin>> {
        self.default_skin.and_then(|id| self.skins.get(id.index()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BoneData, BoneId, EventData, EventId, SlotData, SlotId};

    fn sample_skeleton() -> SkeletonData {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "body", Some(BoneId(0))));
        sd.slots.push(SlotData::new(SlotId(0), "body", BoneId(1)));
        sd.events.push(EventData::new(EventId(0), "footstep"));
        sd.animations.push(Animation::new("walk", 1.0));
        sd.skins.push(Arc::new(Skin::new("default")));
        sd.default_skin = Some(SkinId(0));
        sd
    }

    #[test]
    fn find_bone_hits_and_misses() {
        let sd = sample_skeleton();
        assert_eq!(sd.find_bone("root").unwrap().index, BoneId(0));
        assert_eq!(sd.find_bone("body").unwrap().index, BoneId(1));
        assert!(sd.find_bone("missing").is_none());
    }

    #[test]
    fn find_by_name_across_collections() {
        let sd = sample_skeleton();
        assert!(sd.find_slot("body").is_some());
        assert!(sd.find_event("footstep").is_some());
        assert!(sd.find_animation("walk").is_some());
        assert!(sd.find_skin("default").is_some());
        assert!(sd.find_constraint("anything").is_none());
    }

    #[test]
    fn default_skin_round_trips() {
        let sd = sample_skeleton();
        assert_eq!(sd.default_skin().unwrap().name, "default");
    }
}
