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

//! Skins: attachments keyed by slot and placeholder, plus the skin-required
//! bones and constraints they bring in.
//!
//! Every `(slot, placeholder)` pair the data mentions (in skins, setup
//! attachments and attachment timelines) is interned once as a [`SkinKey`],
//! so a skin is a flat table indexed by key and lookups never hash strings.

use crate::data::{Attachment, AttachmentId, BoneId, ConstraintId, SkinKey, SlotId};
use crate::math::Color;

/// An attachment shown by a slot: one from the shared data, or one owned by
/// the skeleton's current skin (a runtime copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentRef {
    Data(AttachmentId),
    /// Index into the owning skin's [`Skin::owned`].
    Owned(u32),
}

/// Interned `(slot, placeholder)` pairs of one `SkeletonData`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SkinKeys {
    keys: Vec<(SlotId, Box<str>)>,
    by_slot: Vec<Vec<SkinKey>>,
}

impl SkinKeys {
    /// The key for a pair, adding it if new.
    pub fn intern(&mut self, slot: SlotId, placeholder: &str) -> SkinKey {
        if let Some(key) = self.find(slot, placeholder) {
            return key;
        }
        let key = SkinKey(self.keys.len() as u32);
        self.keys.push((slot, placeholder.into()));
        if self.by_slot.len() <= slot.index() {
            self.by_slot.resize_with(slot.index() + 1, Vec::new);
        }
        self.by_slot[slot.index()].push(key);
        key
    }

    /// Slots carry a handful of placeholders, so this is a short scan.
    #[must_use]
    pub fn find(&self, slot: SlotId, placeholder: &str) -> Option<SkinKey> {
        self.slot_keys(slot)
            .iter()
            .copied()
            .find(|k| *self.keys[k.index()].1 == *placeholder)
    }

    #[must_use]
    pub fn slot(&self, key: SkinKey) -> SlotId {
        self.keys[key.index()].0
    }

    #[must_use]
    pub fn placeholder(&self, key: SkinKey) -> &str {
        &self.keys[key.index()].1
    }

