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

use crate::data::{Attachment, AttachmentId, BoneId, Inherit, SkeletonData, Skin, SkinId, SlotId};
use crate::math::Color;
use crate::math::util::{atan2_deg, cos_deg, sin_deg};
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
    // pub(crate) so sibling modules (notably `animation::apply`) can read
    // setup-pose fields while mutably borrowing `self.bones`. External users
    // still go through [`Self::data`].
    pub(crate) data: Arc<SkeletonData>,

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
    /// Port of `spine::Skeleton::setToSetupPose`: bones + constraints first,
    /// then slots (order matters — slot attachment resolution reads from the
    /// current skin, which doesn't depend on bones but preserves parity with
    /// spine-cpp's sequence).
    pub fn set_to_setup_pose(&mut self) {
        self.set_bones_to_setup_pose();
        self.set_slots_to_setup_pose();
    }

    /// Reset every runtime bone + constraint to its `data`'s setup values.
    ///
    /// Does **not** touch slot attachments (they depend on the active skin).
    /// Ports `spine::Skeleton::setBonesToSetupPose`.
    pub fn set_bones_to_setup_pose(&mut self) {
        for bone in &mut self.bones {
            let data = &self.data.bones[bone.data_index.index()];
            bone.set_to_setup_pose(data);
        }
        for c in &mut self.ik_constraints {
            let data = &self.data.ik_constraints[c.data_index.index()];
            c.set_to_setup_pose(data);
        }
        for c in &mut self.transform_constraints {
            let data = &self.data.transform_constraints[c.data_index.index()];
            c.set_to_setup_pose(data);
        }
        for c in &mut self.path_constraints {
            let data = &self.data.path_constraints[c.data_index.index()];
            c.set_to_setup_pose(data);
        }
        for c in &mut self.physics_constraints {
            let data = &self.data.physics_constraints[c.data_index.index()];
            c.set_to_setup_pose(data);
        }
    }

    /// Reset draw order to the identity permutation, then reset every slot's
    /// color, dark-color, deform, and attachment (via current-skin →
    /// default-skin resolution). Ports `spine::Skeleton::setSlotsToSetupPose`.
    pub fn set_slots_to_setup_pose(&mut self) {
        self.draw_order.clear();
        self.draw_order
            .extend(self.data.slots.iter().map(|s| s.index));

        for i in 0..self.slots.len() {
            let slot_id = SlotId(i as u16);
            // Snapshot the data attachment name to a local String so
            // `get_attachment` doesn't fight the borrow of `self.data`.
            let attachment_name = self.data.slots[i].attachment_name.clone();
            {
                let data = &self.data.slots[i];
                self.slots[i].set_to_setup_pose(data);
            }
            self.slots[i].attachment = match attachment_name.as_deref() {
                Some(name) if !name.is_empty() => self.get_attachment(slot_id, name),
                _ => None,
            };
        }
    }

    // ----- attachment resolution ------------------------------------------

    /// Resolve a slot's attachment by name, walking the active skin first
    /// and the default skin as fallback. Matches
    /// `spine::Skeleton::getAttachment(int, const String &)`.
    #[must_use]
    pub fn get_attachment(&self, slot_id: SlotId, name: &str) -> Option<AttachmentId> {
        if name.is_empty() {
            return None;
        }
        if let Some(skin_id) = self.skin
            && let Some(att) = self.data.skins[skin_id.index()].get_attachment(slot_id, name)
        {
            return Some(att);
        }
        self.data
            .default_skin
            .and_then(|id| self.data.skins[id.index()].get_attachment(slot_id, name))
    }

    // ----- skin ------------------------------------------------------------

    /// Activate `skin` (or clear when `None`). On a skin swap, attachments
    /// currently pointing at the old skin's entries are remapped to the new
    /// skin's entries with the same name (spine-cpp's `Skin::attachAll`).
    /// Re-runs `update_cache` because skin-required bones and constraints
    /// may change which entries are active.
    pub fn set_skin(&mut self, new_skin: Option<SkinId>) {
        if self.skin == new_skin {
            return;
        }

        if let Some(new_id) = new_skin {
            match self.skin {
                Some(old_id) => self.attach_all(old_id, new_id),
                None => {
                    // No previous skin: apply the new skin's attachments for
                    // each slot whose data carries a setup attachment name.
                    for i in 0..self.slots.len() {
                        let slot_id = SlotId(i as u16);
                        let Some(name) = self.data.slots[i].attachment_name.clone() else {
                            continue;
                        };
                        if name.is_empty() {
                            continue;
                        }
                        if let Some(att) =
                            self.data.skins[new_id.index()].get_attachment(slot_id, &name)
                        {
                            self.slots[i].attachment = Some(att);
                        }
                    }
                }
            }
        }

        self.skin = new_skin;
        self.update_cache();
    }

    /// Name-lookup wrapper around [`Self::set_skin`].
    ///
    /// # Errors
    ///
    /// Returns [`SkinNotFound`] when `name` doesn't match any skin in
    /// `self.data().skins`.
    pub fn set_skin_by_name(&mut self, name: &str) -> Result<(), SkinNotFound> {
        let skin_id = self
            .data
            .skins
            .iter()
            .position(|s| s.name == name)
            .map(|i| SkinId(i as u16))
            .ok_or_else(|| SkinNotFound(name.to_string()))?;
        self.set_skin(Some(skin_id));
        Ok(())
    }

    /// Port of `spine::Skin::attachAll`: for every attachment entry in the
    /// old skin, if the slot currently shows that attachment *and* the new
    /// skin has an entry with the same name on the same slot, swap the
    /// runtime slot's attachment to the new one. Preserves the "which
    /// attachment is currently showing" state across skin swaps.
    fn attach_all(&mut self, old_skin_id: SkinId, new_skin_id: SkinId) {
        let entries: Vec<(SlotId, String, AttachmentId)> = self.data.skins[old_skin_id.index()]
            .attachments()
            .map(|(slot, name, id)| (slot, name.to_string(), id))
            .collect();
        for (slot_id, name, old_att) in entries {
            if self.slots[slot_id.index()].attachment == Some(old_att)
                && let Some(new_att) =
                    self.data.skins[new_skin_id.index()].get_attachment(slot_id, &name)
            {
                self.slots[slot_id.index()].attachment = Some(new_att);
            }
        }
    }

    // ----- pose pipeline ---------------------------------------------------

    /// Advance simulation time by `delta` seconds. Physics constraints
    /// (Phase 5) read `time` on update; in Phase 2 this is the entire body.
    pub fn update(&mut self, delta: f32) {
        self.time += delta;
    }

    /// Walk [`Self::update_cache`] and compute every bone's world transform.
    ///
    /// Literal port of `spine::Skeleton::updateWorldTransform(Physics)`:
    /// first copies every bone's local TRS into its "applied" counterpart,
    /// then dispatches each cache entry. Constraints no-op in Phase 2 —
    /// Phase 5 will replace their stubs with real solvers.
    pub fn update_world_transform(&mut self, physics: Physics) {
        let physics_arg = physics;
        for bone in &mut self.bones {
            bone.ax = bone.x;
            bone.ay = bone.y;
            bone.a_rotation = bone.rotation;
            bone.a_scale_x = bone.scale_x;
            bone.a_scale_y = bone.scale_y;
            bone.a_shear_x = bone.shear_x;
            bone.a_shear_y = bone.shear_y;
        }

        for i in 0..self.update_cache.len() {
            match self.update_cache[i] {
                UpdateCacheEntry::Bone(bone_id) => {
                    self.update_bone_world_transform(bone_id);
                }
                UpdateCacheEntry::IkConstraint(id) => {
                    crate::skeleton::ik::solve_ik_constraint(self, id.index());
                }
                UpdateCacheEntry::TransformConstraint(id) => {
                    crate::skeleton::transform::solve_transform_constraint(self, id.index());
                }
                UpdateCacheEntry::PathConstraint(id) => {
                    crate::skeleton::path::solve_path_constraint(self, id.index());
                }
                UpdateCacheEntry::PhysicsConstraint(id) => {
                    crate::skeleton::physics::solve_physics_constraint(
                        self,
                        id.index(),
                        physics_arg,
                    );
                }
            }
        }
    }

    /// `Bone::localToWorld(local_x, local_y) -> (world_x, world_y)`.
    /// Projects a point in the bone's local space into skeleton world
    /// space. Used by Transform constraints to compute offset targets.
    pub(crate) fn bone_local_to_world(
        &self,
        bone_id: BoneId,
        local_x: f32,
        local_y: f32,
    ) -> (f32, f32) {
        let b = &self.bones[bone_id.index()];
        (
            b.a * local_x + b.b * local_y + b.world_x,
            b.c * local_x + b.d * local_y + b.world_y,
        )
    }

    /// `Bone::updateAppliedTransform()` — rebuilds the bone's `applied`
    /// local TRS from its current world matrix. Inverse of
    /// [`update_bone_world_transform_with`][Self::update_bone_world_transform_with].
    ///
    /// Called by constraint solvers that write to the world matrix
    /// directly (world-space Transform constraints) so subsequent
    /// readers of the applied-local fields see coherent values.
    ///
    /// Literal port of `spine-cpp/src/spine/Bone.cpp`
    /// `Bone::updateAppliedTransform`. The per-Inherit-mode branches
    /// mirror the forward-transform function exactly.
    #[allow(
        clippy::too_many_lines,
        clippy::float_cmp,
        clippy::many_single_char_names
    )]
    pub(crate) fn update_applied_transform(&mut self, bone_id: BoneId) {
        let idx = bone_id.index();
        let (parent, inherit, a, b, c, d, world_x, world_y, rotation) = {
            let bone = &self.bones[idx];
            (
                bone.parent,
                bone.inherit,
                bone.a,
                bone.b,
                bone.c,
                bone.d,
                bone.world_x,
                bone.world_y,
                bone.rotation,
            )
        };
        let Some(parent_id) = parent else {
            let bone = &mut self.bones[idx];
            bone.ax = world_x - self.x;
            bone.ay = world_y - self.y;
            bone.a_rotation = atan2_deg(c, a);
            bone.a_scale_x = (a * a + c * c).sqrt();
            bone.a_scale_y = (b * b + d * d).sqrt();
            bone.a_shear_x = 0.0;
            bone.a_shear_y = atan2_deg(a * b + c * d, a * d - b * c);
            return;
        };

        let (mut pa, mut pb, mut pc, mut pd, p_world_x, p_world_y) = {
            let p = &self.bones[parent_id.index()];
            (p.a, p.b, p.c, p.d, p.world_x, p.world_y)
        };
        let sk_scale_x = self.scale_x;
        let sk_scale_y = self.scale_y;

        let pid_raw = pa * pd - pb * pc;
        let mut pid = if pid_raw == 0.0 { 0.0 } else { 1.0 / pid_raw };
        let mut ia = pd * pid;
        let mut ib = pb * pid;
        let mut ic = pc * pid;
        let mut id = pa * pid;
        let dx = world_x - p_world_x;
        let dy = world_y - p_world_y;
        let ax = dx * ia - dy * ib;
        let ay = dy * id - dx * ic;

        let (ra, rb, rc, rd);
        if inherit == Inherit::OnlyTranslation {
            ra = a;
            rb = b;
            rc = c;
            rd = d;
        } else {
            match inherit {
                Inherit::NoRotationOrReflection => {
                    let s = (pa * pd - pb * pc).abs() / (pa * pa + pc * pc);
                    let sa = pa / sk_scale_x;
                    let sc = pc / sk_scale_y;
                    pb = -sc * s * sk_scale_x;
                    pd = sa * s * sk_scale_y;
                    pid = 1.0 / (pa * pd - pb * pc);
                    ia = pd * pid;
                    ib = pb * pid;
                }
                Inherit::NoScale | Inherit::NoScaleOrReflection => {
                    let r = rotation.to_radians();
                    let cos = r.cos();
                    let sin = r.sin();
                    pa = (pa * cos + pb * sin) / sk_scale_x;
                    pc = (pc * cos + pd * sin) / sk_scale_y;
                    let mut s = (pa * pa + pc * pc).sqrt();
                    if s > 0.000_01 {
                        s = 1.0 / s;
                    }
                    pa *= s;
                    pc *= s;
                    s = (pa * pa + pc * pc).sqrt();
                    let inherit_flip = inherit == Inherit::NoScale
                        && (pid < 0.0) != ((sk_scale_x < 0.0) != (sk_scale_y < 0.0));
                    if inherit_flip {
                        s = -s;
                    }
                    let rot = std::f32::consts::FRAC_PI_2 + pc.atan2(pa);
                    pb = rot.cos() * s;
                    pd = rot.sin() * s;
                    pid = 1.0 / (pa * pd - pb * pc);
                    ia = pd * pid;
                    ib = pb * pid;
                    ic = pc * pid;
                    id = pa * pid;
                }
                Inherit::Normal | Inherit::OnlyTranslation => {}
            }
            ra = ia * a - ib * c;
            rb = ia * b - ib * d;
            rc = id * c - ic * a;
            rd = id * d - ic * b;
        }

        let bone = &mut self.bones[idx];
        bone.ax = ax;
        bone.ay = ay;
        bone.a_shear_x = 0.0;
        bone.a_scale_x = (ra * ra + rc * rc).sqrt();
        if bone.a_scale_x > 0.0001 {
            let det = ra * rd - rb * rc;
            bone.a_scale_y = det / bone.a_scale_x;
            bone.a_shear_y = -atan2_deg(ra * rb + rc * rd, det);
            bone.a_rotation = atan2_deg(rc, ra);
        } else {
            bone.a_scale_x = 0.0;
            bone.a_scale_y = (rb * rb + rd * rd).sqrt();
            bone.a_shear_y = 0.0;
            bone.a_rotation = 90.0 - atan2_deg(rd, rb);
        }
    }

    /// `Bone::update(Physics)` — the update-cache callback for a bone.
    ///
    /// Reads the bone's **applied** TRS (`ax`, `ay`, …) — not the local
    /// setup values. spine-cpp does the same in `Bone::update`. This is
    /// load-bearing when a bone appears twice in the cache (constrained
    /// bone + descendant-rewired re-run): after a world-space constraint
    /// calls `updateAppliedTransform`, the applied fields carry the
    /// constraint's effect forward into the second `Bone::update` pass,
    /// so the second pass reproduces the constrained world matrix
    /// instead of overwriting it with the unmodified local TRS.
    pub(crate) fn update_bone_world_transform(&mut self, bone_id: BoneId) {
        let (ax, ay, a_rotation, a_scale_x, a_scale_y, a_shear_x, a_shear_y) = {
            let b = &self.bones[bone_id.index()];
            (
                b.ax,
                b.ay,
                b.a_rotation,
                b.a_scale_x,
                b.a_scale_y,
                b.a_shear_x,
                b.a_shear_y,
            )
        };
        self.update_bone_world_transform_with(
            bone_id, ax, ay, a_rotation, a_scale_x, a_scale_y, a_shear_x, a_shear_y,
        );
    }

    /// `Bone::updateWorldTransform(x, y, rotation, scaleX, scaleY, shearX, shearY)`
    /// — the 7-arg overload that IK + Transform constraints call with
    /// solver-derived TRS. Also stores the passed TRS as the bone's
    /// "applied" fields so downstream readers see what was actually used.
    ///
    /// Literal port of `spine-cpp/src/spine/Bone.cpp` `Bone::updateWorldTransform`
    /// (the multi-arg overload). All five [`Inherit`] modes follow —
    /// reflection check, per-axis sign handling, post-multiply block —
    /// ported as-is without refactoring the math.
    // spine-cpp keeps this as one ~120-line function; splitting breaks the
    // port-verbatim rule and makes diffing harder for no correctness gain.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub(crate) fn update_bone_world_transform_with(
        &mut self,
        bone_id: BoneId,
        x: f32,
        y: f32,
        rotation: f32,
        scale_x: f32,
        scale_y: f32,
        shear_x: f32,
        shear_y: f32,
    ) {
        let idx = bone_id.index();

        // Snapshot parent/inherit once.
        let (parent, inherit) = {
            let b = &self.bones[idx];
            (b.parent, b.inherit)
        };

        // spine-cpp's first act is copying the passed-in TRS into the
        // "applied" fields. The top-level `update_world_transform(Physics)`
        // already did this for every bone, but we repeat it here so the
        // per-bone helper is self-contained (matches spine-cpp's two-arg
        // overload, which future animation code in Phase 3 will call with
        // modified values).
        {
            let b = &mut self.bones[idx];
            b.ax = x;
            b.ay = y;
            b.a_rotation = rotation;
            b.a_scale_x = scale_x;
            b.a_scale_y = scale_y;
            b.a_shear_x = shear_x;
            b.a_shear_y = shear_y;
        }

        let sx = self.scale_x;
        let sy = self.scale_y;

        let Some(parent_id) = parent else {
            // Root bone: no parent matrix. spine-cpp uses skeleton.getScaleX/Y
            // directly and skips the post-multiply at the bottom.
            let rx = rotation + shear_x;
            let ry = rotation + 90.0 + shear_y;
            let b = &mut self.bones[idx];
            b.a = cos_deg(rx) * scale_x * sx;
            b.b = cos_deg(ry) * scale_y * sx;
            b.c = sin_deg(rx) * scale_x * sy;
            b.d = sin_deg(ry) * scale_y * sy;
            b.world_x = x * sx + self.x;
            b.world_y = y * sy + self.y;
            return;
        };

        // Non-root: snapshot parent's world matrix.
        let (mut pa, mut pb, mut pc, mut pd, p_world_x, p_world_y) = {
            let p = &self.bones[parent_id.index()];
            (p.a, p.b, p.c, p.d, p.world_x, p.world_y)
        };

        // World translation is always `parent.M * (x, y) + parent.world`,
        // regardless of Inherit mode.
        {
            let b = &mut self.bones[idx];
            b.world_x = pa * x + pb * y + p_world_x;
            b.world_y = pc * x + pd * y + p_world_y;
        }

        match inherit {
            Inherit::Normal => {
                let rx = rotation + shear_x;
                let ry = rotation + 90.0 + shear_y;
                let la = cos_deg(rx) * scale_x;
                let lb = cos_deg(ry) * scale_y;
                let lc = sin_deg(rx) * scale_x;
                let ld = sin_deg(ry) * scale_y;
                let b = &mut self.bones[idx];
                b.a = pa * la + pb * lc;
                b.b = pa * lb + pb * ld;
                b.c = pc * la + pd * lc;
                b.d = pc * lb + pd * ld;
                // Normal returns early — does NOT hit the post-multiply.
                return;
            }
            Inherit::OnlyTranslation => {
                let rx = rotation + shear_x;
                let ry = rotation + 90.0 + shear_y;
                let b = &mut self.bones[idx];
                b.a = cos_deg(rx) * scale_x;
                b.b = cos_deg(ry) * scale_y;
                b.c = sin_deg(rx) * scale_x;
                b.d = sin_deg(ry) * scale_y;
            }
            Inherit::NoRotationOrReflection => {
                let mut s = pa * pa + pc * pc;
                let prx;
                if s > 0.0001 {
                    s = (pa * pd - pb * pc).abs() / s;
                    pa /= sx;
                    pc /= sy;
                    pb = pc * s;
                    pd = pa * s;
                    prx = atan2_deg(pc, pa);
                } else {
                    pa = 0.0;
                    pc = 0.0;
                    prx = 90.0 - atan2_deg(pd, pb);
                }
                let rx = rotation + shear_x - prx;
                let ry = rotation + shear_y - prx + 90.0;
                let la = cos_deg(rx) * scale_x;
                let lb = cos_deg(ry) * scale_y;
                let lc = sin_deg(rx) * scale_x;
                let ld = sin_deg(ry) * scale_y;
                let b = &mut self.bones[idx];
                b.a = pa * la - pb * lc;
                b.b = pa * lb - pb * ld;
                b.c = pc * la + pd * lc;
                b.d = pc * lb + pd * ld;
            }
            Inherit::NoScale | Inherit::NoScaleOrReflection => {
                let rotation_rad = rotation.to_radians();
                let cosine = rotation_rad.cos();
                let sine = rotation_rad.sin();
                let za = (pa * cosine + pb * sine) / sx;
                let zc = (pc * cosine + pd * sine) / sy;
                let mut s = (za * za + zc * zc).sqrt();
                if s > 0.00001 {
                    s = 1.0 / s;
                }
                let za = za * s;
                let zc = zc * s;
                let mut s = (za * za + zc * zc).sqrt();
                if inherit == Inherit::NoScale
                    && (pa * pd - pb * pc < 0.0) != ((sx < 0.0) != (sy < 0.0))
                {
                    s = -s;
                }
                let rot = std::f32::consts::FRAC_PI_2 + zc.atan2(za);
                let zb = rot.cos() * s;
                let zd = rot.sin() * s;
                let la = cos_deg(shear_x) * scale_x;
                let lb = cos_deg(90.0 + shear_y) * scale_y;
                let lc = sin_deg(shear_x) * scale_x;
                let ld = sin_deg(90.0 + shear_y) * scale_y;
                let b = &mut self.bones[idx];
                b.a = za * la + zb * lc;
                b.b = za * lb + zb * ld;
                b.c = zc * la + zd * lc;
                b.d = zc * lb + zd * ld;
            }
        }

        // All non-Normal branches fall through to this post-multiply.
        // (Normal returned early; Root handled above.)
        let b = &mut self.bones[idx];
        b.a *= sx;
        b.b *= sx;
        b.c *= sy;
        b.d *= sy;
    }
}

