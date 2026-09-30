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

//! `Skeleton`: the per-instance pose of a [`SkeletonData`].

use std::sync::Arc;

use crate::data::skin::resolve;
use crate::data::{
    Attachment, AttachmentId, AttachmentRef, BoneId, ConstraintData, ConstraintId, SkeletonData,
    Skin, SkinKey, SlotId,
};
use crate::math::Color;
use crate::skeleton::Physics;
use crate::skeleton::bone::{self, Bone, Frame};
use crate::skeleton::constraint::Constraint;
use crate::skeleton::slot::{DrawOrder, Slot};
use crate::skeleton::update_cache::{ResetEntry, UpdateCacheEntry};

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub(crate) data: Arc<SkeletonData>,
    pub bones: Vec<Bone>,
    pub slots: Vec<Slot>,
    pub draw_order: DrawOrder,
    /// Same order as [`SkeletonData::constraints`].
    pub constraints: Vec<Constraint>,
    /// Parallel to [`Self::constraints`]; inactive constraints don't update.
    pub constraints_active: Vec<bool>,
    pub(crate) physics: Vec<ConstraintId>,
    pub(crate) update_cache: Vec<UpdateCacheEntry>,
    pub(crate) reset_cache: Vec<ResetEntry>,
    pub(crate) skin: Option<Arc<Skin>>,
    pub color: Color,
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub wind_x: f32,
    pub wind_y: f32,
    pub gravity_x: f32,
    pub gravity_y: f32,
    /// Seconds, advanced by [`Self::update`]; drives physics.
    pub time: f32,
    pub(crate) update: u32,
}

