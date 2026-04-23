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

//! Top-level runtime-mutable pose container. Wraps an `Arc<SkeletonData>` and
//! holds one mutable runtime instance per data element (bone, slot, each
//! constraint kind), plus skeleton-wide mutable state (active skin, color,
//! position, scale, time, draw order, update cache).

use std::sync::Arc;

use crate::data::{SkeletonData, SkinId, SlotId};
use crate::math::Color;
use crate::skeleton::{
    Bone, IkConstraint, PathConstraint, Physics, PhysicsConstraint, Slot, TransformConstraint,
    UpdateCacheEntry,
};

/// Runtime-mutable pose for one loaded skeleton.
///
/// Construction copies every setup-pose value out of [`SkeletonData`] into
/// mutable runtime Vecs. Multiple `Skeleton`s can share one `Arc<SkeletonData>`
/// — the immutable asset is never cloned.
#[derive(Debug, Clone)]
pub struct Skeleton {
    data: Arc<SkeletonData>,

    /// One runtime `Bone` per `data.bones` entry, in the same order.
    pub bones: Vec<Bone>,
    /// One runtime `Slot` per `data.slots` entry, in setup draw order.
    pub slots: Vec<Slot>,

    pub ik_constraints: Vec<IkConstraint>,
    pub transform_constraints: Vec<TransformConstraint>,
    pub path_constraints: Vec<PathConstraint>,
    pub physics_constraints: Vec<PhysicsConstraint>,

    /// Current render-order permutation over `slots`. `draw_order[i]` is the
    /// slot drawn i-th from the back. Reset to `0..slots.len()` on setup pose;
    /// `DrawOrderTimeline` (Phase 3) permutes it per animation.
    pub draw_order: Vec<SlotId>,

    /// Currently applied skin, or `None`. Attachment resolution walks the
    /// active skin first, then falls back to `data.default_skin`.
    pub skin: Option<SkinId>,

    /// Skeleton tint; multiplied into every slot color at render time.
    pub color: Color,

    /// Skeleton origin offset in parent/world space. Added into every
    /// bone's computed world translation by `update_world_transform`.
    pub x: f32,
    pub y: f32,

    /// Skeleton-level scale (e.g. `-1` on `scale_x` for mirrored skeletons).
    /// Applied by `update_world_transform` to every root bone. Defaults to 1.
    pub scale_x: f32,
    pub scale_y: f32,

    /// Accumulated simulation time, advanced by [`Skeleton::update`].
    /// Physics constraints consume this to compute per-step deltas. Zero on
    /// construction.
    pub time: f32,

    /// Pose/constraint update order. Rebuilt by
    /// [`Skeleton::update_cache`][Self::update_cache] whenever the skin
    /// changes. Empty until the first `update_cache` call.
    pub update_cache: Vec<UpdateCacheEntry>,
}

impl Skeleton {
    /// Build a fresh runtime skeleton from `data`.
    ///
    /// Populates every mutable runtime vec with instances initialised to
    /// their setup-pose values. Does **not** run `update_cache` or
    /// `update_world_transform` — call [`Self::set_to_setup_pose`] and
    /// [`Self::update_world_transform`] before inspecting bone world
    /// transforms.
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        let mut bones: Vec<Bone> = data.bones.iter().map(Bone::new).collect();
        // Populate each bone's `children` vec. `data.bones` is
        // parents-before-children by invariant, so one forward pass suffices.
        for child_bone in &data.bones {
            if let Some(parent_id) = child_bone.parent {
                bones[parent_id.index()].children.push(child_bone.index);
            }
        }

        let slots: Vec<Slot> = data.slots.iter().map(Slot::new).collect();
        let draw_order: Vec<SlotId> = data.slots.iter().map(|s| s.index).collect();

        let ik_constraints: Vec<IkConstraint> =
            data.ik_constraints.iter().map(IkConstraint::new).collect();
        let transform_constraints: Vec<TransformConstraint> = data
            .transform_constraints
            .iter()
            .map(TransformConstraint::new)
            .collect();
        let path_constraints: Vec<PathConstraint> = data
            .path_constraints
            .iter()
            .map(PathConstraint::new)
            .collect();
        let physics_constraints: Vec<PhysicsConstraint> = data
            .physics_constraints
            .iter()
            .map(PhysicsConstraint::new)
            .collect();

