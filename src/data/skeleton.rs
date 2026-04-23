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

//! Top-level immutable data container for a single Spine skeleton.

use crate::data::{
    Animation, Attachment, BoneData, EventData, IkConstraintData, PathConstraintData,
    PhysicsConstraintData, Skin, SkinId, SlotData, TransformConstraintData,
};

/// Stores the setup pose and every piece of stateless data the runtime needs
/// to animate a skeleton.
///
/// Intended lifecycle: parse once from a `.skel` file, wrap in an
/// `Arc<SkeletonData>`, hand out to many `Skeleton` instances. All fields
/// are public because this is pure data — invariants are upheld during
/// loading, not enforced on the struct.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkeletonData {
    pub name: String,
    pub version: String,
    pub hash: String,

    /// Bones sorted with parents before children; the root bone is always
    /// first. This ordering is relied on by `Skeleton::update_cache`.
    pub bones: Vec<BoneData>,
    /// Slots in setup-pose draw order.
    pub slots: Vec<SlotData>,
    /// All skins, including the default skin (if present, always at index 0
    /// when the skeleton has one).
    pub skins: Vec<Skin>,
    /// Index of the default skin in [`Self::skins`], or `None` if no default
    /// skin was defined.
    pub default_skin: Option<SkinId>,
    pub events: Vec<EventData>,
    pub animations: Vec<Animation>,

    pub ik_constraints: Vec<IkConstraintData>,
    pub transform_constraints: Vec<TransformConstraintData>,
    pub path_constraints: Vec<PathConstraintData>,
    pub physics_constraints: Vec<PhysicsConstraintData>,

    /// Flat store for every [`Attachment`] referenced by any skin. Skins
    /// hold [`AttachmentId`][crate::data::AttachmentId] indices into this
    /// vector — keeps the struct-of-arrays invariant.
    pub attachments: Vec<Attachment>,

    /// Skeleton-local origin and dimensions (bounding box in setup pose).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Scale factor written by the editor for certain render modes (e.g.
    /// physics simulations tuned for a particular unit scale). Defaults to
    /// 100 in spine-cpp.
    pub reference_scale: f32,

    // Non-essential fields (only populated when non-essential data was
    // exported; safe defaults otherwise).
    pub fps: f32,
    pub images_path: String,
    pub audio_path: String,
}

impl SkeletonData {
    /// Linear-scan lookup by name. Cache the result when called repeatedly.
    #[must_use]
    pub fn find_bone(&self, name: &str) -> Option<&BoneData> {
        self.bones.iter().find(|b| b.name == name)
    }

    #[must_use]
    pub fn find_slot(&self, name: &str) -> Option<&SlotData> {
        self.slots.iter().find(|s| s.name == name)
    }

    #[must_use]
    pub fn find_skin(&self, name: &str) -> Option<&Skin> {
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
    pub fn find_ik_constraint(&self, name: &str) -> Option<&IkConstraintData> {
        self.ik_constraints.iter().find(|c| c.name == name)
    }

    #[must_use]
    pub fn find_transform_constraint(&self, name: &str) -> Option<&TransformConstraintData> {
        self.transform_constraints.iter().find(|c| c.name == name)
    }

    #[must_use]
    pub fn find_path_constraint(&self, name: &str) -> Option<&PathConstraintData> {
        self.path_constraints.iter().find(|c| c.name == name)
    }

    #[must_use]
    pub fn find_physics_constraint(&self, name: &str) -> Option<&PhysicsConstraintData> {
        self.physics_constraints.iter().find(|c| c.name == name)
    }

    /// Convenience for grabbing the default skin, if any.
    #[must_use]
    pub fn default_skin(&self) -> Option<&Skin> {
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
        sd.skins.push(Skin::new("default"));
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
        assert!(sd.find_ik_constraint("anything").is_none());
    }

    #[test]
    fn default_skin_round_trips() {
        let sd = sample_skeleton();
        assert_eq!(sd.default_skin().unwrap().name, "default");
    }
}