    #[must_use]
    pub fn slot_keys(&self, slot: SlotId) -> &[SkinKey] {
        self.by_slot.get(slot.index()).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// A skin for one `SkeletonData`: its keys and data attachment ids are only
/// meaningful against that data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Skin {
    pub name: String,
    pub bones: Vec<BoneId>,
    pub constraints: Vec<ConstraintId>,
    /// Nonessential editor color.
    pub color: Color,
    /// Indexed by [`SkinKey`]; keys past the end are empty.
    entries: Vec<Option<AttachmentRef>>,
    /// Placeholders the data never mentions, so no timeline or setup pose
    /// can reach them; only name lookups do.
    extra: Vec<(SlotId, Box<str>, AttachmentRef)>,
    owned: Vec<Attachment>,
}

impl Skin {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: Color::new(0.996_078_43, 0.619_607_87, 0.309_803_93, 1.0),
            ..Self::default()
        }
    }

    #[inline]
    #[must_use]
    pub fn get(&self, key: SkinKey) -> Option<AttachmentRef> {
        self.entries.get(key.index()).copied().flatten()
    }

    /// # Panics
    ///
    /// If `attachment` is an owned index this skin doesn't have.
    pub fn set(&mut self, key: SkinKey, attachment: AttachmentRef) {
        if let AttachmentRef::Owned(i) = attachment {
            assert!(
                (i as usize) < self.owned.len(),
                "owned attachment {i} out of range"
            );
        }
        if self.entries.len() <= key.index() {
            self.entries.resize(key.index() + 1, None);
        }
        self.entries[key.index()] = Some(attachment);
    }

    /// Clears the entry. An owned attachment it pointed at stays in the
    /// arena, since other entries may share it.
    pub fn remove(&mut self, key: SkinKey) {
        if let Some(e) = self.entries.get_mut(key.index()) {
            *e = None;
        }
    }

    /// The attachment for a placeholder name, including runtime-only names.
    #[must_use]
    pub fn get_named(
        &self,
        keys: &SkinKeys,
        slot: SlotId,
        placeholder: &str,
    ) -> Option<AttachmentRef> {
        match keys.find(slot, placeholder) {
            Some(key) => self.get(key),
            None => self
                .extra
                .iter()
                .find(|(s, n, _)| *s == slot && **n == *placeholder)
                .map(|e| e.2),
        }
    }

    /// Sets by name. A name the data never mentions is kept as a
    /// runtime-only placeholder.
    ///
    /// # Panics
    ///
    /// If `attachment` is an owned index this skin doesn't have.
    pub fn set_named(
        &mut self,
        keys: &SkinKeys,
        slot: SlotId,
        placeholder: &str,
        attachment: AttachmentRef,
    ) {
        if let Some(key) = keys.find(slot, placeholder) {
            self.set(key, attachment);
            return;
        }
        if let AttachmentRef::Owned(i) = attachment {
            assert!(
                (i as usize) < self.owned.len(),
                "owned attachment {i} out of range"
            );
        }
        self.set_extra(slot, placeholder, attachment);
    }

    pub fn remove_named(&mut self, keys: &SkinKeys, slot: SlotId, placeholder: &str) {
        match keys.find(slot, placeholder) {
            Some(key) => self.remove(key),
            None => self
                .extra
                .retain(|(s, n, _)| !(*s == slot && **n == *placeholder)),
        }
    }

    /// Runtime-only placeholders on `slot`.
    pub fn extra_on(&self, slot: SlotId) -> impl Iterator<Item = (&str, AttachmentRef)> + '_ {
        self.extra
            .iter()
            .filter(move |e| e.0 == slot)
            .map(|(_, n, a)| (&**n, *a))
    }

    /// Moves `attachment` into this skin's arena. Pair with [`Self::set`].
    pub fn add_owned(&mut self, attachment: Attachment) -> AttachmentRef {
        self.owned.push(attachment);
        AttachmentRef::Owned(self.owned.len() as u32 - 1)
    }

    #[must_use]
    pub fn owned(&self) -> &[Attachment] {
        &self.owned
    }

    #[must_use]
    pub fn owned_mut(&mut self) -> &mut [Attachment] {
        &mut self.owned
    }

    /// Every non-empty entry.
    pub fn entries(&self) -> impl Iterator<Item = (SkinKey, AttachmentRef)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.map(|a| (SkinKey(i as u32), a)))
    }

    #[must_use]
    pub fn attachment_count(&self) -> usize {
        self.entries.iter().flatten().count() + self.extra.len()
    }

    /// Adds `other`'s bones, constraints and entries, replacing entries with
    /// the same key. Its owned attachments are copied into this skin; data
    /// attachments are shared.
    pub fn add_skin(&mut self, other: &Skin) {
        self.add_requirements(other);
        let base = self.owned.len() as u32;
        self.owned.extend_from_slice(&other.owned);
        let remap = |a: AttachmentRef| match a {
            AttachmentRef::Owned(i) => AttachmentRef::Owned(base + i),
            data @ AttachmentRef::Data(_) => data,
        };
        for (key, attachment) in other.entries() {
            self.set(key, remap(attachment));
        }
        for (slot, name, attachment) in &other.extra {
            self.set_extra(*slot, name, remap(*attachment));
        }
    }

    /// Like [`Self::add_skin`], but every attachment becomes an owned copy
    /// (see [`Attachment::copy`]).
    pub fn copy_skin(&mut self, other: &Skin, attachments: &[Attachment]) {
        self.add_requirements(other);
        for (key, attachment) in other.entries() {
            let copy = self.copy_of(other, attachments, attachment);
            self.set(key, copy);
        }
        for (slot, name, attachment) in &other.extra {
            let copy = self.copy_of(other, attachments, *attachment);
            self.set_extra(*slot, name, copy);
        }
    }

    fn copy_of(
        &mut self,
        other: &Skin,
        attachments: &[Attachment],
        attachment: AttachmentRef,
    ) -> AttachmentRef {
        let copy = other.resolve(attachments, attachment).copy(attachment);
        self.add_owned(copy)
    }

    fn add_requirements(&mut self, other: &Skin) {
        for &b in &other.bones {
            if !self.bones.contains(&b) {
                self.bones.push(b);
            }
        }
        for &c in &other.constraints {
            if !self.constraints.contains(&c) {
                self.constraints.push(c);
            }
        }
    }

    fn set_extra(&mut self, slot: SlotId, placeholder: &str, attachment: AttachmentRef) {
        match self
            .extra
            .iter_mut()
            .find(|(s, n, _)| *s == slot && **n == *placeholder)
        {
            Some(e) => e.2 = attachment,
            None => self.extra.push((slot, placeholder.into(), attachment)),
        }
    }

    /// Drops owned attachments that no entry refers to and `keep` doesn't
    /// mark. Returns each old owned index's new index.
    pub(crate) fn compact(&mut self, keep: &mut [bool]) -> Vec<Option<u32>> {
        let mark = |keep: &mut [bool], a: AttachmentRef| {
            if let AttachmentRef::Owned(i) = a {
                keep[i as usize] = true;
            }
        };
        for a in self.entries.iter().flatten() {
            mark(keep, *a);
        }
        for e in &self.extra {
            mark(keep, e.2);
        }
        let mut remap = vec![None; self.owned.len()];
        let mut next = 0;
        let mut i = 0;
        self.owned.retain(|_| {
            let kept = keep[i];
            if kept {
                remap[i] = Some(next);
                next += 1;
            }
            i += 1;
            kept
        });
        let apply = |a: &mut AttachmentRef| {
            if let AttachmentRef::Owned(i) = a {
                *i = remap[*i as usize].expect("referenced attachments are kept");
            }
        };
        for a in self.entries.iter_mut().flatten() {
            apply(a);
        }
        for e in &mut self.extra {
            apply(&mut e.2);
        }
        remap
    }

    /// The attachment `r` refers to, reading owned ones from this skin.
    ///
    /// # Panics
    ///
    /// If `r` is owned and out of range for this skin.
    #[inline]
    #[must_use]
    pub fn resolve<'a>(
        &'a self,
        attachments: &'a [Attachment],
        r: AttachmentRef,
    ) -> &'a Attachment {
        match r {
            AttachmentRef::Data(id) => &attachments[id.index()],
            AttachmentRef::Owned(i) => &self.owned[i as usize],
        }
    }
}

