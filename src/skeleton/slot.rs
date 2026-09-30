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

//! Runtime slots and the draw order.

use crate::data::{AttachmentId, BoneId, SkeletonData, SlotData, SlotId};
use crate::math::Color;
use crate::skeleton::pose::{Pose, Posed};

#[derive(Debug, Clone, PartialEq)]
pub struct SlotPose {
    pub color: Color,
    /// Meaningful only when `has_dark_color`.
    pub dark_color: Color,
    pub has_dark_color: bool,
    pub attachment: Option<AttachmentId>,
    /// Sequence frame, or -1 for the sequence's setup frame.
    pub sequence_index: i32,
    /// Deformed vertices for the attachment. Empty means undeformed.
    pub deform: Vec<f32>,
}

impl Default for SlotPose {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            dark_color: Color::new(0.0, 0.0, 0.0, 0.0),
            has_dark_color: false,
            attachment: None,
            sequence_index: 0,
            deform: Vec::new(),
        }
    }
}

impl Pose for SlotPose {
    fn set_from(&mut self, other: &Self) {
        self.color = other.color;
        if other.has_dark_color {
            self.dark_color = other.dark_color;
        }
        self.has_dark_color = other.has_dark_color;
        self.attachment = other.attachment;
        self.sequence_index = other.sequence_index;
        self.deform.clone_from(&other.deform);
    }
}

impl SlotPose {
    /// Changes the attachment, keeping deform only when both attachments
    /// share timelines. Resets the sequence frame.
    pub fn set_attachment(&mut self, attachment: Option<AttachmentId>, data: &SkeletonData) {
        if self.attachment == attachment {
            return;
        }
        let timeline_of = |id: AttachmentId| data.attachments[id.index()].timeline_attachment(id);
        match (attachment, self.attachment) {
            (Some(new), Some(old)) if timeline_of(new) == timeline_of(old) => {}
            _ => self.deform.clear(),
        }
        self.attachment = attachment;
        self.sequence_index = -1;
    }
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub data: SlotId,
    pub bone: BoneId,
    pub posed: Posed<SlotPose>,
    /// Scratch state `AnimationState` uses while applying attachment timelines.
    pub(crate) attachment_state: i32,
}

impl Slot {
    #[must_use]
    pub fn new(data: &SlotData) -> Self {
        let mut pose = SlotPose::default();
        pose.has_dark_color = data.dark_color.is_some();
        Self {
            data: data.index,
            bone: data.bone,
            posed: Posed::new(pose.clone(), pose),
            attachment_state: 0,
        }
    }

    #[inline]
    #[must_use]
    pub fn applied(&self) -> &SlotPose {
        self.posed.applied()
    }

    /// Resets color, dark color and attachment. `attachment` is the setup
    /// attachment resolved through the skeleton's skins.
    pub fn setup_pose(
        &mut self,
        data: &SlotData,
        attachment: Option<AttachmentId>,
        skeleton_data: &SkeletonData,
    ) {
        let pose = &mut self.posed.pose;
        pose.color = data.color;
        if pose.has_dark_color
            && let Some(dark) = data.dark_color
        {
            pose.dark_color = dark;
        }
        pose.sequence_index = 0;
        if data.attachment_name.is_none() {
            pose.set_attachment(None, skeleton_data);
        } else {
            pose.attachment = None;
            pose.set_attachment(attachment, skeleton_data);
        }
    }
}

/// Slot render order. Draw order timelines write `pose`; sliders may
/// constrain it.
#[derive(Debug, Clone, Default)]
pub struct DrawOrder {
    pub pose: Vec<SlotId>,
    pub constrained: Vec<SlotId>,
    is_constrained: bool,
}

impl DrawOrder {
    #[must_use]
    pub fn applied(&self) -> &[SlotId] {
        if self.is_constrained {
            &self.constrained
        } else {
            &self.pose
        }
    }

    pub fn select_mut(&mut self, applied: bool) -> &mut Vec<SlotId> {
        if applied && self.is_constrained {
            &mut self.constrained
        } else {
            &mut self.pose
        }
    }

    pub fn setup_pose(&mut self, slot_count: usize) {
        self.pose.clear();
        self.pose.extend((0..slot_count).map(|i| SlotId(i as u16)));
    }

    pub(crate) fn constrain(&mut self) {
        self.is_constrained = true;
    }

    pub(crate) fn unconstrain(&mut self) {
        self.is_constrained = false;
    }

    #[must_use]
    pub fn is_constrained(&self) -> bool {
        self.is_constrained
    }

    pub(crate) fn reset(&mut self) {
        self.constrained.clone_from(&self.pose);
    }
}