impl Skeleton {
    /// A skeleton in its setup pose with its update cache built.
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        let mut bones: Vec<Bone> = data.bones.iter().map(Bone::new).collect();
        for i in 0..bones.len() {
            if let Some(parent) = bones[i].parent {
                bones[parent.index()].children.push(BoneId(i as u16));
            }
        }
        let slots = data.slots.iter().map(Slot::new).collect();
        let constraints: Vec<Constraint> = data.constraints.iter().map(Constraint::new).collect();
        let physics = data
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, ConstraintData::Physics(_)))
            .map(|(i, _)| ConstraintId(i as u16))
            .collect();
        let mut skeleton = Self {
            constraints_active: vec![false; constraints.len()],
            bones,
            slots,
            draw_order: DrawOrder::default(),
            constraints,
            physics,
            update_cache: Vec::new(),
            reset_cache: Vec::new(),
            skin: None,
            color: Color::WHITE,
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            wind_x: 1.0,
            wind_y: 0.0,
            gravity_x: 0.0,
            gravity_y: 1.0,
            time: 0.0,
            update: 0,
            data,
        };
        skeleton.draw_order.setup_pose(skeleton.slots.len());
        skeleton.setup_pose_slots();
        skeleton.update_cache();
        skeleton
    }

    #[must_use]
    pub fn data(&self) -> &Arc<SkeletonData> {
        &self.data
    }

    #[inline]
    #[must_use]
    pub(crate) fn frame(&self) -> Frame {
        Frame {
            x: self.x,
            y: self.y,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            update: self.update,
        }
    }

    /// Rebuilds the update order. Call after changing the skin or which
    /// bones and constraints are active.
    pub fn update_cache(&mut self) {
        let data = Arc::clone(&self.data);
        self.update_cache.clear();
        self.reset_cache.clear();

        self.draw_order.unconstrain();
        for slot in &mut self.slots {
            slot.posed.unconstrain();
        }

        for (bone, bone_data) in self.bones.iter_mut().zip(&data.bones) {
            bone.sorted = bone_data.skin_required;
            bone.active = !bone.sorted;
            bone.posed.unconstrain();
        }
        if let Some(skin) = &self.skin {
            for &b in &skin.bones {
                let mut bone = Some(b);
                while let Some(id) = bone {
                    let bone_ref = &mut self.bones[id.index()];
                    bone_ref.sorted = false;
                    bone_ref.active = true;
                    bone = bone_ref.parent;
                }
            }
        }

        for c in &mut self.constraints {
            c.unconstrain();
        }
        for i in 0..self.constraints.len() {
            let constraint_data = &data.constraints[i];
            let id = ConstraintId(i as u16);
            let active = self.is_source_active(constraint_data)
                && (!constraint_data.skin_required()
                    || self
                        .skin
                        .as_ref()
                        .is_some_and(|s| s.constraints.contains(&id)));
            self.constraints_active[i] = active;
            if active {
                self.sort_constraint(id, &data);
            }
        }

        for i in 0..self.bones.len() {
            self.sort_bone(BoneId(i as u16));
        }
    }

    fn is_source_active(&self, data: &ConstraintData) -> bool {
        match data {
            ConstraintData::Ik(d) => self.bones[d.target.index()].active,
            ConstraintData::Transform(d) => self.bones[d.source.index()].active,
            ConstraintData::Path(d) => {
                let slot_bone = self.slots[d.slot.index()].bone;
                self.bones[slot_bone.index()].active
            }
            ConstraintData::Physics(d) => self.bones[d.bone.index()].active,
            ConstraintData::Slider(d) => d.bone.is_none_or(|b| self.bones[b.index()].active),
        }
    }

    fn sort_constraint(&mut self, id: ConstraintId, data: &SkeletonData) {
        match &data.constraints[id.index()] {
            ConstraintData::Ik(d) => self.sort_ik(id, d),
            ConstraintData::Transform(d) => self.sort_transform(id, d),
            ConstraintData::Path(d) => self.sort_path(id, d, data),
            ConstraintData::Physics(d) => self.sort_physics(id, d),
            ConstraintData::Slider(d) => self.sort_slider(id, d, data),
        }
    }

    pub(crate) fn sort_bone(&mut self, id: BoneId) {
        let bone = &self.bones[id.index()];
        if bone.sorted || !bone.active {
            return;
        }
        if let Some(parent) = bone.parent {
            self.sort_bone(parent);
        }
        self.bones[id.index()].sorted = true;
        self.update_cache.push(UpdateCacheEntry::Bone(id));
    }

    /// Marks active sorted descendants unsorted so they re-sort after a
    /// constraint.
    pub(crate) fn sort_reset(&mut self, parent: BoneId) {
        for c in 0..self.bones[parent.index()].children.len() {
            let child = self.bones[parent.index()].children[c];
            let bone = &self.bones[child.index()];
            if bone.active {
                if bone.sorted {
                    self.sort_reset(child);
                }
                self.bones[child.index()].sorted = false;
            }
        }
    }

    pub(crate) fn constrain_bone(&mut self, id: BoneId) {
        let posed = &mut self.bones[id.index()].posed;
        if !posed.is_constrained() {
            posed.constrain();
            self.reset_cache.push(ResetEntry::Bone(id));
        }
    }

    pub(crate) fn constrain_slot(&mut self, id: SlotId) {
        let posed = &mut self.slots[id.index()].posed;
        if !posed.is_constrained() {
            posed.constrain();
            self.reset_cache.push(ResetEntry::Slot(id));
        }
    }

    pub(crate) fn constrain_constraint(&mut self, id: ConstraintId) {
        if self.constraints[id.index()].constrain() {
            self.reset_cache.push(ResetEntry::Constraint(id));
        }
    }

    /// Poses every bone and runs constraints in update order.
    pub fn update_world_transform(&mut self, physics: Physics) {
        self.update = self.update.wrapping_add(1);
        if self.update == 0 {
            // 0 marks "never", so skip it on wraparound.
            self.update = 1;
        }
        if self.draw_order.is_constrained() {
            self.draw_order.reset();
        }
        for i in 0..self.reset_cache.len() {
            match self.reset_cache[i] {
                ResetEntry::Bone(id) => self.bones[id.index()].posed.reset_constrained(),
                ResetEntry::Slot(id) => self.slots[id.index()].posed.reset_constrained(),
                ResetEntry::Constraint(id) => self.constraints[id.index()].reset_constrained(),
            }
        }
        let frame = self.frame();
        let data = Arc::clone(&self.data);
        for i in 0..self.update_cache.len() {
            match self.update_cache[i] {
                UpdateCacheEntry::Bone(id) => {
                    bone::update_bone(&mut self.bones, id.index(), &frame);
                }
                UpdateCacheEntry::Constraint(id) => match &data.constraints[id.index()] {
                    ConstraintData::Ik(d) => self.update_ik(id, d, &data),
                    ConstraintData::Transform(d) => self.update_transform(id, d),
                    ConstraintData::Path(d) => self.update_path(id, d, &data),
                    ConstraintData::Physics(d) => self.update_physics(id, d, physics),
                    ConstraintData::Slider(d) => self.update_slider(id, d, &data),
                },
            }
        }
    }

    pub fn setup_pose(&mut self) {
        self.setup_pose_bones();
        self.setup_pose_slots();
    }

    pub fn setup_pose_bones(&mut self) {
        let data = Arc::clone(&self.data);
        for (bone, bone_data) in self.bones.iter_mut().zip(&data.bones) {
            bone.setup_pose(bone_data);
        }
        for (c, c_data) in self.constraints.iter_mut().zip(&data.constraints) {
            c.setup_pose(c_data);
        }
    }

    pub fn setup_pose_slots(&mut self) {
        let data = Arc::clone(&self.data);
        self.draw_order.setup_pose(self.slots.len());
        for i in 0..self.slots.len() {
            let slot_data = &data.slots[i];
            let attachment = slot_data
                .attachment_key
                .and_then(|key| self.get_attachment_by_key(key));
            let timeline = self.timeline_attachment(attachment);
            self.slots[i].setup_pose(slot_data, attachment, timeline);
        }
    }

    #[must_use]
    pub fn find_bone(&self, name: &str) -> Option<BoneId> {
        self.data
            .bones
            .iter()
            .position(|b| b.name == name)
            .map(|i| BoneId(i as u16))
    }

    #[must_use]
    pub fn find_slot(&self, name: &str) -> Option<SlotId> {
        self.data
            .slots
            .iter()
            .position(|s| s.name == name)
            .map(|i| SlotId(i as u16))
    }

    /// The skin being worn, if any.
    #[must_use]
    pub fn skin(&self) -> Option<&Arc<Skin>> {
        self.skin.as_ref()
    }

    /// The worn skin, for editing in place; cloned first if other skeletons
    /// share it. Slots keep showing what they showed, as with any skin edit.
    /// Call [`Self::update_cache`] after changing its bones or constraints.
    pub fn skin_mut(&mut self) -> Option<&mut Skin> {
        self.skin.as_mut().map(Arc::make_mut)
    }

    /// Drops owned attachments of the worn skin that neither the skin nor
    /// any slot uses anymore, which replacing or removing entries leaves
    /// behind.
    pub fn compact_skin(&mut self) {
        let Some(skin) = self.skin.as_mut() else {
            return;
        };
        let skin = Arc::make_mut(skin);
        let mut keep = vec![false; skin.owned().len()];
        for slot in &self.slots {
            for pose in [&slot.posed.pose, &slot.posed.constrained] {
                if let Some(AttachmentRef::Owned(i)) = pose.attachment {
                    keep[i as usize] = true;
                }
            }
        }
        let remap = skin.compact(&mut keep);
        for slot in &mut self.slots {
            for pose in [&mut slot.posed.pose, &mut slot.posed.constrained] {
                // Shown attachments were marked, so they always remap.
                if let Some(AttachmentRef::Owned(i)) = &mut pose.attachment
                    && let Some(new) = remap[*i as usize]
                {
                    *i = new;
                }
            }
        }
    }

    /// The attachment `r` refers to. Owned attachments come from the worn
    /// skin.
    ///
    /// # Panics
    ///
    /// If `r` is owned and not in the worn skin.
    #[inline]
    #[must_use]
    pub fn attachment(&self, r: AttachmentRef) -> &Attachment {
        resolve(&self.data.attachments, self.skin.as_deref(), r)
    }

    /// The attachment a slot shows in its applied pose.
    #[must_use]
    pub fn slot_attachment(&self, slot: SlotId) -> Option<&Attachment> {
        self.slots[slot.index()]
            .applied()
            .attachment
            .map(|r| self.attachment(r))
    }

    /// The data attachment whose timelines drive `r`.
    #[must_use]
    pub fn timeline_attachment(&self, r: Option<AttachmentRef>) -> Option<AttachmentId> {
        r.and_then(|r| self.attachment(r).timeline_attachment(r))
    }

    /// The attachment for a placeholder on a slot, from the skin, falling
    /// back to the default skin.
    #[must_use]
    pub fn get_attachment(&self, slot: SlotId, placeholder: &str) -> Option<AttachmentRef> {
        if placeholder.is_empty() {
            return None;
        }
        match self.data.skin_keys.find(slot, placeholder) {
            Some(key) => self.get_attachment_by_key(key),
            None => self
                .skin
                .as_ref()?
                .extra_on(slot)
                .find(|(n, _)| *n == placeholder)
                .map(|(_, a)| a),
        }
    }

    /// [`Self::get_attachment`] for an interned placeholder.
    #[inline]
    #[must_use]
    pub fn get_attachment_by_key(&self, key: SkinKey) -> Option<AttachmentRef> {
        if let Some(skin) = &self.skin
            && let Some(att) = skin.get(key)
        {
            return Some(att);
        }
        self.data.default_skin()?.get(key)
    }

    /// Shows the attachment for `placeholder` on a slot, or clears it with
    /// `None`. Does nothing if the placeholder has no attachment.
    pub fn set_attachment(&mut self, slot: SlotId, placeholder: Option<&str>) {
        let attachment = match placeholder {
            Some(p) => match self.get_attachment(slot, p) {
                Some(a) => Some(a),
                None => return,
            },
            None => None,
        };
        let timeline = self.timeline_attachment(attachment);
        self.slots[slot.index()]
            .posed
            .pose
            .set_attachment(attachment, timeline);
    }

    /// Changes the skin. Slots showing the old skin's attachments switch to
    /// the new skin's for the same placeholder; with no old skin, setup
    /// attachments come from the new one. A slot left showing an attachment
    /// the old skin owned is cleared, since that attachment goes with it.
    pub fn set_skin(&mut self, new_skin: Option<Arc<Skin>>) {
        let same = match (&self.skin, &new_skin) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        let data = Arc::clone(&self.data);
        let old_skin = std::mem::replace(&mut self.skin, new_skin);
        let new = self.skin.clone();
        for i in 0..self.slots.len() {
            let slot = SlotId(i as u16);
            let Some(current) = self.slots[i].posed.pose.attachment else {
                if old_skin.is_none()
                    && let Some(new) = &new
                    && let Some(key) = data.slots[i].attachment_key
                    && let Some(att) = new.get(key)
                {
                    self.show_new_skin_attachment(i, att);
                }
                continue;
            };
            let replacement = match (&old_skin, &new) {
                (None, Some(new)) => data.slots[i].attachment_key.and_then(|key| new.get(key)),
                (Some(old), Some(new)) => {
                    let keyed = data
                        .skin_keys
                        .slot_keys(slot)
                        .iter()
                        .find(|&&k| old.get(k) == Some(current))
                        .map(|&k| new.get(k));
                    match keyed {
                        Some(found) => found,
                        None => old
                            .extra_on(slot)
                            .find(|(_, a)| *a == current)
                            .and_then(|(n, _)| new.get_named(&data.skin_keys, slot, n)),
                    }
                }
                (_, None) => None,
            };
            match replacement {
                Some(att) => self.show_new_skin_attachment(i, att),
                None if matches!(current, AttachmentRef::Owned(_)) => {
                    self.slots[i].posed.pose.set_attachment(None, None);
                }
                None => {}
            }
        }
        self.update_cache();
    }

    fn show_new_skin_attachment(&mut self, slot: usize, att: AttachmentRef) {
        let timeline = self.timeline_attachment(Some(att));
        let pose = &mut self.slots[slot].posed.pose;
        if matches!(att, AttachmentRef::Owned(_)) {
            pose.replace_attachment(Some(att), timeline);
        } else {
            pose.set_attachment(Some(att), timeline);
        }
    }

    /// Wears one of the data's skins.
    ///
    /// # Errors
    /// [`SkinNotFound`] if the data has no skin with that name.
    pub fn set_skin_by_name(&mut self, name: &str) -> Result<(), SkinNotFound> {
        let skin = self
            .data
            .find_skin(name)
            .cloned()
            .ok_or_else(|| SkinNotFound(name.to_string()))?;
        self.set_skin(Some(skin));
        Ok(())
    }

    /// Advances [`Self::time`], which physics uses.
    pub fn update(&mut self, delta: f32) {
        self.time += delta;
    }

    /// Moves every physics simulation without adding inertia.
    pub fn physics_translate(&mut self, x: f32, y: f32) {
        for i in 0..self.physics.len() {
            if let Constraint::Physics(p) = &mut self.constraints[self.physics[i].index()] {
                p.translate(x, y);
            }
        }
    }

    /// Rotates every physics simulation around `(x, y)` without adding inertia.
    pub fn physics_rotate(&mut self, x: f32, y: f32, degrees: f32) {
        for i in 0..self.physics.len() {
            if let Constraint::Physics(p) = &mut self.constraints[self.physics[i].index()] {
                p.rotate(x, y, degrees);
            }
        }
    }

    /// Recomputes a bone's local transform if a constraint left it stale.
    pub fn validate_local_transform(&mut self, bone: BoneId) {
        let f = self.frame();
        bone::validate_local_transform(&mut self.bones, bone.index(), &f);
    }

    /// Update order, for tests and debugging.
    #[must_use]
    pub fn update_cache_entries(&self) -> &[UpdateCacheEntry] {
        &self.update_cache
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("skeleton has no skin named `{0}`")]
pub struct SkinNotFound(pub String);