        Self {
            data,
            bones,
            slots,
            ik_constraints,
            transform_constraints,
            path_constraints,
            physics_constraints,
            draw_order,
            skin: None,
            color: Color::WHITE,
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            time: 0.0,
            update_cache: Vec::new(),
        }
    }

    /// Immutable handle to the shared setup-pose data.
    #[inline]
    #[must_use]
    pub fn data(&self) -> &Arc<SkeletonData> {
        &self.data
    }

    // ----- update cache ----------------------------------------------------

    /// Rebuild [`Self::update_cache`] from the current skin + constraint
    /// dependency graph.
    ///
    /// Port target: `spine::Skeleton::updateCache` (Phase 2c). Until then
    /// this is a stub.
    pub fn update_cache(&mut self) {
        unimplemented!("Skeleton::update_cache: port in Phase 2c");
    }

    // ----- setup pose ------------------------------------------------------

    /// Reset every bone, slot, and constraint to its setup-pose value.
    ///
    /// Port target: `spine::Skeleton::setToSetupPose`. Sub-phase 2e.
    pub fn set_to_setup_pose(&mut self) {
        unimplemented!("Skeleton::set_to_setup_pose: port in Phase 2e");
    }

    /// Reset every bone to setup pose. Sub-phase 2e.
    pub fn set_bones_to_setup_pose(&mut self) {
        unimplemented!("Skeleton::set_bones_to_setup_pose: port in Phase 2e");
    }

    /// Reset every slot (color + attachment) to setup pose. Sub-phase 2e.
    pub fn set_slots_to_setup_pose(&mut self) {
        unimplemented!("Skeleton::set_slots_to_setup_pose: port in Phase 2e");
    }

    // ----- skin ------------------------------------------------------------

    /// Activate `skin` (or clear when `None`). Re-runs `update_cache` because
    /// skin-required bones and constraints change which entries are active.
    ///
    /// Sub-phase 2e.
    pub fn set_skin(&mut self, _skin: Option<SkinId>) {
        unimplemented!("Skeleton::set_skin: port in Phase 2e");
    }

    /// Name-lookup wrapper around [`Self::set_skin`]. Sub-phase 2e.
    ///
    /// # Errors
    ///
    /// Returns [`SkinNotFound`] when `name` doesn't match any skin in
    /// `self.data().skins`.
    pub fn set_skin_by_name(&mut self, _name: &str) -> Result<(), SkinNotFound> {
        unimplemented!("Skeleton::set_skin_by_name: port in Phase 2e");
    }

    // ----- pose pipeline ---------------------------------------------------

    /// Advance simulation time by `delta` seconds. Physics constraints
    /// (Phase 5) read `time` on update; in Phase 2 this is the entire body.
    pub fn update(&mut self, delta: f32) {
        self.time += delta;
    }

    /// Walk [`Self::update_cache`] and compute every bone's world transform.
    ///
    /// Port target: `spine::Skeleton::updateWorldTransform(Physics)`.
    /// Sub-phase 2d.
    pub fn update_world_transform(&mut self, _physics: Physics) {
        unimplemented!("Skeleton::update_world_transform: port in Phase 2d");
    }
}

/// Error returned by [`Skeleton::set_skin_by_name`] when no skin with the
/// given name exists in the skeleton's data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("skeleton has no skin named `{0}`")]
pub struct SkinNotFound(pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BoneData, BoneId, SlotData};

    fn two_bone_data() -> Arc<SkeletonData> {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "body", Some(BoneId(0))));
        sd.slots.push(SlotData::new(SlotId(0), "torso", BoneId(1)));
        Arc::new(sd)
    }

    #[test]
    fn construction_mirrors_data_sizes() {
        let data = two_bone_data();
        let sk = Skeleton::new(Arc::clone(&data));
        assert_eq!(sk.bones.len(), 2);
        assert_eq!(sk.slots.len(), 1);
        assert_eq!(sk.draw_order, vec![SlotId(0)]);
        assert!(Arc::ptr_eq(sk.data(), &data));
    }

    #[test]
    fn children_populated_from_parent_links() {
        let sk = Skeleton::new(two_bone_data());
        assert_eq!(sk.bones[0].children, vec![BoneId(1)]);
        assert!(sk.bones[1].children.is_empty());
    }

    #[test]
    fn update_advances_time_without_touching_anything_else() {
        let mut sk = Skeleton::new(two_bone_data());
        sk.update(0.25);
        sk.update(0.5);
        assert!((sk.time - 0.75).abs() < 1e-6);
    }
}
