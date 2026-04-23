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

//! Binary `.skel` parser — port of `spine-cpp/SkeletonBinary.cpp`.
//!
//! The public entry point is [`SkeletonBinary`]. Instantiate with a mutable
//! [`AttachmentLoader`] and call [`SkeletonBinary::read`] on a byte buffer
//! to produce a [`SkeletonData`].
//!
//! The implementation mirrors the spine-cpp single-file approach so that a
//! reader comparing line-for-line sees roughly matching structure. Sections
//! are tagged with `// --- Section name` banners.

// Pedantic clippy lints that fire on faithful ports of spine-cpp's dense
// single-file parser. These are silenced at module scope so the port stays
// verbatim against SkeletonBinary.cpp — chasing each lint would require
// restructuring the port away from the reference.
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::many_single_char_names,
    clippy::match_same_arms,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_self,
    clippy::assigning_clones,
    clippy::doc_markdown,
    clippy::missing_panics_doc
)]

use crate::data::attachment::{Attachment, Sequence, VertexData};
use crate::data::{
    Animation, AnimationEvent, AttachmentId, BlendMode, BoneData, BoneId, CurveFrames, EventData,
    EventId, IkConstraintData, IkConstraintId, Inherit, PathConstraintData, PathConstraintId,
    PhysicsConstraintData, PhysicsConstraintId, PhysicsProperty, PositionMode, RotateMode,
    SkeletonData, Skin, SkinId, SlotData, SlotId, SpacingMode, Timeline, TransformConstraintData,
    TransformConstraintId,
};
use crate::load::AttachmentLoader;

use super::reader::{BinaryError, BinaryReader};

/// Spine editor version this runtime is built for. Used as a prefix check
/// against the skeleton's embedded version string.
pub const TARGET_VERSION: &str = "4.2";

// Timeline type discriminants, from SkeletonBinary.h.
const BONE_ROTATE: u8 = 0;
const BONE_TRANSLATE: u8 = 1;
const BONE_TRANSLATE_X: u8 = 2;
const BONE_TRANSLATE_Y: u8 = 3;
const BONE_SCALE: u8 = 4;
const BONE_SCALE_X: u8 = 5;
const BONE_SCALE_Y: u8 = 6;
const BONE_SHEAR: u8 = 7;
const BONE_SHEAR_X: u8 = 8;
const BONE_SHEAR_Y: u8 = 9;
const BONE_INHERIT: u8 = 10;

const SLOT_ATTACHMENT: u8 = 0;
const SLOT_RGBA: u8 = 1;
const SLOT_RGB: u8 = 2;
const SLOT_RGBA2: u8 = 3;
const SLOT_RGB2: u8 = 4;
const SLOT_ALPHA: u8 = 5;

const ATTACHMENT_DEFORM: u8 = 0;
const ATTACHMENT_SEQUENCE: u8 = 1;

const PATH_POSITION: u8 = 0;
const PATH_SPACING: u8 = 1;
const PATH_MIX: u8 = 2;

const PHYSICS_INERTIA: u8 = 0;
const PHYSICS_STRENGTH: u8 = 1;
const PHYSICS_DAMPING: u8 = 2;
// Note: discriminant 3 is skipped in spine-cpp (MASS = 4).
const PHYSICS_MASS: u8 = 4;
const PHYSICS_WIND: u8 = 5;
const PHYSICS_GRAVITY: u8 = 6;
const PHYSICS_MIX: u8 = 7;
const PHYSICS_RESET: u8 = 8;

const CURVE_LINEAR: i8 = 0;
const CURVE_STEPPED: i8 = 1;
const CURVE_BEZIER: i8 = 2;

/// Record of a mesh attachment whose vertex data is inherited from a parent
/// mesh in another skin. Linked meshes are resolved after all skins have
/// loaded (see [`SkeletonBinary::resolve_linked_meshes`]).
struct LinkedMesh {
    mesh: AttachmentId,
    skin_index: usize,
    slot_index: usize,
    parent_name: String,
    inherit_timeline: bool,
}

/// Stateful parser for the binary `.skel` format. Keeps scratch state for
/// linked-mesh resolution between top-level sections.
pub struct SkeletonBinary<'loader> {
    loader: &'loader mut dyn AttachmentLoader,
    scale: f32,
    linked_meshes: Vec<LinkedMesh>,
}

impl<'loader> SkeletonBinary<'loader> {
    /// Build a parser that resolves attachments through `loader`.
    pub fn with_loader(loader: &'loader mut dyn AttachmentLoader) -> Self {
        Self {
            loader,
            scale: 1.0,
            linked_meshes: Vec::new(),
        }
    }

