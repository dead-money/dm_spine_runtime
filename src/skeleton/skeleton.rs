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

use crate::data::{Attachment, BoneId, SkeletonData, Skin, SkinId, SlotId};
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
    /// Literal port of `spine::Skeleton::updateCache`. Two passes:
    ///
    /// 1. Seed every bone's `sorted` / `active` flags from skin-required
    ///    status; then walk parents up from each skin-listed bone to
    ///    re-activate the skin chain.
    /// 2. Iterate constraints by ascending `data.order`, dispatching to the
    ///    matching `sort_*` helper (which recursively `sort_bone`s its
    ///    dependencies, appends itself, then `sort_reset`s affected subtrees
    ///    so bones can re-appear in the cache after the constraint).
    /// 3. Final `sort_bone` sweep pulls in anything not yet in the cache.
    ///
    /// After this call, walking `self.update_cache` in order produces a
    /// dependency-correct update schedule.
    pub fn update_cache(&mut self) {
        self.update_cache.clear();

        for bone in &mut self.bones {
            let skin_required = self.data.bones[bone.data_index.index()].skin_required;
            bone.sorted = skin_required;
            bone.active = !skin_required;
        }

        // A skin re-enables its required bones (and every ancestor). We copy
        // the bones list out of `self.data` first so we can mutably borrow
        // `self.bones` in the loop without aliasing.
        if let Some(skin_id) = self.skin {
            let skin_bones_len = self.data.skins[skin_id.index()].bones.len();
            for i in 0..skin_bones_len {
                let start = self.data.skins[skin_id.index()].bones[i];
                let mut current: Option<BoneId> = Some(start);
                while let Some(id) = current {
                    let bone = &mut self.bones[id.index()];
                    bone.sorted = false;
                    bone.active = true;
                    current = bone.parent;
                }
            }
        }

        // Constraint-order pass. Matches the `goto continue_outer` shape of
        // `spine-cpp/src/spine/Skeleton.cpp` literally: find the constraint
        // (of any kind) whose `data.order == i`, sort it, bump `i`, restart
        // the search. `i` still advances when nothing is found at that order
        // (mirrors the outer `for (; i < count; ++i)` fallthrough).
        let ik_count = self.ik_constraints.len();
        let transform_count = self.transform_constraints.len();
        let path_count = self.path_constraints.len();
        let physics_count = self.physics_constraints.len();
        let constraint_count = ik_count + transform_count + path_count + physics_count;

        let mut i: u32 = 0;
        'outer: loop {
            if (i as usize) >= constraint_count {
                break;
            }

            for ii in 0..ik_count {
                let data_idx = self.ik_constraints[ii].data_index;
                if self.data.ik_constraints[data_idx.index()].order == i {
                    self.sort_ik_constraint(ii);
                    i += 1;
                    continue 'outer;
                }
            }
            for ii in 0..transform_count {
                let data_idx = self.transform_constraints[ii].data_index;
                if self.data.transform_constraints[data_idx.index()].order == i {
                    self.sort_transform_constraint(ii);
                    i += 1;
                    continue 'outer;
                }
            }
            for ii in 0..path_count {
                let data_idx = self.path_constraints[ii].data_index;
                if self.data.path_constraints[data_idx.index()].order == i {
                    self.sort_path_constraint(ii);
                    i += 1;
                    continue 'outer;
                }
            }
            for ii in 0..physics_count {
                let data_idx = self.physics_constraints[ii].data_index;
                if self.data.physics_constraints[data_idx.index()].order == i {
                    self.sort_physics_constraint(ii);
                    i += 1;
                    continue 'outer;
                }
            }

            i += 1;
        }

        for i in 0..self.bones.len() {
            self.sort_bone(BoneId(i as u16));
        }
    }

    // ----- update_cache helpers --------------------------------------------

    /// Append `bone_id` (and its parent chain, if unsorted) to the update
    /// cache. Ports `spine::Skeleton::sortBone`.
    fn sort_bone(&mut self, bone_id: BoneId) {
        if self.bones[bone_id.index()].sorted {
            return;
        }
        if let Some(parent_id) = self.bones[bone_id.index()].parent {
            self.sort_bone(parent_id);
        }
        self.bones[bone_id.index()].sorted = true;
        self.update_cache.push(UpdateCacheEntry::Bone(bone_id));
    }

    /// Clear the `sorted` flag on active bones in `bone_ids`, recursing into
    /// children of any bone that *was* sorted. Ports `spine::Skeleton::sortReset`.
    ///
    /// After a constraint appends itself to the cache, every descendant of
    /// the constrained bones may need to run *again* downstream — clearing
    /// `sorted` here lets the final bone-sweep pass re-append them after the
    /// constraint.
    fn sort_reset(&mut self, bone_ids: &[BoneId]) {
        for &bone_id in bone_ids {
            if !self.bones[bone_id.index()].active {
                continue;
            }
            if self.bones[bone_id.index()].sorted {
                // Clone children so the recursion can freely take `&mut self`.
                // Children lists are short (usually <10) and this only runs
                // during `update_cache`, so allocation cost is negligible.
                let children = self.bones[bone_id.index()].children.clone();
                self.sort_reset(&children);
            }
            self.bones[bone_id.index()].sorted = false;
        }
    }

    /// Whether the currently active skin includes `constraint_data_index`
    /// for IK constraints. Used in the `active` predicate; matches
    /// `_skin && _skin->_constraints.contains(&constraint->_data)`.
    fn current_skin(&self) -> Option<&Skin> {
        self.skin.map(|id| &self.data.skins[id.index()])
    }

    fn sort_ik_constraint(&mut self, idx: usize) {
        let (target, constrained, data_idx) = {
            let c = &self.ik_constraints[idx];
            (c.target, c.bones.clone(), c.data_index)
        };
        let data = &self.data.ik_constraints[data_idx.index()];
        let skin_ok = !data.skin_required
            || self
                .current_skin()
                .is_some_and(|s| s.ik_constraints.contains(&data_idx));
        let target_active = self.bones[target.index()].active;
        let active = target_active && skin_ok;
        self.ik_constraints[idx].active = active;
        if !active {
            return;
        }

        self.sort_bone(target);
        let parent_id = constrained[0];
        self.sort_bone(parent_id);

        if constrained.len() == 1 {
            self.update_cache
                .push(UpdateCacheEntry::IkConstraint(data_idx));
            let children = self.bones[parent_id.index()].children.clone();
            self.sort_reset(&children);
        } else {
            let child_id = constrained[constrained.len() - 1];
            self.sort_bone(child_id);
            self.update_cache
                .push(UpdateCacheEntry::IkConstraint(data_idx));
            let children = self.bones[parent_id.index()].children.clone();
            self.sort_reset(&children);
            self.bones[child_id.index()].sorted = true;
        }
    }

    fn sort_transform_constraint(&mut self, idx: usize) {
        let (target, constrained, data_idx) = {
            let c = &self.transform_constraints[idx];
            (c.target, c.bones.clone(), c.data_index)
        };
        let (skin_required, is_local) = {
            let data = &self.data.transform_constraints[data_idx.index()];
            (data.skin_required, data.local)
        };
        let skin_ok = !skin_required
            || self
                .current_skin()
                .is_some_and(|s| s.transform_constraints.contains(&data_idx));
        let target_active = self.bones[target.index()].active;
        let active = target_active && skin_ok;
        self.transform_constraints[idx].active = active;
        if !active {
            return;
        }

        self.sort_bone(target);

        if is_local {
            for &child_id in &constrained {
                if let Some(parent_id) = self.bones[child_id.index()].parent {
                    self.sort_bone(parent_id);
                }
                self.sort_bone(child_id);
            }
        } else {
            for &bone_id in &constrained {
                self.sort_bone(bone_id);
            }
        }

        self.update_cache
            .push(UpdateCacheEntry::TransformConstraint(data_idx));

        for &bone_id in &constrained {
            let children = self.bones[bone_id.index()].children.clone();
            self.sort_reset(&children);
        }
        for &bone_id in &constrained {
            self.bones[bone_id.index()].sorted = true;
        }
    }

    fn sort_path_constraint(&mut self, idx: usize) {
        let (target_slot, constrained, data_idx) = {
            let c = &self.path_constraints[idx];
            (c.target, c.bones.clone(), c.data_index)
        };
        let data = &self.data.path_constraints[data_idx.index()];

        // The target is a Slot; path constraints are `active` only when the
        // slot's bone is `active` *and* skin requirements are met.
        let slot_bone_id = self.data.slots[target_slot.index()].bone;
        let slot_bone_active = self.bones[slot_bone_id.index()].active;
        let skin_ok = !data.skin_required
            || self
                .current_skin()
                .is_some_and(|s| s.path_constraints.contains(&data_idx));
        let active = slot_bone_active && skin_ok;
        self.path_constraints[idx].active = active;
        if !active {
            return;
        }

        // Sort the path attachment's own bones. spine-cpp walks every skin
        // here (current → default → all), in case any of them carries a path
        // attachment on this slot that the constraint will use.
        if let Some(current_skin_id) = self.skin {
            self.sort_path_constraint_attachment_in_skin(
                current_skin_id,
                target_slot,
                slot_bone_id,
            );
        }
        if let Some(default_skin_id) = self.data.default_skin
            && Some(default_skin_id) != self.skin
        {
            self.sort_path_constraint_attachment_in_skin(
                default_skin_id,
                target_slot,
                slot_bone_id,
            );
        }
        let skin_count = self.data.skins.len();
        for ii in 0..skin_count {
            self.sort_path_constraint_attachment_in_skin(
                SkinId(ii as u16),
                target_slot,
                slot_bone_id,
            );
        }

        // Also sort the slot's currently-assigned attachment if it happens
        // to be a PathAttachment.
        if let Some(attachment_id) = self.slots[target_slot.index()].attachment {
            self.sort_path_constraint_attachment(attachment_id, slot_bone_id);
        }

        for &bone_id in &constrained {
            self.sort_bone(bone_id);
        }

        self.update_cache
            .push(UpdateCacheEntry::PathConstraint(data_idx));

        for &bone_id in &constrained {
            let children = self.bones[bone_id.index()].children.clone();
            self.sort_reset(&children);
        }
        for &bone_id in &constrained {
            self.bones[bone_id.index()].sorted = true;
        }
    }

    fn sort_physics_constraint(&mut self, idx: usize) {
        let (bone_id, data_idx) = {
            let c = &self.physics_constraints[idx];
            (c.bone, c.data_index)
        };
        let data = &self.data.physics_constraints[data_idx.index()];
        let bone_active = self.bones[bone_id.index()].active;
        let skin_ok = !data.skin_required
            || self
                .current_skin()
                .is_some_and(|s| s.physics_constraints.contains(&data_idx));
        let active = bone_active && skin_ok;
        self.physics_constraints[idx].active = active;
        if !active {
            return;
        }

        self.sort_bone(bone_id);
        self.update_cache
            .push(UpdateCacheEntry::PhysicsConstraint(data_idx));
        let children = self.bones[bone_id.index()].children.clone();
        self.sort_reset(&children);
        self.bones[bone_id.index()].sorted = true;
    }

    /// Walk all attachments of `skin`, and for each entry at `slot_index`
    /// that happens to be a `PathAttachment`, sort the bones it references.
    /// Matches `spine::Skeleton::sortPathConstraintAttachment(Skin *, ...)`.
    fn sort_path_constraint_attachment_in_skin(
        &mut self,
        skin_id: SkinId,
        slot_index: SlotId,
        slot_bone: BoneId,
    ) {
        // Snapshot matching attachment ids up-front so the iteration doesn't
        // hold a borrow of `self.data` while we call back into `&mut self`.
        let mut matching: Vec<crate::data::AttachmentId> = Vec::new();
        let skin = &self.data.skins[skin_id.index()];
        for (slot, _name, attachment_id) in skin.attachments() {
            if slot == slot_index {
                matching.push(attachment_id);
            }
        }
        for attachment_id in matching {
            self.sort_path_constraint_attachment(attachment_id, slot_bone);
        }
    }

    /// If `attachment_id` resolves to a `PathAttachment`, sort every bone
    /// the attachment references. Matches
    /// `spine::Skeleton::sortPathConstraintAttachment(Attachment *, Bone &)`.
    ///
    /// The weighted-bone stream on a path attachment is the length-prefixed
    /// pattern `[count, bone, bone, ..., count', bone, ...]` — ported
    /// verbatim from `spine-cpp`.
    fn sort_path_constraint_attachment(
        &mut self,
        attachment_id: crate::data::AttachmentId,
        slot_bone: BoneId,
    ) {
        let path_bones: Vec<i32> = match &self.data.attachments[attachment_id.index()] {
            Attachment::Path(path) => path.vertex_data.bones.clone(),
            _ => return,
        };

        if path_bones.is_empty() {
            self.sort_bone(slot_bone);
        } else {
            let mut i: usize = 0;
            let n = path_bones.len();
            while i < n {
                let nn = path_bones[i] as usize;
                i += 1;
                let group_end = i + nn;
                while i < group_end {
                    let bone_index = path_bones[i] as usize;
                    i += 1;
                    self.sort_bone(BoneId(bone_index as u16));
                }
            }
        }
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
    use crate::data::{
        BoneData, BoneId, IkConstraintData, IkConstraintId, SlotData, TransformConstraintData,
        TransformConstraintId,
    };

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

    // -- update_cache ordering ----------------------------------------------

    /// A three-bone linear chain (root → mid → tip), no constraints. Cache
    /// must be exactly those three bones in parent-first order.
    #[test]
    fn update_cache_three_bone_chain() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "mid", Some(BoneId(0))));
        sd.bones
            .push(BoneData::new(BoneId(2), "tip", Some(BoneId(1))));
        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();
        assert_eq!(
            sk.update_cache,
            vec![
                UpdateCacheEntry::Bone(BoneId(0)),
                UpdateCacheEntry::Bone(BoneId(1)),
                UpdateCacheEntry::Bone(BoneId(2)),
            ]
        );
        assert!(sk.bones.iter().all(|b| b.active));
    }

    /// Two-bone IK pulling `upper → lower` toward `target`. Expected cache:
    /// parents sorted first (root, upper), then lower, then the IK constraint,
    /// then `target` lands via the final bone sweep (it's not a child of any
    /// constrained bone). After the IK, `lower.sorted` is set to `true` by
    /// the sort helper so the final sweep doesn't re-append it.
    #[test]
    fn update_cache_two_bone_ik_places_constraint_after_chain() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "upper", Some(BoneId(0))));
        sd.bones
            .push(BoneData::new(BoneId(2), "lower", Some(BoneId(1))));
        sd.bones
            .push(BoneData::new(BoneId(3), "target", Some(BoneId(0))));

        let mut ik = IkConstraintData::new(IkConstraintId(0), "ik", BoneId(3));
        ik.bones = vec![BoneId(1), BoneId(2)];
        ik.order = 0;
        sd.ik_constraints.push(ik);

        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        // First four entries: root, target (sorted as IK's target), upper, lower,
        // then the IK itself. Final sweep adds nothing new because every bone
        // is already `sorted = true` (target and upper set by sort_bone, lower
        // explicitly set by the IK helper's final `sorted = true`).
        assert_eq!(
            sk.update_cache,
            vec![
                UpdateCacheEntry::Bone(BoneId(0)), // root (parent of target)
                UpdateCacheEntry::Bone(BoneId(3)), // target
                UpdateCacheEntry::Bone(BoneId(1)), // upper (IK parent)
                UpdateCacheEntry::Bone(BoneId(2)), // lower (IK child)
                UpdateCacheEntry::IkConstraint(IkConstraintId(0)),
            ]
        );
        assert!(sk.ik_constraints[0].active);
    }

    /// Two-bone IK feeds into a transform constraint that reads `lower`
    /// (the IK's child). The transform has a higher `order`, so it must
    /// appear *after* the IK in the cache, not before. Also verifies that
    /// `sort_reset` clears `sorted` on IK descendants so they run again
    /// after the transform if needed.
    #[test]
    fn update_cache_transform_after_ik_when_order_says_so() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "upper", Some(BoneId(0))));
        sd.bones
            .push(BoneData::new(BoneId(2), "lower", Some(BoneId(1))));
        sd.bones
            .push(BoneData::new(BoneId(3), "ik_target", Some(BoneId(0))));
        sd.bones
            .push(BoneData::new(BoneId(4), "follower", Some(BoneId(0))));

        let mut ik = IkConstraintData::new(IkConstraintId(0), "ik", BoneId(3));
        ik.bones = vec![BoneId(1), BoneId(2)];
        ik.order = 0;
        sd.ik_constraints.push(ik);

        // Transform constraint: `follower` copies from `lower`.
        let mut tc = TransformConstraintData::new(TransformConstraintId(0), "tc", BoneId(2));
        tc.bones = vec![BoneId(4)];
        tc.order = 1;
        sd.transform_constraints.push(tc);

        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();

        // Find each constraint's position in the cache.
        let ik_pos = sk
            .update_cache
            .iter()
            .position(|e| matches!(e, UpdateCacheEntry::IkConstraint(_)))
            .expect("IK in cache");
        let tc_pos = sk
            .update_cache
            .iter()
            .position(|e| matches!(e, UpdateCacheEntry::TransformConstraint(_)))
            .expect("TC in cache");
        assert!(ik_pos < tc_pos, "IK (order 0) must run before TC (order 1)");

        // `lower` (IK's child) must appear before the IK. The transform's
        // target sort may or may not re-enqueue `lower` — what we care about
        // is IK-first, TC-second.
        let lower_pos = sk
            .update_cache
            .iter()
            .position(|e| *e == UpdateCacheEntry::Bone(BoneId(2)))
            .expect("lower in cache");
        assert!(lower_pos < ik_pos);

        // `follower` must come before TC and after IK (TC pulls it in).
        let follower_first = sk
            .update_cache
            .iter()
            .position(|e| *e == UpdateCacheEntry::Bone(BoneId(4)))
            .expect("follower in cache");
        assert!(follower_first < tc_pos);
    }

    /// Skin-required bones stay inactive (and out of the cache) until a skin
    /// that lists them is applied.
    #[test]
    fn update_cache_excludes_skin_required_bone_without_matching_skin() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "normal", Some(BoneId(0))));
        let mut hidden = BoneData::new(BoneId(2), "hidden", Some(BoneId(0)));
        hidden.skin_required = true;
        sd.bones.push(hidden);
        // Skin "extras" activates the hidden bone.
        let mut skin = crate::data::Skin::new("extras");
        skin.bones.push(BoneId(2));
        sd.skins.push(skin);
        let data = Arc::new(sd);

        // No skin applied — hidden bone must be excluded.
        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();
        assert!(
            !sk.update_cache.contains(&UpdateCacheEntry::Bone(BoneId(2))),
            "hidden bone must stay out of the cache when no skin claims it"
        );
        assert!(!sk.bones[2].active);

        // Apply the extras skin — hidden bone (and its root ancestor,
        // already active) come in, with `active = true`.
        sk.skin = Some(SkinId(0));
        sk.update_cache();
        assert!(
            sk.update_cache.contains(&UpdateCacheEntry::Bone(BoneId(2))),
            "hidden bone must appear once extras skin is applied"
        );
        assert!(sk.bones[2].active);
    }

    #[test]
    fn update_advances_time_without_touching_anything_else() {
        let mut sk = Skeleton::new(two_bone_data());
        sk.update(0.25);
        sk.update(0.5);
        assert!((sk.time - 0.75).abs() < 1e-6);
    }
}