/// Resolves `r` against the data's attachments and, for owned ones, `skin`.
///
/// # Panics
///
/// If `r` is owned and `skin` is `None` or doesn't hold it.
#[inline]
#[must_use]
pub fn resolve<'a>(
    attachments: &'a [Attachment],
    skin: Option<&'a Skin>,
    r: AttachmentRef,
) -> &'a Attachment {
    match r {
        AttachmentRef::Data(id) => &attachments[id.index()],
        AttachmentRef::Owned(i) => {
            &skin.expect("owned attachment without a skin").owned[i as usize]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PointAttachment;

    #[test]
    fn keys_intern_once_per_slot_and_name() {
        let mut keys = SkinKeys::default();
        let a = keys.intern(SlotId(2), "head");
        let b = keys.intern(SlotId(0), "head");
        assert_ne!(a, b);
        assert_eq!(keys.intern(SlotId(2), "head"), a);
        assert_eq!(keys.find(SlotId(0), "head"), Some(b));
        assert_eq!(keys.find(SlotId(1), "head"), None);
        assert_eq!(keys.slot(a), SlotId(2));
        assert_eq!(keys.placeholder(b), "head");
    }

    #[test]
    fn add_skin_remaps_owned_attachments() {
        let (k0, k1) = (SkinKey(0), SkinKey(3));
        let mut base = Skin::new("base");
        let p = base.add_owned(Attachment::Point(PointAttachment::new("p")));
        base.set(k0, p);
        let mut other = Skin::new("other");
        let q = other.add_owned(Attachment::Point(PointAttachment::new("q")));
        other.set(k1, q);
        other.set(k0, AttachmentRef::Data(AttachmentId(5)));
        base.add_skin(&other);
        assert_eq!(base.get(k0), Some(AttachmentRef::Data(AttachmentId(5))));
        let q = base.get(k1).unwrap();
        assert_eq!(base.resolve(&[], q).name(), "q");
        assert_eq!(base.attachment_count(), 2);
        base.remove(k1);
        assert_eq!(base.get(k1), None);
    }
}