    /// Override the load-time world-space scale (default `1.0`). Applied to
    /// position fields that spine-cpp scales during load.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Parse a `.skel` byte buffer and return the populated skeleton data.
    ///
    /// # Errors
    /// Returns [`BinaryError`] on malformed content, UTF-8 violations,
    /// version mismatches, or attachment-loader failures.
    pub fn read(mut self, bytes: &[u8]) -> Result<SkeletonData, BinaryError> {
        let mut r = BinaryReader::new(bytes);
        let mut sd = SkeletonData::default();
        self.linked_meshes.clear();

        // --- Header ---------------------------------------------------------
        let low = r.read_int()? as u32;
        let high = r.read_int()? as u32;
        sd.hash = format!("{high:x}{low:x}");

        sd.version = r.read_string()?.unwrap_or_default();
        if !sd.version.starts_with(TARGET_VERSION) {
            return Err(BinaryError::UnsupportedVersion {
                found: sd.version.clone(),
                expected: TARGET_VERSION.to_string(),
            });
        }

        sd.x = r.read_float()?;
        sd.y = r.read_float()?;
        sd.width = r.read_float()?;
        sd.height = r.read_float()?;
        sd.reference_scale = r.read_float()? * self.scale;

        let nonessential = r.read_bool()?;
        if nonessential {
            sd.fps = r.read_float()?;
            sd.images_path = r.read_string()?.unwrap_or_default();
            sd.audio_path = r.read_string()?.unwrap_or_default();
        }

        // --- String table ---------------------------------------------------
        let num_strings = r.read_uvarint()?;
        let mut strings = Vec::with_capacity(num_strings);
        for _ in 0..num_strings {
            strings.push(r.read_string()?.unwrap_or_default());
        }

        // --- Bones ----------------------------------------------------------
        let num_bones = r.read_uvarint()?;
        sd.bones.reserve(num_bones);
        for i in 0..num_bones {
            let name = r.read_string()?.unwrap_or_default();
            let parent = if i == 0 {
                None
            } else {
                let idx = r.read_uvarint()?;
                check_index(&r, "bone", idx, sd.bones.len())?;
                Some(BoneId(idx as u16))
            };
            let id = BoneId(i as u16);
            let mut b = BoneData::new(id, name, parent);
            b.rotation = r.read_float()?;
            b.x = r.read_float()? * self.scale;
            b.y = r.read_float()? * self.scale;
            b.scale_x = r.read_float()?;
            b.scale_y = r.read_float()?;
            b.shear_x = r.read_float()?;
            b.shear_y = r.read_float()?;
            b.length = r.read_float()? * self.scale;
            b.inherit = read_inherit(&mut r)?;
            b.skin_required = r.read_bool()?;
            if nonessential {
                b.color = r.read_color()?;
                b.icon = r.read_string()?.unwrap_or_default();
                b.visible = r.read_bool()?;
            }
            sd.bones.push(b);
        }

        // --- Slots ----------------------------------------------------------
        let num_slots = r.read_uvarint()?;
        sd.slots.reserve(num_slots);
        for i in 0..num_slots {
            let name = r.read_string()?.unwrap_or_default();
            let bone_idx = r.read_uvarint()?;
            check_index(&r, "bone", bone_idx, sd.bones.len())?;
            let mut slot = SlotData::new(SlotId(i as u16), name, BoneId(bone_idx as u16));
            slot.color = r.read_color()?;

            // Dark color: 4 bytes, all 0xFF meaning "no dark color".
            let a = r.read_byte()?;
            let dr = r.read_byte()?;
            let dg = r.read_byte()?;
            let db = r.read_byte()?;
            if !(dr == 0xFF && dg == 0xFF && db == 0xFF && a == 0xFF) {
                slot.dark_color = Some(crate::math::Color::new(
                    f32::from(dr) / 255.0,
                    f32::from(dg) / 255.0,
                    f32::from(db) / 255.0,
                    1.0,
                ));
            }
            slot.attachment_name = r.read_string_ref(&strings)?;
            slot.blend_mode = read_blend_mode(&mut r)?;
            if nonessential {
                slot.visible = r.read_bool()?;
            }
            sd.slots.push(slot);
        }

        // --- IK constraints -------------------------------------------------
        let num_ik = r.read_uvarint()?;
        sd.ik_constraints.reserve(num_ik);
        for i in 0..num_ik {
            let name = r.read_string()?.unwrap_or_default();
            let order = r.read_uvarint()? as u32;
            let bones_n = r.read_uvarint()?;
            let mut bones = Vec::with_capacity(bones_n);
            for _ in 0..bones_n {
                let idx = r.read_uvarint()?;
                check_index(&r, "bone", idx, sd.bones.len())?;
                bones.push(BoneId(idx as u16));
            }
            let target_idx = r.read_uvarint()?;
            check_index(&r, "bone", target_idx, sd.bones.len())?;
            let flags = r.read_byte()?;
            let mut ik =
                IkConstraintData::new(IkConstraintId(i as u16), name, BoneId(target_idx as u16));
            ik.order = order;
            ik.bones = bones;
            ik.skin_required = flags & 1 != 0;
            ik.bend_direction = if flags & 2 != 0 { 1 } else { -1 };
            ik.compress = flags & 4 != 0;
            ik.stretch = flags & 8 != 0;
            ik.uniform = flags & 16 != 0;
            if flags & 32 != 0 {
                ik.mix = if flags & 64 != 0 {
                    r.read_float()?
                } else {
                    1.0
                };
            } else {
                ik.mix = 0.0;
            }
            if flags & 128 != 0 {
                ik.softness = r.read_float()? * self.scale;
            }
            sd.ik_constraints.push(ik);
        }

        // --- Transform constraints -----------------------------------------
        let num_tc = r.read_uvarint()?;
        sd.transform_constraints.reserve(num_tc);
        for i in 0..num_tc {
            let name = r.read_string()?.unwrap_or_default();
            let order = r.read_uvarint()? as u32;
            let bones_n = r.read_uvarint()?;
            let mut bones = Vec::with_capacity(bones_n);
            for _ in 0..bones_n {
                let idx = r.read_uvarint()?;
                check_index(&r, "bone", idx, sd.bones.len())?;
                bones.push(BoneId(idx as u16));
            }
            let target_idx = r.read_uvarint()?;
            check_index(&r, "bone", target_idx, sd.bones.len())?;
            let mut tc = TransformConstraintData::new(
                TransformConstraintId(i as u16),
                name,
                BoneId(target_idx as u16),
            );
            tc.order = order;
            tc.bones = bones;
            let flags = r.read_byte()?;
            tc.skin_required = flags & 1 != 0;
            tc.local = flags & 2 != 0;
            tc.relative = flags & 4 != 0;
            if flags & 8 != 0 {
                tc.offset_rotation = r.read_float()?;
            }
            if flags & 16 != 0 {
                tc.offset_x = r.read_float()? * self.scale;
            }
            if flags & 32 != 0 {
                tc.offset_y = r.read_float()? * self.scale;
            }
            if flags & 64 != 0 {
                tc.offset_scale_x = r.read_float()?;
            }
            if flags & 128 != 0 {
                tc.offset_scale_y = r.read_float()?;
            }
            let flags = r.read_byte()?;
            if flags & 1 != 0 {
                tc.offset_shear_y = r.read_float()?;
            }
            if flags & 2 != 0 {
                tc.mix_rotate = r.read_float()?;
            }
            if flags & 4 != 0 {
                tc.mix_x = r.read_float()?;
            }
            if flags & 8 != 0 {
                tc.mix_y = r.read_float()?;
            }
            if flags & 16 != 0 {
                tc.mix_scale_x = r.read_float()?;
            }
            if flags & 32 != 0 {
                tc.mix_scale_y = r.read_float()?;
            }
            if flags & 64 != 0 {
                tc.mix_shear_y = r.read_float()?;
            }
            sd.transform_constraints.push(tc);
        }

        // --- Path constraints ----------------------------------------------
        let num_pc = r.read_uvarint()?;
        sd.path_constraints.reserve(num_pc);
        for i in 0..num_pc {
            let name = r.read_string()?.unwrap_or_default();
            let order = r.read_uvarint()? as u32;
            let skin_required = r.read_bool()?;
            let bones_n = r.read_uvarint()?;
            let mut bones = Vec::with_capacity(bones_n);
            for _ in 0..bones_n {
                let idx = r.read_uvarint()?;
                check_index(&r, "bone", idx, sd.bones.len())?;
                bones.push(BoneId(idx as u16));
            }
            let target_idx = r.read_uvarint()?;
            check_index(&r, "slot", target_idx, sd.slots.len())?;
            let mut pc = PathConstraintData::new(
                PathConstraintId(i as u16),
                name,
                SlotId(target_idx as u16),
            );
            pc.order = order;
            pc.skin_required = skin_required;
            pc.bones = bones;
            let flags = r.read_byte()?;
            pc.position_mode = match flags & 1 {
                0 => PositionMode::Fixed,
                _ => PositionMode::Percent,
            };
            pc.spacing_mode = match (flags >> 1) & 3 {
                0 => SpacingMode::Length,
                1 => SpacingMode::Fixed,
                2 => SpacingMode::Percent,
                _ => SpacingMode::Proportional,
            };
            pc.rotate_mode = match (flags >> 3) & 3 {
                0 => RotateMode::Tangent,
                1 => RotateMode::Chain,
                _ => RotateMode::ChainScale,
            };
            if flags & 128 != 0 {
                pc.offset_rotation = r.read_float()?;
            }
            pc.position = r.read_float()?;
            if pc.position_mode == PositionMode::Fixed {
                pc.position *= self.scale;
            }
            pc.spacing = r.read_float()?;
            if matches!(pc.spacing_mode, SpacingMode::Length | SpacingMode::Fixed) {
                pc.spacing *= self.scale;
            }
            pc.mix_rotate = r.read_float()?;
            pc.mix_x = r.read_float()?;
            pc.mix_y = r.read_float()?;
            sd.path_constraints.push(pc);
        }

        // --- Physics constraints -------------------------------------------
        let num_phys = r.read_uvarint()?;
        sd.physics_constraints.reserve(num_phys);
        for i in 0..num_phys {
            let name = r.read_string()?.unwrap_or_default();
            let order = r.read_uvarint()? as u32;
            let bone_idx = r.read_uvarint()?;
            check_index(&r, "bone", bone_idx, sd.bones.len())?;
            let mut ph = PhysicsConstraintData::new(
                PhysicsConstraintId(i as u16),
                name,
                BoneId(bone_idx as u16),
            );
            ph.order = order;
            let flags = r.read_byte()?;
            ph.skin_required = flags & 1 != 0;
            if flags & 2 != 0 {
                ph.x = r.read_float()?;
            }
            if flags & 4 != 0 {
                ph.y = r.read_float()?;
            }
            if flags & 8 != 0 {
                ph.rotate = r.read_float()?;
            }
            if flags & 16 != 0 {
                ph.scale_x = r.read_float()?;
            }
            if flags & 32 != 0 {
                ph.shear_x = r.read_float()?;
            }
            ph.limit = if flags & 64 != 0 {
                r.read_float()?
            } else {
                5000.0
            } * self.scale;
            ph.step = 1.0 / f32::from(r.read_byte()?);
            ph.inertia = r.read_float()?;
            ph.strength = r.read_float()?;
            ph.damping = r.read_float()?;
            ph.mass_inverse = if flags & 128 != 0 {
                r.read_float()?
            } else {
                1.0
            };
            ph.wind = r.read_float()?;
            ph.gravity = r.read_float()?;
            let flags = r.read_byte()?;
            ph.inertia_global = flags & 1 != 0;
            ph.strength_global = flags & 2 != 0;
            ph.damping_global = flags & 4 != 0;
            ph.mass_global = flags & 8 != 0;
            ph.wind_global = flags & 16 != 0;
            ph.gravity_global = flags & 32 != 0;
            ph.mix_global = flags & 64 != 0;
            ph.mix = if flags & 128 != 0 {
                r.read_float()?
            } else {
                1.0
            };
            sd.physics_constraints.push(ph);
        }

        // --- Default skin ---------------------------------------------------
        if let Some(skin) = self.read_skin(&mut r, true, &mut sd, &strings, nonessential)? {
            let id = SkinId(sd.skins.len() as u16);
            sd.default_skin = Some(id);
            sd.skins.push(skin);
        }

        // --- Named skins ----------------------------------------------------
        let num_skins = r.read_uvarint()?;
        for _ in 0..num_skins {
            let skin = self
                .read_skin(&mut r, false, &mut sd, &strings, nonessential)?
                .expect("named skins always return Some");
            sd.skins.push(skin);
        }

        // --- Linked mesh resolution ----------------------------------------
        self.resolve_linked_meshes(&mut sd)?;

        // --- Events ---------------------------------------------------------
        let num_events = r.read_uvarint()?;
        sd.events.reserve(num_events);
        for i in 0..num_events {
            let name = r.read_string()?.unwrap_or_default();
            let mut e = EventData::new(EventId(i as u16), name);
            e.int_value = r.read_varint(false)?;
            e.float_value = r.read_float()?;
            e.string_value = r.read_string()?.unwrap_or_default();
            e.audio_path = r.read_string()?.unwrap_or_default();
            if !e.audio_path.is_empty() {
                e.volume = r.read_float()?;
                e.balance = r.read_float()?;
            }
            sd.events.push(e);
        }

        // --- Animations -----------------------------------------------------
        let num_anims = r.read_uvarint()?;
        sd.animations.reserve(num_anims);
        for _ in 0..num_anims {
            let name = r.read_string()?.unwrap_or_default();
            let anim = self.read_animation(&mut r, &sd, &strings, name)?;
            sd.animations.push(anim);
        }

        Ok(sd)
    }

