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

//! Setup-pose slot data. Slots position attachments relative to a bone and
//! carry the slot's tint color and blend mode.

use crate::data::{BoneId, SlotId};
use crate::math::Color;

/// Rendering blend mode for a slot's attachment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Additive,
    Multiply,
    Screen,
}

/// Immutable setup-pose slot, owned by [`SkeletonData`].
///
/// [`SkeletonData`]: crate::data::SkeletonData
#[derive(Debug, Clone, PartialEq)]
pub struct SlotData {
    pub index: SlotId,
    pub name: String,
    /// Bone this slot is parented to.
    pub bone: BoneId,
    pub color: Color,
    /// "Tint-black" secondary color, applied only when enabled on export.
    pub dark_color: Option<Color>,
    /// Name of the attachment shown in the setup pose — `None` means no
    /// attachment is visible by default.
    pub attachment_name: Option<String>,
    pub blend_mode: BlendMode,

    // Non-essential.
    pub visible: bool,
    /// Editor-only hint for the attachment path; unused at runtime.
    pub path: String,
}

impl SlotData {
    #[must_use]
    pub fn new(index: SlotId, name: impl Into<String>, bone: BoneId) -> Self {
        Self {
            index,
            name: name.into(),
            bone,
            color: Color::WHITE,
            dark_color: None,
            attachment_name: None,
            blend_mode: BlendMode::Normal,
            visible: true,
            path: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_slot_defaults() {
        let s = SlotData::new(SlotId(3), "body", BoneId(1));
        assert_eq!(s.color, Color::WHITE);
        assert!(s.dark_color.is_none());
        assert!(s.attachment_name.is_none());
        assert_eq!(s.blend_mode, BlendMode::Normal);
        assert!(s.visible);
    }
}