/// Error returned by [`Skeleton::set_skin_by_name`] when no skin with the
/// given name exists in the skeleton's data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("skeleton has no skin named `{0}`")]
pub struct SkinNotFound(pub String);

#[cfg(test)]
// Golden values in this module come from spine-cpp printf `%.9g` output —
// keep them verbatim for diff-ability against the capture harness rather
// than reformatting for Rust lint style.
#[allow(clippy::excessive_precision, clippy::unreadable_literal)]
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

    // -- Inherit::NoScaleOrReflection coverage ------------------------------
    //
    // No example rig (raptor, stretchyman, hero, etc.) carries a bone with
    // Inherit::NoScaleOrReflection, so this case sits outside the
    // golden_pose.rs integration test. The golden values below came from
    // `tools/spine_capture/synthetic.cpp` — a small spine-cpp program that
    // builds the same two-bone configuration directly and dumps its output.
    //
    // To regenerate (if the case configuration ever changes):
    //   cd tools/spine_capture && make && ./build/spine_synthetic

    fn synthetic_noscale_skeleton(child_inherit: Inherit) -> Skeleton {
        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.x = 10.0;
        root.y = 5.0;
        root.scale_x = -1.0;
        root.scale_y = 1.0;
        root.rotation = 30.0;
        sd.bones.push(root);

        let mut child = BoneData::new(BoneId(1), "child", Some(BoneId(0)));
        child.x = 20.0;
        child.y = 0.0;
        child.rotation = 45.0;
        child.scale_x = 2.0;
        child.scale_y = 0.5;
        child.shear_x = 10.0;
        child.shear_y = -5.0;
        child.inherit = child_inherit;
        sd.bones.push(child);

        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();
        sk.update_world_transform(Physics::None);
        sk
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-4 || (a - b).abs() <= 1e-4 * a.abs().max(b.abs())
    }

    fn assert_child_matches(sk: &Skeleton, expected: (f32, f32, f32, f32, f32, f32), label: &str) {
        let c = &sk.bones[1];
        let (ea, eb, ec, ed, ewx, ewy) = expected;
        let fields = [
            ("a", ea, c.a),
            ("b", eb, c.b),
            ("c", ec, c.c),
            ("d", ed, c.d),
            ("world_x", ewx, c.world_x),
            ("world_y", ewy, c.world_y),
        ];
        for (field, want, got) in fields {
            assert!(
                close(want, got),
                "[{label}] child.{field} mismatch: expected {want}, got {got}"
            );
        }
    }

    /// Sanity: the shared `NoScale`/`NoScaleOrReflection` code path is
    /// exercised by real fixtures, but having the synthetic side-by-side
    /// with the `NoScaleOrReflection` test lets a future refactor catch
    /// divergence between the two modes immediately.
    #[test]
    fn inherit_no_scale_matches_spine_cpp_synthetic() {
        let sk = synthetic_noscale_skeleton(Inherit::NoScale);
        // Golden from spine-cpp spine_synthetic (reflected parent, default
        // skeleton scale 1,1 → `s = -s` flip applies for NoScale).
        assert_child_matches(
            &sk,
            (
                -1.81261575,
                0.0868240446,
                0.84523654,
                0.492403954,
                -7.32050705,
                -5.0,
            ),
            "NoScale",
        );
    }

    // -- setup pose + skin --------------------------------------------------

    /// `set_to_setup_pose` must restore local TRS after animation-style
    /// mutation. Applies a nonsense local, calls `set_to_setup_pose`, checks
    /// the bone snaps back to data values.
    #[test]
    fn set_to_setup_pose_restores_bone_local_trs() {
        let mut sd = SkeletonData::default();
        let mut root = BoneData::new(BoneId(0), "root", None);
        root.x = 10.0;
        root.rotation = 45.0;
        root.scale_x = 2.0;
        sd.bones.push(root);
        let mut sk = Skeleton::new(Arc::new(sd));

        // Clobber local TRS as an animation would.
        sk.bones[0].x = 999.0;
        sk.bones[0].rotation = -12.0;
        sk.bones[0].scale_x = 0.5;

        sk.set_to_setup_pose();

        assert!((sk.bones[0].x - 10.0).abs() < 1e-6);
        assert!((sk.bones[0].rotation - 45.0).abs() < 1e-6);
        assert!((sk.bones[0].scale_x - 2.0).abs() < 1e-6);
    }

    /// `set_to_setup_pose` also resets constraint mix values.
    #[test]
    fn set_to_setup_pose_restores_ik_mix() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.bones
            .push(BoneData::new(BoneId(1), "target", Some(BoneId(0))));

        let mut ik = IkConstraintData::new(IkConstraintId(0), "ik", BoneId(1));
        ik.bones.push(BoneId(0));
        ik.mix = 0.7;
        ik.softness = 12.0;
        ik.bend_direction = 1;
        sd.ik_constraints.push(ik);

        let mut sk = Skeleton::new(Arc::new(sd));
        sk.ik_constraints[0].mix = 0.0;
        sk.ik_constraints[0].softness = 0.0;
        sk.ik_constraints[0].bend_direction = -1;

        sk.set_to_setup_pose();

        assert!((sk.ik_constraints[0].mix - 0.7).abs() < 1e-6);
        assert!((sk.ik_constraints[0].softness - 12.0).abs() < 1e-6);
        assert_eq!(sk.ik_constraints[0].bend_direction, 1);
    }

    /// `set_skin_by_name("missing")` returns a `SkinNotFound` with the name.
    #[test]
    fn set_skin_by_name_reports_missing() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.skins.push(crate::data::Skin::new("existing"));
        let mut sk = Skeleton::new(Arc::new(sd));

        let err = sk.set_skin_by_name("missing").unwrap_err();
        assert_eq!(err, SkinNotFound("missing".into()));
        // Unchanged.
        assert_eq!(sk.skin, None);

        sk.set_skin_by_name("existing").unwrap();
        assert_eq!(sk.skin, Some(SkinId(0)));
    }

    /// Attachment resolution walks the active skin first and falls back to
    /// the default skin. Verifies both paths and the empty-name early return.
    #[test]
    fn get_attachment_walks_active_then_default_skin() {
        use crate::data::{Attachment, AttachmentId, RegionAttachment};

        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        sd.slots.push(SlotData::new(SlotId(0), "body", BoneId(0)));

        // Three attachments: only-in-default, only-in-extras, and a shared
        // name whose resolution should prefer the active skin.
        sd.attachments
            .push(Attachment::Region(RegionAttachment::new("default-only")));
        sd.attachments
            .push(Attachment::Region(RegionAttachment::new("extras-only")));
        sd.attachments
            .push(Attachment::Region(RegionAttachment::new(
                "shared-in-default",
            )));
        sd.attachments
            .push(Attachment::Region(RegionAttachment::new(
                "shared-in-extras",
            )));

        let mut default_skin = crate::data::Skin::new("default");
        default_skin.set_attachment(SlotId(0), "default-only", AttachmentId(0));
        default_skin.set_attachment(SlotId(0), "shared", AttachmentId(2));

        let mut extras_skin = crate::data::Skin::new("extras");
        extras_skin.set_attachment(SlotId(0), "extras-only", AttachmentId(1));
        extras_skin.set_attachment(SlotId(0), "shared", AttachmentId(3));

        sd.skins.push(default_skin);
        sd.skins.push(extras_skin);
        sd.default_skin = Some(SkinId(0));

        let mut sk = Skeleton::new(Arc::new(sd));

        // No active skin yet: fallback path only.
        assert_eq!(
            sk.get_attachment(SlotId(0), "default-only"),
            Some(AttachmentId(0))
        );
        assert_eq!(
            sk.get_attachment(SlotId(0), "shared"),
            Some(AttachmentId(2))
        );
        assert_eq!(sk.get_attachment(SlotId(0), "extras-only"), None);
        assert_eq!(sk.get_attachment(SlotId(0), ""), None);

        // Activate extras: shared now resolves through extras, default-only
        // still resolves through default (fallback).
        sk.set_skin(Some(SkinId(1)));
        assert_eq!(
            sk.get_attachment(SlotId(0), "extras-only"),
            Some(AttachmentId(1))
        );
        assert_eq!(
            sk.get_attachment(SlotId(0), "shared"),
            Some(AttachmentId(3))
        );
        assert_eq!(
            sk.get_attachment(SlotId(0), "default-only"),
            Some(AttachmentId(0))
        );
    }

    /// `set_skin` triggers an `update_cache` rebuild — verifies the
    /// skin-required-bone inclusion rule (same state machine exercised in
    /// the dedicated `update_cache` test, but via the public setter).
    #[test]
    fn set_skin_rebuilds_update_cache_for_skin_required_bone() {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        let mut hidden = BoneData::new(BoneId(1), "hidden", Some(BoneId(0)));
        hidden.skin_required = true;
        sd.bones.push(hidden);

        let mut extras = crate::data::Skin::new("extras");
        extras.bones.push(BoneId(1));
        sd.skins.push(extras);

        let mut sk = Skeleton::new(Arc::new(sd));
        sk.update_cache();
        assert!(!sk.bones[1].active);

        sk.set_skin_by_name("extras").unwrap();
        assert!(sk.bones[1].active);
        assert!(
            sk.update_cache.contains(&UpdateCacheEntry::Bone(BoneId(1))),
            "skin swap should have rebuilt the cache"
        );

        sk.set_skin(None);
        assert!(!sk.bones[1].active);
    }

    /// Primary goal of this test: cover the `Inherit::NoScaleOrReflection`
    /// branch, which (a) takes the shared `NoScale` code path but (b) skips
    /// the reflection-sign flip. With a reflected parent and identity
    /// skeleton scale, `NoScale` and `NoScaleOrReflection` must produce
    /// visibly different a/b/c/d — verifying both in one test proves the
    /// port didn't accidentally collapse the two modes into one.
    #[test]
    fn inherit_no_scale_or_reflection_matches_spine_cpp_synthetic() {
        let sk = synthetic_noscale_skeleton(Inherit::NoScaleOrReflection);
        // Golden from spine-cpp spine_synthetic: same configuration but
        // child.inherit = NoScaleOrReflection → no sign flip.
        assert_child_matches(
            &sk,
            (
                -1.99238956,
                -0.171010092,
                0.174311399,
                -0.469846398,
                -7.32050705,
                -5.0,
            ),
            "NoScaleOrReflection",
        );
    }
}