    // -----------------------------------------------------------------------
    // Skin + attachment
    // -----------------------------------------------------------------------

    fn read_skin(
        &mut self,
        r: &mut BinaryReader<'_>,
        default_skin: bool,
        sd: &mut SkeletonData,
        strings: &[String],
        nonessential: bool,
    ) -> Result<Option<Skin>, BinaryError> {
        let skin_index = sd.skins.len();
        let (mut skin, slot_count) = if default_skin {
            let sc = r.read_uvarint()?;
            if sc == 0 {
                return Ok(None);
            }
            (Skin::new("default"), sc)
        } else {
            let name = r.read_string()?.unwrap_or_default();
            let mut skin = Skin::new(name);
            if nonessential {
                // Skin color — not stored on the Skin type yet; read and
                // discard to keep the stream aligned.
                let _ = r.read_color()?;
            }
            let bones_n = r.read_uvarint()?;
            for _ in 0..bones_n {
                let idx = r.read_uvarint()?;
                check_index(r, "bone", idx, sd.bones.len())?;
                skin.bones.push(BoneId(idx as u16));
            }
            let ik_n = r.read_uvarint()?;
            for _ in 0..ik_n {
                let idx = r.read_uvarint()?;
                check_index(r, "ik_constraint", idx, sd.ik_constraints.len())?;
                skin.ik_constraints.push(IkConstraintId(idx as u16));
            }
            let tc_n = r.read_uvarint()?;
            for _ in 0..tc_n {
                let idx = r.read_uvarint()?;
                check_index(
                    r,
                    "transform_constraint",
                    idx,
                    sd.transform_constraints.len(),
                )?;
                skin.transform_constraints
                    .push(TransformConstraintId(idx as u16));
            }
            let pc_n = r.read_uvarint()?;
            for _ in 0..pc_n {
                let idx = r.read_uvarint()?;
                check_index(r, "path_constraint", idx, sd.path_constraints.len())?;
                skin.path_constraints.push(PathConstraintId(idx as u16));
            }
            let phys_n = r.read_uvarint()?;
            for _ in 0..phys_n {
                let idx = r.read_uvarint()?;
                check_index(r, "physics_constraint", idx, sd.physics_constraints.len())?;
                skin.physics_constraints
                    .push(PhysicsConstraintId(idx as u16));
            }
            let sc = r.read_uvarint()?;
            (skin, sc)
        };

        for _ in 0..slot_count {
            let slot_idx = r.read_uvarint()?;
            check_index(r, "slot", slot_idx, sd.slots.len())?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let name = r.read_string_ref(strings)?.unwrap_or_default();
                let attachment = self.read_attachment(
                    r,
                    skin_index,
                    slot_idx,
                    &skin.name,
                    &name,
                    sd,
                    strings,
                    nonessential,
                )?;
                let id = AttachmentId(sd.attachments.len() as u32);
                sd.attachments.push(attachment);
                skin.set_attachment(SlotId(slot_idx as u16), name, id);
            }
        }
        Ok(Some(skin))
    }

    fn read_sequence(&self, r: &mut BinaryReader<'_>) -> Result<Sequence, BinaryError> {
        let count = r.read_uvarint()? as i32;
        let mut seq = Sequence::new(count);
        seq.start = r.read_uvarint()? as i32;
        seq.digits = r.read_uvarint()? as i32;
        seq.setup_index = r.read_uvarint()? as i32;
        Ok(seq)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_attachment(
        &mut self,
        r: &mut BinaryReader<'_>,
        _skin_index: usize,
        slot_idx: usize,
        skin_name: &str,
        attachment_name: &str,
        sd: &SkeletonData,
        strings: &[String],
        nonessential: bool,
    ) -> Result<Attachment, BinaryError> {
        let flags = r.read_byte()?;
        let name = if flags & 8 != 0 {
            r.read_string_ref(strings)?.unwrap_or_default()
        } else {
            attachment_name.to_string()
        };
        let kind = flags & 0x7;
        let slot_name = sd.slots[slot_idx].name.as_str();

        match kind {
            // Region
            0 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?.unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                };
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let mut sequence = if flags & 64 != 0 {
                    Some(self.read_sequence(r)?)
                } else {
                    None
                };
                let rotation = if flags & 128 != 0 {
                    r.read_float()?
                } else {
                    0.0
                };
                let x = r.read_float()? * self.scale;
                let y = r.read_float()? * self.scale;
                let scale_x = r.read_float()?;
                let scale_y = r.read_float()?;
                let width = r.read_float()? * self.scale;
                let height = r.read_float()? * self.scale;

                let mut attachment = self.loader.new_region_attachment(
                    skin_name,
                    slot_name,
                    &name,
                    &path,
                    sequence.as_mut(),
                )?;
                if let Attachment::Region(reg) = &mut attachment {
                    reg.path = path;
                    reg.rotation = rotation;
                    reg.x = x;
                    reg.y = y;
                    reg.scale_x = scale_x;
                    reg.scale_y = scale_y;
                    reg.width = width;
                    reg.height = height;
                    reg.color = color;
                    reg.sequence = sequence;
                }
                Ok(attachment)
            }

            // BoundingBox
            1 => {
                let mut attachment = self
                    .loader
                    .new_bounding_box_attachment(skin_name, slot_name, &name)?;
                let (vd, _len) = self.read_vertices(r, flags & 16 != 0)?;
                if let Attachment::BoundingBox(bb) = &mut attachment {
                    bb.vertex_data = vd;
                    if nonessential {
                        bb.color = r.read_color()?;
                    }
                }
                Ok(attachment)
            }

            // Mesh
            2 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?.unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                };
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let mut sequence = if flags & 64 != 0 {
                    Some(self.read_sequence(r)?)
                } else {
                    None
                };
                let hull_length = r.read_uvarint()? as u32;
                let (vd, verts_len) = self.read_vertices(r, flags & 128 != 0)?;
                let uvs = read_float_array(r, verts_len as usize, 1.0)?;
                // Triangle count from spine-cpp: `(verticesLength -
                // hullLength - 2) * 3`. `verticesLength` is `vertexCount *
                // 2`; `hullLength` is the raw value from the wire (vertex-
                // count units, not doubled). For a 10/10 (all-hull) mesh
                // this gives (20 - 10 - 2) * 3 = 24 indices = 8 triangles,
                // matching hull triangulation N - 2 = 8.
                let tri_count = ((verts_len as i32 - hull_length as i32 - 2).max(0)) as usize * 3;
                let triangles = read_short_array(r, tri_count)?;
                let (edges, width, height) = if nonessential {
                    let n = r.read_uvarint()?;
                    let e = read_short_array(r, n)?;
                    let w = r.read_float()?;
                    let h = r.read_float()?;
                    (e, w, h)
                } else {
                    (Vec::new(), 0.0, 0.0)
                };

                let mut attachment = self.loader.new_mesh_attachment(
                    skin_name,
                    slot_name,
                    &name,
                    &path,
                    sequence.as_mut(),
                )?;
                if let Attachment::Mesh(mesh) = &mut attachment {
                    mesh.path = path;
                    mesh.color = color;
                    mesh.vertex_data = vd;
                    mesh.region_uvs = uvs;
                    mesh.triangles = triangles;
                    mesh.hull_length = hull_length;
                    mesh.sequence = sequence;
                    mesh.edges = edges;
                    mesh.width = width;
                    mesh.height = height;
                }
                Ok(attachment)
            }

            // LinkedMesh
            3 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?.unwrap_or_else(|| name.clone())
                } else {
                    name.clone()
                };
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let mut sequence = if flags & 64 != 0 {
                    Some(self.read_sequence(r)?)
                } else {
                    None
                };
                let inherit_timeline = flags & 128 != 0;
                let parent_skin_index = r.read_uvarint()?;
                let parent = r.read_string_ref(strings)?.unwrap_or_default();
                let (width, height) = if nonessential {
                    (r.read_float()? * self.scale, r.read_float()? * self.scale)
                } else {
                    (0.0, 0.0)
                };

                let mut attachment = self.loader.new_mesh_attachment(
                    skin_name,
                    slot_name,
                    &name,
                    &path,
                    sequence.as_mut(),
                )?;
                if let Attachment::Mesh(mesh) = &mut attachment {
                    mesh.path = path;
                    mesh.color = color;
                    mesh.sequence = sequence;
                    if nonessential {
                        mesh.width = width;
                        mesh.height = height;
                    }
                }
                // Track for a second pass — we'll resolve the parent mesh
                // after all skins load. The attachment id isn't known yet
                // (the caller pushes it into sd.attachments). We stash the
                // anticipated id based on the current length + 0 offset from
                // where read_attachment's caller will push. That's fragile;
                // instead, store (skin_index, slot_index, name) and look up
                // by name later.
                //
                // We use skin_index from the outer read_skin (same scope
                // we're in). For name, we use `name` (the attachment name).
                self.linked_meshes.push(LinkedMesh {
                    mesh: AttachmentId(u32::MAX), // filled in below by caller
                    skin_index: parent_skin_index,
                    slot_index: slot_idx,
                    parent_name: parent,
                    inherit_timeline,
                });
                // Record where to patch the `mesh` id later: the caller will
                // push this attachment at `sd.attachments.len()`.
                let anticipated = sd.attachments.len() as u32;
                self.linked_meshes.last_mut().unwrap().mesh = AttachmentId(anticipated);
                Ok(attachment)
            }

            // Path
            4 => {
                let closed = flags & 16 != 0;
                let constant_speed = flags & 32 != 0;
                let (vd, verts_len) = self.read_vertices(r, flags & 64 != 0)?;
                let lengths_count = (verts_len / 6) as usize;
                let mut lengths = Vec::with_capacity(lengths_count);
                for _ in 0..lengths_count {
                    lengths.push(r.read_float()? * self.scale);
                }
                let color = if nonessential {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };

                let mut attachment = self
                    .loader
                    .new_path_attachment(skin_name, slot_name, &name)?;
                if let Attachment::Path(pa) = &mut attachment {
                    pa.closed = closed;
                    pa.constant_speed = constant_speed;
                    pa.vertex_data = vd;
                    pa.lengths = lengths;
                    if nonessential {
                        pa.color = color;
                    }
                }
                Ok(attachment)
            }

            // Point
            5 => {
                let rotation = r.read_float()?;
                let x = r.read_float()? * self.scale;
                let y = r.read_float()? * self.scale;
                let mut attachment = self
                    .loader
                    .new_point_attachment(skin_name, slot_name, &name)?;
                if let Attachment::Point(pt) = &mut attachment {
                    pt.rotation = rotation;
                    pt.x = x;
                    pt.y = y;
                    if nonessential {
                        pt.color = r.read_color()?;
                    }
                }
                Ok(attachment)
            }

            // Clipping
            6 => {
                let end_slot_idx = r.read_uvarint()?;
                check_index(r, "slot", end_slot_idx, sd.slots.len())?;
                let (vd, _len) = self.read_vertices(r, flags & 16 != 0)?;
                let mut attachment = self.loader.new_clipping_attachment(
                    skin_name,
                    slot_name,
                    &name,
                    SlotId(end_slot_idx as u16),
                )?;
                if let Attachment::Clipping(cl) = &mut attachment {
                    cl.vertex_data = vd;
                    if nonessential {
                        cl.color = r.read_color()?;
                    }
                }
                Ok(attachment)
            }

            other => Err(BinaryError::UnknownDiscriminant {
                at: r.position(),
                entity: "attachment type",
                value: u32::from(other),
            }),
        }
    }

    fn read_vertices(
        &self,
        r: &mut BinaryReader<'_>,
        weighted: bool,
    ) -> Result<(VertexData, u32), BinaryError> {
        let vertex_count = r.read_uvarint()?;
        let vertices_len = (vertex_count * 2) as u32;
        let mut vd = VertexData {
            world_vertices_length: vertices_len,
            ..VertexData::default()
        };
        if !weighted {
            vd.vertices = read_float_array(r, vertices_len as usize, self.scale)?;
            return Ok((vd, vertices_len));
        }
        for _ in 0..vertex_count {
            let bone_count = r.read_uvarint()?;
            vd.bones.push(bone_count as i32);
            for _ in 0..bone_count {
                let bone_index = r.read_uvarint()? as i32;
                vd.bones.push(bone_index);
                vd.vertices.push(r.read_float()? * self.scale);
                vd.vertices.push(r.read_float()? * self.scale);
                vd.vertices.push(r.read_float()?); // weight (no scale)
            }
        }
        Ok((vd, vertices_len))
    }

    fn resolve_linked_meshes(&mut self, sd: &mut SkeletonData) -> Result<(), BinaryError> {
        for lm in std::mem::take(&mut self.linked_meshes) {
            let skin = sd
                .skins
                .get(lm.skin_index)
                .ok_or(BinaryError::IndexOutOfRange {
                    at: 0,
                    entity: "skin",
                    index: lm.skin_index,
                    len: sd.skins.len(),
                })?;
            let parent_id = skin
                .get_attachment(SlotId(lm.slot_index as u16), &lm.parent_name)
                .ok_or(BinaryError::LinkedMeshParentMissing {
                    at: 0,
                    skin: skin.name.clone(),
                    slot: lm.slot_index,
                    parent: lm.parent_name.clone(),
                })?;
            // Copy parent geometry into the linked mesh, preserving the
            // linked mesh's own color/path/sequence already written earlier.
            let parent = sd.attachments[parent_id.index()].clone();
            let Attachment::Mesh(parent_mesh) = parent else {
                continue;
            };
            let slot = lm.mesh.index();
            if let Attachment::Mesh(child) = &mut sd.attachments[slot] {
                child.vertex_data = parent_mesh.vertex_data.clone();
                child.region_uvs = parent_mesh.region_uvs.clone();
                child.triangles = parent_mesh.triangles.clone();
                child.hull_length = parent_mesh.hull_length;
                child.edges = parent_mesh.edges.clone();
                child.parent_mesh = Some(parent_id);
                child.vertex_data.timeline_attachment = Some(if lm.inherit_timeline {
                    parent_id
                } else {
                    lm.mesh
                });
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Animations
    // -----------------------------------------------------------------------

    fn read_animation(
        &mut self,
        r: &mut BinaryReader<'_>,
        sd: &SkeletonData,
        strings: &[String],
        name: String,
    ) -> Result<Animation, BinaryError> {
        let mut anim = Animation::new(name, 0.0);
        let _num_timelines = r.read_uvarint()?; // hint only, unused

        // Slot timelines
        let slot_groups = r.read_uvarint()?;
        for _ in 0..slot_groups {
            let slot_idx = r.read_uvarint()?;
            check_index(r, "slot", slot_idx, sd.slots.len())?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let ttype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                match ttype {
                    SLOT_ATTACHMENT => {
                        let mut frames = Vec::with_capacity(frame_count);
                        let mut names = Vec::with_capacity(frame_count);
                        for _ in 0..frame_count {
                            frames.push(r.read_float()?);
                            names.push(r.read_string_ref(strings)?);
                        }
                        anim.timelines.push(Timeline::Attachment {
                            slot: SlotId(slot_idx as u16),
                            frames,
                            names,
                        });
                    }
                    SLOT_RGBA => {
                        let bezier_count = r.read_uvarint()?;
                        let curves = read_color_timeline(r, frame_count, bezier_count, 4)?;
                        anim.timelines.push(Timeline::Rgba {
                            slot: SlotId(slot_idx as u16),
                            curves,
                        });
                    }
                    SLOT_RGB => {
                        let bezier_count = r.read_uvarint()?;
                        let curves = read_color_timeline(r, frame_count, bezier_count, 3)?;
                        anim.timelines.push(Timeline::Rgb {
                            slot: SlotId(slot_idx as u16),
                            curves,
                        });
                    }
                    SLOT_RGBA2 => {
                        let bezier_count = r.read_uvarint()?;
                        let curves = read_color_timeline(r, frame_count, bezier_count, 7)?;
                        anim.timelines.push(Timeline::Rgba2 {
                            slot: SlotId(slot_idx as u16),
                            curves,
                        });
                    }
                    SLOT_RGB2 => {
                        let bezier_count = r.read_uvarint()?;
                        let curves = read_color_timeline(r, frame_count, bezier_count, 6)?;
                        anim.timelines.push(Timeline::Rgb2 {
                            slot: SlotId(slot_idx as u16),
                            curves,
                        });
                    }
                    SLOT_ALPHA => {
                        let bezier_count = r.read_uvarint()?;
                        let curves = read_color_timeline(r, frame_count, bezier_count, 1)?;
                        anim.timelines.push(Timeline::Alpha {
                            slot: SlotId(slot_idx as u16),
                            curves,
                        });
                    }
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "slot timeline",
                            value: u32::from(other),
                        });
                    }
                }
            }
        }

        // Bone timelines
        let bone_groups = r.read_uvarint()?;
        for _ in 0..bone_groups {
            let bone_idx = r.read_uvarint()?;
            check_index(r, "bone", bone_idx, sd.bones.len())?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let ttype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                if ttype == BONE_INHERIT {
                    let mut frames = Vec::with_capacity(frame_count);
                    let mut inherits = Vec::with_capacity(frame_count);
                    for _ in 0..frame_count {
                        frames.push(r.read_float()?);
                        // In the InheritTimeline binary layout, each frame's
                        // inherit mode is a single byte — not a varint. This
                        // differs from the BoneData header where the same
                        // field is a varint. Matches spine-cpp's
                        // SkeletonBinary.cpp line 1092.
                        inherits.push(read_inherit_byte(r)?);
                    }
                    anim.timelines.push(Timeline::Inherit {
                        bone: BoneId(bone_idx as u16),
                        frames,
                        inherits,
                    });
                    continue;
                }
                let bezier_count = r.read_uvarint()?;
                let (entries, scale) = match ttype {
                    BONE_ROTATE => (2, 1.0),
                    BONE_TRANSLATE => (3, self.scale),
                    BONE_TRANSLATE_X | BONE_TRANSLATE_Y => (2, self.scale),
                    BONE_SCALE => (3, 1.0),
                    BONE_SCALE_X | BONE_SCALE_Y => (2, 1.0),
                    BONE_SHEAR => (3, 1.0),
                    BONE_SHEAR_X | BONE_SHEAR_Y => (2, 1.0),
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "bone timeline",
                            value: u32::from(other),
                        });
                    }
                };
                let curves = read_curve_timeline(r, frame_count, bezier_count, entries, scale)?;
                anim.timelines.push(match ttype {
                    BONE_ROTATE => Timeline::Rotate {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_TRANSLATE => Timeline::Translate {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_TRANSLATE_X => Timeline::TranslateX {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_TRANSLATE_Y => Timeline::TranslateY {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SCALE => Timeline::Scale {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SCALE_X => Timeline::ScaleX {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SCALE_Y => Timeline::ScaleY {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SHEAR => Timeline::Shear {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SHEAR_X => Timeline::ShearX {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    BONE_SHEAR_Y => Timeline::ShearY {
                        bone: BoneId(bone_idx as u16),
                        curves,
                    },
                    _ => unreachable!(),
                });
            }
        }

        // IK timelines
        let ik_n = r.read_uvarint()?;
        for _ in 0..ik_n {
            let idx = r.read_uvarint()?;
            check_index(r, "ik_constraint", idx, sd.ik_constraints.len())?;
            let frame_count = r.read_uvarint()?;
            let bezier_count = r.read_uvarint()?;
            // IK timelines have their own frame shape (time + mix + softness
            // + 4 flag bits). We capture the raw frames and curves verbatim
            // for Phase 3 to interpret.
            let curves = read_ik_timeline(r, frame_count, bezier_count, self.scale)?;
            anim.timelines.push(Timeline::IkConstraint {
                constraint: IkConstraintId(idx as u16),
                curves,
            });
        }

        // Transform constraint timelines
        let tc_n = r.read_uvarint()?;
        for _ in 0..tc_n {
            let idx = r.read_uvarint()?;
            check_index(
                r,
                "transform_constraint",
                idx,
                sd.transform_constraints.len(),
            )?;
            let frame_count = r.read_uvarint()?;
            let bezier_count = r.read_uvarint()?;
            let curves = read_curve_timeline(r, frame_count, bezier_count, 7, 1.0)?;
            anim.timelines.push(Timeline::TransformConstraint {
                constraint: TransformConstraintId(idx as u16),
                curves,
            });
        }

        // Path constraint timelines
        let pc_n = r.read_uvarint()?;
        for _ in 0..pc_n {
            let idx = r.read_uvarint()?;
            check_index(r, "path_constraint", idx, sd.path_constraints.len())?;
            let data = &sd.path_constraints[idx];
            let sub_n = r.read_uvarint()?;
            for _ in 0..sub_n {
                let ptype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                let bezier_count = r.read_uvarint()?;
                match ptype {
                    PATH_POSITION => {
                        let scale = if data.position_mode == PositionMode::Fixed {
                            self.scale
                        } else {
                            1.0
                        };
                        let curves = read_curve_timeline(r, frame_count, bezier_count, 2, scale)?;
                        anim.timelines.push(Timeline::PathConstraintPosition {
                            constraint: PathConstraintId(idx as u16),
                            curves,
                        });
                    }
                    PATH_SPACING => {
                        let scale = if matches!(
                            data.spacing_mode,
                            SpacingMode::Length | SpacingMode::Fixed
                        ) {
                            self.scale
                        } else {
                            1.0
                        };
                        let curves = read_curve_timeline(r, frame_count, bezier_count, 2, scale)?;
                        anim.timelines.push(Timeline::PathConstraintSpacing {
                            constraint: PathConstraintId(idx as u16),
                            curves,
                        });
                    }
                    PATH_MIX => {
                        let curves = read_curve_timeline(r, frame_count, bezier_count, 4, 1.0)?;
                        anim.timelines.push(Timeline::PathConstraintMix {
                            constraint: PathConstraintId(idx as u16),
                            curves,
                        });
                    }
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "path constraint timeline",
                            value: u32::from(other),
                        });
                    }
                }
            }
        }

        // Physics timelines
        let phys_n = r.read_uvarint()?;
        for _ in 0..phys_n {
            // spine-cpp reads `index - 1`, so index of 0 means "all
            // constraints" (used by PhysicsReset).
            let raw = r.read_uvarint()?;
            let constraint = if raw == 0 {
                None
            } else {
                Some(PhysicsConstraintId((raw - 1) as u16))
            };
            let sub_n = r.read_uvarint()?;
            for _ in 0..sub_n {
                let ptype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                if ptype == PHYSICS_RESET {
                    let mut frames = Vec::with_capacity(frame_count);
                    for _ in 0..frame_count {
                        frames.push(r.read_float()?);
                    }
                    anim.timelines
                        .push(Timeline::PhysicsReset { constraint, frames });
                    continue;
                }
                let bezier_count = r.read_uvarint()?;
                let property = match ptype {
                    PHYSICS_INERTIA => PhysicsProperty::Inertia,
                    PHYSICS_STRENGTH => PhysicsProperty::Strength,
                    PHYSICS_DAMPING => PhysicsProperty::Damping,
                    PHYSICS_MASS => PhysicsProperty::Mass,
                    PHYSICS_WIND => PhysicsProperty::Wind,
                    PHYSICS_GRAVITY => PhysicsProperty::Gravity,
                    PHYSICS_MIX => PhysicsProperty::Mix,
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "physics timeline",
                            value: u32::from(other),
                        });
                    }
                };
                let curves = read_curve_timeline(r, frame_count, bezier_count, 2, 1.0)?;
                anim.timelines.push(Timeline::Physics {
                    constraint,
                    property,
                    curves,
                });
            }
        }

        // Attachment timelines (Deform + Sequence)
        let skin_groups = r.read_uvarint()?;
        for _ in 0..skin_groups {
            let skin_idx = r.read_uvarint()?;
            check_index(r, "skin", skin_idx, sd.skins.len())?;
            let slot_groups = r.read_uvarint()?;
            for _ in 0..slot_groups {
                let slot_idx = r.read_uvarint()?;
                check_index(r, "slot", slot_idx, sd.slots.len())?;
                let att_n = r.read_uvarint()?;
                for _ in 0..att_n {
                    let att_name = r.read_string_ref(strings)?.unwrap_or_default();
                    let attachment_id = sd.skins[skin_idx]
                        .get_attachment(SlotId(slot_idx as u16), &att_name)
                        .ok_or(BinaryError::LinkedMeshParentMissing {
                            at: r.position(),
                            skin: sd.skins[skin_idx].name.clone(),
                            slot: slot_idx,
                            parent: att_name.clone(),
                        })?;
                    let ttype = r.read_byte()?;
                    let frame_count = r.read_uvarint()?;
                    match ttype {
                        ATTACHMENT_DEFORM => {
                            let vertices_len = deform_frame_len(sd, attachment_id);
                            let bezier_count = r.read_uvarint()?;
                            let (frames, curves, deform_vertices) = read_deform_timeline(
                                r,
                                frame_count,
                                bezier_count,
                                vertices_len,
                                self.scale,
                                // For non-weighted attachments we need the
                                // setup pose vertices to layer the deltas on
                                // top of, to match spine-cpp's apply semantics.
                                // Phase 3 evaluation will perform that layering
                                // — we just store the deltas verbatim.
                            )?;
                            anim.timelines.push(Timeline::Deform {
                                slot: SlotId(slot_idx as u16),
                                attachment: attachment_id,
                                curves: CurveFrames { frames, curves },
                                vertices: deform_vertices,
                            });
                        }
                        ATTACHMENT_SEQUENCE => {
                            let mut frames = Vec::with_capacity(frame_count * 3);
                            for _ in 0..frame_count {
                                let time = r.read_float()?;
                                let mode_and_index = r.read_int()? as f32;
                                let delay = r.read_float()?;
                                frames.push(time);
                                frames.push(mode_and_index);
                                frames.push(delay);
                            }
                            anim.timelines.push(Timeline::Sequence {
                                slot: SlotId(slot_idx as u16),
                                attachment: attachment_id,
                                frames,
                            });
                        }
                        other => {
                            return Err(BinaryError::UnknownDiscriminant {
                                at: r.position(),
                                entity: "attachment timeline",
                                value: u32::from(other),
                            });
                        }
                    }
                }
            }
        }

        // Draw order timeline
        let draw_order_n = r.read_uvarint()?;
        if draw_order_n > 0 {
            let slot_count = sd.slots.len();
            let mut frames = Vec::with_capacity(draw_order_n);
            let mut draw_orders = Vec::with_capacity(draw_order_n);
            for _ in 0..draw_order_n {
                let time = r.read_float()?;
                let offset_count = r.read_uvarint()?;
                frames.push(time);
                if offset_count == 0 {
                    draw_orders.push(None);
                    continue;
                }
                let mut draw_order: Vec<i32> = vec![-1; slot_count];
                let mut unchanged: Vec<i32> = vec![0; slot_count - offset_count];
                let mut original_index: i32 = 0;
                let mut unchanged_index: usize = 0;
                for _ in 0..offset_count {
                    let slot_idx = r.read_uvarint()? as i32;
                    while original_index != slot_idx {
                        unchanged[unchanged_index] = original_index;
                        unchanged_index += 1;
                        original_index += 1;
                    }
                    // spine-cpp reads `shift` as an unsigned varint but then
                    // adds it to `index` via `size_t`, relying on unsigned
                    // wraparound to convert values like 0xFFFFFFFE back into
                    // signed -2 (slot moves N positions earlier in draw
                    // order). Rust's usize/u32 conversion doesn't preserve
                    // that trick on 64-bit targets, so we read the varint
                    // as a signed i32 and perform the addition in signed
                    // arithmetic.
                    let shift = r.read_varint(true)?;
                    let target = original_index + shift;
                    draw_order[target as usize] = original_index;
                    original_index += 1;
                }
                while (original_index as usize) < slot_count {
                    unchanged[unchanged_index] = original_index;
                    unchanged_index += 1;
                    original_index += 1;
                }
                for ii in (0..slot_count).rev() {
                    if draw_order[ii] == -1 {
                        unchanged_index -= 1;
                        draw_order[ii] = unchanged[unchanged_index];
                    }
                }
                draw_orders.push(Some(
                    draw_order.into_iter().map(|x| SlotId(x as u16)).collect(),
                ));
            }
            anim.timelines.push(Timeline::DrawOrder {
                frames,
                draw_orders,
            });
        }

        // Event timeline
        let event_n = r.read_uvarint()?;
        if event_n > 0 {
            let mut frames = Vec::with_capacity(event_n);
            let mut events = Vec::with_capacity(event_n);
            for _ in 0..event_n {
                let time = r.read_float()?;
                let ei = r.read_uvarint()?;
                check_index(r, "event", ei, sd.events.len())?;
                let data = &sd.events[ei];
                let int_value = r.read_varint(false)?;
                let float_value = r.read_float()?;
                let string_value = r.read_string()?.or_else(|| Some(data.string_value.clone()));
                let (volume, balance) = if data.audio_path.is_empty() {
                    (data.volume, data.balance)
                } else {
                    (r.read_float()?, r.read_float()?)
                };
                frames.push(time);
                events.push(AnimationEvent {
                    time,
                    event: EventId(ei as u16),
                    int_value,
                    float_value,
                    string_value,
                    volume,
                    balance,
                });
            }
            anim.timelines.push(Timeline::Event { frames, events });
        }

        // Duration = max end time across timelines. For Phase 1b (no
        // evaluation) we approximate from whatever frames we stored. This
        // is only used for animation-state queries; Phase 3 will refine.
        let duration = timeline_duration(&anim);
        anim.duration = duration;
        Ok(anim)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn check_index(
    r: &BinaryReader<'_>,
    entity: &'static str,
    index: usize,
    len: usize,
) -> Result<(), BinaryError> {
    if index >= len {
        Err(BinaryError::IndexOutOfRange {
            at: r.position(),
            entity,
            index,
            len,
        })
    } else {
        Ok(())
    }
}

fn read_inherit(r: &mut BinaryReader<'_>) -> Result<Inherit, BinaryError> {
    let v = r.read_uvarint()?;
    inherit_from_value(r, v as u32)
}

/// Single-byte inherit reader used by the `InheritTimeline`, where spine-cpp
/// uses `readByte` rather than the varint encoding seen in bone headers.
fn read_inherit_byte(r: &mut BinaryReader<'_>) -> Result<Inherit, BinaryError> {
    let v = r.read_byte()?;
    inherit_from_value(r, u32::from(v))
}

fn inherit_from_value(r: &BinaryReader<'_>, v: u32) -> Result<Inherit, BinaryError> {
    match v {
        0 => Ok(Inherit::Normal),
        1 => Ok(Inherit::OnlyTranslation),
        2 => Ok(Inherit::NoRotationOrReflection),
        3 => Ok(Inherit::NoScale),
        4 => Ok(Inherit::NoScaleOrReflection),
        other => Err(BinaryError::UnknownDiscriminant {
            at: r.position(),
            entity: "inherit mode",
            value: other,
        }),
    }
}

fn read_blend_mode(r: &mut BinaryReader<'_>) -> Result<BlendMode, BinaryError> {
    let v = r.read_uvarint()?;
    match v {
        0 => Ok(BlendMode::Normal),
        1 => Ok(BlendMode::Additive),
        2 => Ok(BlendMode::Multiply),
        3 => Ok(BlendMode::Screen),
        other => Err(BinaryError::UnknownDiscriminant {
            at: r.position(),
            entity: "blend mode",
            value: other as u32,
        }),
    }
}

fn read_float_array(
    r: &mut BinaryReader<'_>,
    n: usize,
    scale: f32,
) -> Result<Vec<f32>, BinaryError> {
    let mut out = Vec::with_capacity(n);
    if (scale - 1.0).abs() < f32::EPSILON {
        for _ in 0..n {
            out.push(r.read_float()?);
        }
    } else {
        for _ in 0..n {
            out.push(r.read_float()? * scale);
        }
    }
    Ok(out)
}

fn read_short_array(r: &mut BinaryReader<'_>, n: usize) -> Result<Vec<u16>, BinaryError> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let v = r.read_uvarint()? as u16;
        out.push(v);
    }
    Ok(out)
}

