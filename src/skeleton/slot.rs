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

use crate::data::{AttachmentId, AttachmentRef, BoneId, SlotData, SlotId};
use crate::math::Color;
use crate::skeleton::pose::{Pose, Posed};

/// A slot's animatable state: color, attachment, sequence frame and deform.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotPose {
    /// Tint multiplied into the attachment's color when rendering.
    pub color: Color,
    /// Two-color tint dark color; alpha is unused. Meaningful only when
    /// `has_dark_color`.
    pub dark_color: Color,
    /// Set from the slot data; true when the slot uses two-color tinting.
    pub has_dark_color: bool,
    /// The shown attachment. Set it with [`Self::set_attachment`] or
    /// [`Skeleton::set_attachment`](crate::skeleton::Skeleton::set_attachment)
    /// so deform and sequence state stay consistent.
    pub attachment: Option<AttachmentRef>,
    /// The data attachment whose timelines drive `attachment`.
    pub timeline_attachment: Option<AttachmentId>,
    /// Sequence frame, or -1 for the sequence's setup frame.
    pub sequence_index: i32,
    /// Deformed local vertices for the attachment, replacing its vertices
    /// (unweighted) or offsetting them (weighted). Empty means undeformed.
    pub deform: Vec<f32>,
}

impl Default for SlotPose {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            dark_color: Color::new(0.0, 0.0, 0.0, 0.0),
            has_dark_color: false,
            attachment: None,
            timeline_attachment: None,
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
        self.timeline_attachment = other.timeline_attachment;
        self.sequence_index = other.sequence_index;
        self.deform.clone_from(&other.deform);
    }
}

impl SlotPose {
    /// Changes the attachment, keeping deform only when both attachments
    /// share timelines. Resets the sequence frame. `timeline` is the new
    /// attachment's [`timeline_attachment`](crate::data::Attachment::timeline_attachment).
    pub fn set_attachment(
        &mut self,
        attachment: Option<AttachmentRef>,
        timeline: Option<AttachmentId>,
    ) {
        if self.attachment == attachment {
            return;
        }
        self.replace_attachment(attachment, timeline);
    }

    /// [`Self::set_attachment`] without the same-ref shortcut, for refs from
    /// a different skin.
    pub(crate) fn replace_attachment(
        &mut self,
        attachment: Option<AttachmentRef>,
        timeline: Option<AttachmentId>,
    ) {
        if attachment.is_none() || timeline.is_none() || timeline != self.timeline_attachment {
            self.deform.clear();
        }
        self.attachment = attachment;
        self.timeline_attachment = timeline;
        self.sequence_index = -1;
    }
}

/// A slot instance: which bone it follows and its current pose.
#[derive(Debug, Clone)]
pub struct Slot {
    /// This slot's id, also its index in [`SkeletonData::slots`](crate::data::SkeletonData::slots).
    pub data: SlotId,
    /// The bone whose world transform positions the attachment.
    pub bone: BoneId,
    pub posed: Posed<SlotPose>,
    /// Scratch state `AnimationState` uses while applying attachment timelines.
    pub(crate) attachment_state: i32,
}

impl Slot {
    /// A slot with a default pose. [`Self::setup_pose`] applies the data's
    /// setup values.
    #[must_use]
    pub fn new(data: &SlotData) -> Self {
        let pose = SlotPose {
            has_dark_color: data.dark_color.is_some(),
            ..SlotPose::default()
        };
        Self {
            data: data.index,
            bone: data.bone,
            posed: Posed::new(pose.clone(), pose),
            attachment_state: 0,
        }
    }

    /// The pose to render: constrained if a slider writes this slot.
    #[inline]
    #[must_use]
    pub fn applied(&self) -> &SlotPose {
        self.posed.applied()
    }

    /// Resets color, dark color, sequence frame and attachment. `attachment`
    /// is the setup attachment resolved through the skeleton's skins, and
    /// `timeline` its timeline attachment.
    pub fn setup_pose(
        &mut self,
        data: &SlotData,
        attachment: Option<AttachmentRef>,
        timeline: Option<AttachmentId>,
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
            pose.set_attachment(None, None);
        } else {
            pose.attachment = None;
            pose.set_attachment(attachment, timeline);
        }
    }
}

/// Slot render order. Draw order timelines write `pose`; sliders may
/// constrain it, in which case `constrained` is the applied order.
#[derive(Debug, Clone, Default)]
pub struct DrawOrder {
    /// Slots back to front.
    pub pose: Vec<SlotId>,
    /// Copy of `pose` reset each `update_world_transform`, written by
    /// sliders. Meaningful only while [`Self::is_constrained`].
    pub constrained: Vec<SlotId>,
    is_constrained: bool,
}

impl DrawOrder {
    /// The order to render, back to front.
    #[must_use]
    pub fn applied(&self) -> &[SlotId] {
        if self.is_constrained {
            &self.constrained
        } else {
            &self.pose
        }
    }

    /// The applied order if `applied`, else `pose`.
    pub fn select_mut(&mut self, applied: bool) -> &mut Vec<SlotId> {
        if applied && self.is_constrained {
            &mut self.constrained
        } else {
            &mut self.pose
        }
    }

    /// Resets `pose` to slot order.
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

    /// Whether a slider writes the draw order.
    #[must_use]
    pub fn is_constrained(&self) -> bool {
        self.is_constrained
    }

    pub(crate) fn reset(&mut self) {
        self.constrained.clone_from(&self.pose);
    }
}
