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

//! Named set of attachments, keyed by slot and placeholder name, plus the
//! skin-required bones and constraints the skin brings in.

use std::collections::HashMap;

use crate::data::{AttachmentId, BoneId, ConstraintId, SlotId};
use crate::math::Color;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Skin {
    pub name: String,
    pub bones: Vec<BoneId>,
    pub constraints: Vec<ConstraintId>,
    /// Nonessential editor color.
    pub color: Color,
    /// Indexed by slot; each map is placeholder name to attachment.
    slots: Vec<HashMap<String, AttachmentId>>,
}

impl Skin {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: Color::new(0.99607843, 0.61960787, 0.30980393, 1.0),
            ..Self::default()
        }
    }

    pub fn set_attachment(
        &mut self,
        slot: SlotId,
        placeholder: impl Into<String>,
        attachment: AttachmentId,
    ) {
        let i = slot.index();
        if self.slots.len() <= i {
            self.slots.resize_with(i + 1, HashMap::new);
        }
        self.slots[i].insert(placeholder.into(), attachment);
    }

    #[must_use]
    pub fn get_attachment(&self, slot: SlotId, placeholder: &str) -> Option<AttachmentId> {
        self.slots.get(slot.index())?.get(placeholder).copied()
    }

    pub fn remove_attachment(&mut self, slot: SlotId, placeholder: &str) {
        if let Some(map) = self.slots.get_mut(slot.index()) {
            map.remove(placeholder);
        }
    }

    #[must_use]
    pub fn attachment_count(&self) -> usize {
        self.slots.iter().map(HashMap::len).sum()
    }

    /// Every `(slot, placeholder, attachment)` entry.
    pub fn attachments(&self) -> impl Iterator<Item = (SlotId, &str, AttachmentId)> + '_ {
        self.slots.iter().enumerate().flat_map(|(slot, map)| {
            map.iter()
                .map(move |(name, id)| (SlotId(slot as u16), name.as_str(), *id))
        })
    }

    /// Entries on one slot.
    pub fn slot_attachments(
        &self,
        slot: SlotId,
    ) -> impl Iterator<Item = (&str, AttachmentId)> + '_ {
        self.slots
            .get(slot.index())
            .into_iter()
            .flat_map(|map| map.iter().map(|(name, id)| (name.as_str(), *id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_set_get_remove() {
        let mut skin = Skin::new("default");
        skin.set_attachment(SlotId(0), "body", AttachmentId(7));
        skin.set_attachment(SlotId(3), "head", AttachmentId(8));
        skin.set_attachment(SlotId(0), "body", AttachmentId(9));
        assert_eq!(
            skin.get_attachment(SlotId(0), "body"),
            Some(AttachmentId(9))
        );
        assert_eq!(
            skin.get_attachment(SlotId(3), "head"),
            Some(AttachmentId(8))
        );
        assert_eq!(skin.get_attachment(SlotId(1), "head"), None);
        assert_eq!(skin.get_attachment(SlotId(9), "head"), None);
        assert_eq!(skin.attachment_count(), 2);
        skin.remove_attachment(SlotId(0), "body");
        assert_eq!(skin.attachment_count(), 1);
    }
}