/// Read a CurveTimelineN where each frame has `entries` floats (time + values).
/// Scale is applied to every value (not time).
fn read_curve_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    bezier_count: usize,
    entries: usize,
    scale: f32,
) -> Result<CurveFrames, BinaryError> {
    // Spine-cpp stores frames and curve metadata as separate arrays built up
    // incrementally. We replicate that: `frames` holds `(time, v1, [v2..])`
    // per frame, and `curves` is a free-form f32 stream of per-segment
    // curve data (type code + optional bezier samples). For Phase 1b we
    // preserve the raw stream the file contained, so Phase 3's evaluator
    // can read back bit-exactly what spine-cpp wrote.
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * entries);
    let mut curves: Vec<f32> = Vec::new();

    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }

    let frame_last = frame_count - 1;
    let mut time = r.read_float()?;
    let mut values: Vec<f32> = Vec::with_capacity(entries - 1);
    for _ in 0..(entries - 1) {
        values.push(r.read_float()? * scale);
    }
    for frame in 0..frame_count {
        frames.push(time);
        for v in &values {
            frames.push(*v);
        }
        if frame == frame_last {
            break;
        }
        let time2 = r.read_float()?;
        let mut values2: Vec<f32> = Vec::with_capacity(entries - 1);
        for _ in 0..(entries - 1) {
            values2.push(r.read_float()? * scale);
        }
        let ctype = r.read_sbyte()?;
        curves.push(ctype as f32);
        match ctype {
            CURVE_LINEAR | CURVE_STEPPED => {}
            CURVE_BEZIER => {
                for _ in 0..(entries - 1) {
                    // Each value channel gets 4 bezier control floats on the wire.
                    curves.push(r.read_float()?);
                    curves.push(r.read_float()?);
                    curves.push(r.read_float()?);
                    curves.push(r.read_float()?);
                }
            }
            other => {
                return Err(BinaryError::UnknownDiscriminant {
                    at: r.position(),
                    entity: "curve type",
                    value: other as u32,
                });
            }
        }
        time = time2;
        values = values2;
    }

    // bezier_count is informational — we've just captured the actual bezier
    // segment payload based on the per-frame curve type bytes.
    let _ = bezier_count;
    Ok(CurveFrames { frames, curves })
}

