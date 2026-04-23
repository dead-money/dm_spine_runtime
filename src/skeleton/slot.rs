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

//! Runtime-mutable slot state. Mirrors `spine::Slot` from
//! `spine-cpp/include/spine/Slot.h`: tint color, dark color, active
//! attachment, and a per-vertex deform scratch buffer.

use crate::data::{AttachmentId, SlotData, SlotId};
use crate::math::Color;

/// Runtime-mutable slot. One per [`SlotData`] in the owning `Skeleton`.
///
/// The slot's bone link is immutable — read it from `data.slots[idx].bone`
/// when needed rather than caching a second copy here.
#[derive(Debug, Clone)]
pub struct Slot {
    /// Index into [`SkeletonData::slots`][crate::data::SkeletonData::slots].
    pub data_index: SlotId,

    /// Current tint color. Animations / `setToSetupPose` both write here.
    pub color: Color,
    /// "Tint-black" secondary color; `None` when the slot was exported without
    /// dark-color support (matches [`SlotData::dark_color`]).
    pub dark_color: Option<Color>,

    /// Resolved attachment for the current skin, or `None` for an empty slot.
    /// Set by `Skeleton::set_slots_to_setup_pose` via the active skin +
    /// default-skin fallback; animations change it through `AttachmentTimeline`.
    pub attachment: Option<AttachmentId>,

    /// Counter used by `AttachmentTimeline` to invalidate cached deform data
    /// when the attachment changes mid-animation. Ported verbatim from
    /// `spine-cpp`.
    pub attachment_state: i32,

    /// Active sequence frame for attachments that carry a [`Sequence`]. `-1`
    /// means "use the attachment's default frame" (spine-cpp convention).
    ///
    /// [`Sequence`]: crate::data::Sequence
    pub sequence_index: i32,

    /// Scratch buffer written by `DeformTimeline` each frame, consumed by the
    /// mesh/path attachment world-vertex computation. Empty in setup pose.
    pub deform: Vec<f32>,
}

impl Slot {
    /// Build a runtime slot initialised to `data`'s setup pose.
    ///
    /// Attachment resolution requires the active skin and so is not done
    /// here — `Skeleton::set_slots_to_setup_pose` handles it.
    #[must_use]
    pub fn new(data: &SlotData) -> Self {
        Self {
            data_index: data.index,
            color: data.color,
            dark_color: data.dark_color,
            attachment: None,
            attachment_state: 0,
            sequence_index: -1,
            deform: Vec::new(),
        }
    }

    /// Reset mutable state to `data`'s setup pose. Leaves attachment
    /// resolution to the caller — see `Skeleton::set_slots_to_setup_pose`.
    pub fn set_to_setup_pose(&mut self, data: &SlotData) {
        debug_assert_eq!(data.index, self.data_index);
        self.color = data.color;
        self.dark_color = data.dark_color;
        self.deform.clear();
    }
}