/// Color timelines use byte-packed channels (each frame's color values are
/// stored as `u8 / 255`). `channels` is the number of color channels per
/// frame — 1 for Alpha, 3 for RGB, 4 for RGBA, 6 for RGB2, 7 for RGBA2.
fn read_color_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    _bezier_count: usize,
    channels: usize,
) -> Result<CurveFrames, BinaryError> {
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * (1 + channels));
    let mut curves: Vec<f32> = Vec::new();
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    let frame_last = frame_count - 1;

    let mut time = r.read_float()?;
    let mut values: Vec<f32> = Vec::with_capacity(channels);
    for _ in 0..channels {
        values.push(f32::from(r.read_byte()?) / 255.0);
    }
    for frame in 0..frame_count {
        frames.push(time);
        for v in &values {
            frames.push(*v);
        }
        if frame == frame_last {
            break;
        }
        let time2 = r.read_float()?;
        let mut values2: Vec<f32> = Vec::with_capacity(channels);
        for _ in 0..channels {
            values2.push(f32::from(r.read_byte()?) / 255.0);
        }
        let ctype = r.read_sbyte()?;
        curves.push(ctype as f32);
        if ctype == CURVE_BEZIER {
            for _ in 0..channels {
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

/// IK constraint timeline: per-frame (time, flags, mix?, softness?) with two
/// bezier channels on bezier frames (mix, softness).
fn read_ik_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    _bezier_count: usize,
    scale: f32,
) -> Result<CurveFrames, BinaryError> {
    // We capture as: frames = [time, mix, softness, bend_direction,
    // compress_flag, stretch_flag], curves = per-frame-pair curve data.
    // Phase 3 decodes these back into solver-ready fields.
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 6);
    let mut curves: Vec<f32> = Vec::new();
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    let frame_last = frame_count - 1;
    let mut flags = r.read_byte()?;
    let mut time = r.read_float()?;
    let mut mix = if flags & 1 != 0 {
        if flags & 2 != 0 { r.read_float()? } else { 1.0 }
    } else {
        0.0
    };
    let mut softness = if flags & 4 != 0 {
        r.read_float()? * scale
    } else {
        0.0
    };
    for frame in 0..frame_count {
        frames.push(time);
        frames.push(mix);
        frames.push(softness);
        frames.push(if flags & 8 != 0 { 1.0 } else { -1.0 });
        frames.push(if flags & 16 != 0 { 1.0 } else { 0.0 });
        frames.push(if flags & 32 != 0 { 1.0 } else { 0.0 });
        if frame == frame_last {
            break;
        }
        flags = r.read_byte()?;
        let time2 = r.read_float()?;
        let mix2 = if flags & 1 != 0 {
            if flags & 2 != 0 { r.read_float()? } else { 1.0 }
        } else {
            0.0
        };
        let softness2 = if flags & 4 != 0 {
            r.read_float()? * scale
        } else {
            0.0
        };
        if flags & 64 != 0 {
            curves.push(CURVE_STEPPED as f32);
        } else if flags & 128 != 0 {
            curves.push(CURVE_BEZIER as f32);
            for _ in 0..2 {
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
                curves.push(r.read_float()?);
            }
        } else {
            curves.push(CURVE_LINEAR as f32);
        }
        time = time2;
        mix = mix2;
        softness = softness2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Deform timeline: each frame is a sparse vertex-offset array. Returns
/// `(frame_times, curve_data, per_frame_vertex_deltas)`.
/// Return value of [`read_deform_timeline`]: frame times, per-segment curve
/// data, and one Vec-of-vertex-deltas per frame.
type DeformTimelineData = (Vec<f32>, Vec<f32>, Vec<Vec<f32>>);

fn read_deform_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    _bezier_count: usize,
    deform_length: usize,
    scale: f32,
) -> Result<DeformTimelineData, BinaryError> {
    let mut frames = Vec::with_capacity(frame_count);
    let mut curves: Vec<f32> = Vec::new();
    let mut vertices: Vec<Vec<f32>> = Vec::with_capacity(frame_count);
    if frame_count == 0 {
        return Ok((frames, curves, vertices));
    }
    let frame_last = frame_count - 1;
    let mut time = r.read_float()?;
    for frame in 0..frame_count {
        let mut deform = vec![0.0f32; deform_length];
        let end = r.read_uvarint()?;
        if end != 0 {
            let start = r.read_uvarint()?;
            let end = start + end;
            if (scale - 1.0).abs() < f32::EPSILON {
                for slot in &mut deform[start..end] {
                    *slot = r.read_float()?;
                }
            } else {
                for slot in &mut deform[start..end] {
                    *slot = r.read_float()? * scale;
                }
            }
        }
        frames.push(time);
        vertices.push(deform);
        if frame == frame_last {
            break;
        }
        let time2 = r.read_float()?;
        let ctype = r.read_sbyte()?;
        curves.push(ctype as f32);
        if ctype == CURVE_BEZIER {
            curves.push(r.read_float()?);
            curves.push(r.read_float()?);
            curves.push(r.read_float()?);
            curves.push(r.read_float()?);
        }
        time = time2;
    }
    Ok((frames, curves, vertices))
}

/// Deform-frame length for a given attachment. Matches spine-cpp:
/// weighted meshes store `vertices.len() / 3 * 2`, unweighted just
/// `vertices.len()`.
fn deform_frame_len(sd: &SkeletonData, att: AttachmentId) -> usize {
    let Some(attachment) = sd.attachments.get(att.index()) else {
        return 0;
    };
    let vd: &VertexData = match attachment {
        Attachment::Mesh(m) => &m.vertex_data,
        Attachment::BoundingBox(b) => &b.vertex_data,
        Attachment::Path(p) => &p.vertex_data,
        Attachment::Clipping(c) => &c.vertex_data,
        _ => return 0,
    };
    if vd.bones.is_empty() {
        vd.vertices.len()
    } else {
        vd.vertices.len() / 3 * 2
    }
}

/// Duration of an animation = max timestamp of any timeline's last frame.
/// Used before Phase 3 can properly compute durations per-variant.
fn timeline_duration(anim: &Animation) -> f32 {
    let mut max = 0.0f32;
    for t in &anim.timelines {
        let last = match t {
            Timeline::Rotate { curves, .. }
            | Timeline::Translate { curves, .. }
            | Timeline::TranslateX { curves, .. }
            | Timeline::TranslateY { curves, .. }
            | Timeline::Scale { curves, .. }
            | Timeline::ScaleX { curves, .. }
            | Timeline::ScaleY { curves, .. }
            | Timeline::Shear { curves, .. }
            | Timeline::ShearX { curves, .. }
            | Timeline::ShearY { curves, .. }
            | Timeline::Rgba { curves, .. }
            | Timeline::Rgb { curves, .. }
            | Timeline::Alpha { curves, .. }
            | Timeline::Rgba2 { curves, .. }
            | Timeline::Rgb2 { curves, .. }
            | Timeline::IkConstraint { curves, .. }
            | Timeline::TransformConstraint { curves, .. }
            | Timeline::PathConstraintPosition { curves, .. }
            | Timeline::PathConstraintSpacing { curves, .. }
            | Timeline::PathConstraintMix { curves, .. }
            | Timeline::Physics { curves, .. } => {
                curves.frames.first().copied().unwrap_or(0.0).max(
                    // Last frame's time is frames[len - stride], but we don't
                    // know stride here. Use the max f32 in the entire frames
                    // vector as a conservative duration estimate — it'll be at
                    // least the last keyframe's time since frames are monotonic.
                    curves.frames.iter().copied().fold(0.0f32, f32::max),
                )
            }
            Timeline::Inherit { frames, .. }
            | Timeline::PhysicsReset { frames, .. }
            | Timeline::DrawOrder { frames, .. } => frames.last().copied().unwrap_or(0.0),
            Timeline::Attachment { frames, .. } => frames.last().copied().unwrap_or(0.0),
            Timeline::Deform { curves, .. } => curves.frames.last().copied().unwrap_or(0.0),
            Timeline::Sequence { frames, .. } => {
                // Sequence frames are interleaved (time, packed, delay); step 3.
                frames.iter().step_by(3).copied().fold(0.0f32, f32::max)
            }
            Timeline::Event { frames, .. } => frames.last().copied().unwrap_or(0.0),
        };
        max = max.max(last);
    }
    max
}
