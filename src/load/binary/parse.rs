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

//! Binary `.skel` parser. Entry point is [`SkeletonBinary`].

// Keeps the parser's structure diffable against SkeletonBinary.cpp.
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

use crate::animation::{BEZIER_SIZE, compute_bezier_samples};
use crate::data::attachment::{Attachment, Sequence, VertexData};
use std::sync::Arc;

use crate::data::{
    Animation, AnimationEvent, AnimationId, AttachmentId, AttachmentRef, BlendMode, BoneData,
    BoneId, ConstraintData, ConstraintId, CurveFrames, EventData, EventId, FromProperty,
    IkConstraintData, Inherit, PathConstraintData, PhysicsConstraintData, PhysicsProperty,
    PositionMode, RotateMode, ScaleYMode, SkeletonData, Skin, SkinId, SliderData, SliderProperty,
    SlotData, SlotId, SpacingMode, Timeline, ToProperty, TransformConstraintData,
    TransformProperty,
};
use crate::load::AttachmentLoader;

use super::reader::{BinaryError, BinaryReader};

/// Exports must report a version starting with this.
pub const TARGET_VERSION: &str = "4.3";

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

const CONSTRAINT_IK: u8 = 0;
const CONSTRAINT_PATH: u8 = 1;
const CONSTRAINT_TRANSFORM: u8 = 2;
const CONSTRAINT_PHYSICS: u8 = 3;
const CONSTRAINT_SLIDER: u8 = 4;

const ATTACHMENT_DEFORM: u8 = 0;
const ATTACHMENT_SEQUENCE: u8 = 1;

const PATH_POSITION: u8 = 0;
const PATH_SPACING: u8 = 1;
const PATH_MIX: u8 = 2;

const PHYSICS_INERTIA: u8 = 0;
const PHYSICS_STRENGTH: u8 = 1;
const PHYSICS_DAMPING: u8 = 2;
const PHYSICS_MASS: u8 = 4;
const PHYSICS_WIND: u8 = 5;
const PHYSICS_GRAVITY: u8 = 6;
const PHYSICS_MIX: u8 = 7;
const PHYSICS_RESET: u8 = 8;

const SLIDER_TIME: u8 = 0;
const SLIDER_MIX: u8 = 1;

const CURVE_LINEAR: i8 = 0;
const CURVE_STEPPED: i8 = 1;
const CURVE_BEZIER: i8 = 2;

/// A linked mesh waiting for its source, which may sit in a later skin.
struct LinkedMesh {
    mesh: AttachmentId,
    skin_index: usize,
    source_slot: usize,
    source: String,
    inherit_timelines: bool,
}

/// Parser for binary `.skel` exports.
///
/// Build one with [`with_loader`](Self::with_loader), optionally set
/// [`with_scale`](Self::with_scale), then consume it with
/// [`read`](Self::read).
pub struct SkeletonBinary<'loader> {
    loader: &'loader mut dyn AttachmentLoader,
    scale: f32,
    linked_meshes: Vec<LinkedMesh>,
}

impl<'loader> SkeletonBinary<'loader> {
    /// Parser that creates attachments through `loader`, at scale 1.
    pub fn with_loader(loader: &'loader mut dyn AttachmentLoader) -> Self {
        Self {
            loader,
            scale: 1.0,
            linked_meshes: Vec::new(),
        }
    }

    /// Multiplies values in skeleton units as they load (bone translation and
    /// length, attachment geometry, translation keys, IK softness, physics
    /// limit, `reference_scale`, and so on). Rotation, scale and shear are
    /// unaffected. Defaults to 1.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Parses a whole `.skel` buffer.
    ///
    /// # Errors
    /// [`BinaryError::UnsupportedVersion`] if the export's version does not
    /// start with `4.3`, [`BinaryError::AttachmentLoader`] if the loader
    /// fails, and the other [`BinaryError`] variants for truncated or
    /// malformed content.
    ///
    /// # Panics
    /// Some malformed deform or draw-order data indexes past its buffer and
    /// panics instead of returning an error.
    pub fn read(mut self, bytes: &[u8]) -> Result<SkeletonData, BinaryError> {
        let scale = self.scale;
        let mut r = BinaryReader::new(bytes);
        let mut sd = SkeletonData::default();
        self.linked_meshes.clear();

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
        sd.reference_scale = r.read_float()? * scale;

        let nonessential = r.read_bool()?;
        if nonessential {
            sd.fps = r.read_float()?;
            sd.images_path = r.read_string()?.unwrap_or_default();
            sd.audio_path = r.read_string()?.unwrap_or_default();
        }

        let num_strings = r.read_uvarint()?;
        let mut strings = Vec::with_capacity(num_strings);
        for _ in 0..num_strings {
            strings.push(r.read_string()?.unwrap_or_default());
        }

        // Bones.
        let num_bones = r.read_uvarint()?;
        sd.bones.reserve(num_bones);
        for i in 0..num_bones {
            let name = r.read_string()?.unwrap_or_default();
            let parent = if i == 0 {
                None
            } else {
                Some(read_bone(&mut r, &sd)?)
            };
            let mut b = BoneData::new(BoneId(i as u16), name, parent);
            let setup = &mut b.setup;
            setup.rotation = r.read_float()?;
            setup.x = r.read_float()? * scale;
            setup.y = r.read_float()? * scale;
            setup.scale_x = r.read_float()?;
            setup.scale_y = r.read_float()?;
            setup.shear_x = r.read_float()?;
            setup.shear_y = r.read_float()?;
            setup.inherit = read_inherit(&mut r)?;
            b.length = r.read_float()? * scale;
            b.skin_required = r.read_bool()?;
            if nonessential {
                b.color = r.read_color()?;
                b.icon = r.read_string()?.unwrap_or_default();
                b.icon_size = r.read_float()?;
                b.icon_rotation = r.read_float()?;
                b.visible = r.read_bool()?;
            }
            sd.bones.push(b);
        }

        // Slots.
        let num_slots = r.read_uvarint()?;
        sd.slots.reserve(num_slots);
        for i in 0..num_slots {
            let name = r.read_string()?.unwrap_or_default();
            let bone = read_bone(&mut r, &sd)?;
            let mut slot = SlotData::new(SlotId(i as u16), name, bone);
            slot.color = r.read_color()?;
            let dark = r.read_int()?;
            if dark != -1 {
                slot.dark_color = Some(crate::math::Color::new(
                    ((dark >> 16) & 0xff) as f32 / 255.0,
                    ((dark >> 8) & 0xff) as f32 / 255.0,
                    (dark & 0xff) as f32 / 255.0,
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

        // Constraints, in update order.
        let num_constraints = r.read_uvarint()?;
        sd.constraints.reserve(num_constraints);
        for _ in 0..num_constraints {
            let name = r.read_string()?.unwrap_or_default();
            let kind = r.read_byte()?;
            let constraint = match kind {
                CONSTRAINT_IK => {
                    let bones = read_bones(&mut r, &sd)?;
                    let target = read_bone(&mut r, &sd)?;
                    let mut data = IkConstraintData::new(name, target);
                    data.bones = bones;
                    let flags = r.read_byte()?;
                    data.skin_required = flags & 1 != 0;
                    if flags & 2 != 0 {
                        let v = u32::from(r.read_byte()?);
                        data.scale_y_mode =
                            ScaleYMode::from_index(v).ok_or(BinaryError::UnknownDiscriminant {
                                at: r.position(),
                                entity: "scale Y mode",
                                value: v,
                            })?;
                    }
                    let setup = &mut data.setup;
                    setup.bend_direction = if flags & 4 != 0 { -1 } else { 1 };
                    setup.compress = flags & 8 != 0;
                    setup.stretch = flags & 16 != 0;
                    if flags & 32 != 0 {
                        setup.mix = if flags & 64 != 0 {
                            r.read_float()?
                        } else {
                            1.0
                        };
                    }
                    if flags & 128 != 0 {
                        setup.softness = r.read_float()? * scale;
                    }
                    ConstraintData::Ik(data)
                }
                CONSTRAINT_TRANSFORM => {
                    let bones = read_bones(&mut r, &sd)?;
                    let source = read_bone(&mut r, &sd)?;
                    let mut data = TransformConstraintData::new(name, source);
                    data.bones = bones;
                    let flags = r.read_byte()?;
                    data.skin_required = flags & 1 != 0;
                    data.local_source = flags & 2 != 0;
                    data.local_target = flags & 4 != 0;
                    data.additive = flags & 8 != 0;
                    data.clamp = flags & 16 != 0;
                    for _ in 0..(flags >> 5) {
                        let from_type = r.read_sbyte()?;
                        let from = TransformProperty::from_index(i32::from(from_type)).ok_or(
                            BinaryError::UnknownDiscriminant {
                                at: r.position(),
                                entity: "transform from property",
                                value: from_type as u32,
                            },
                        )?;
                        let from_scale = property_scale(from, scale);
                        let offset = r.read_float()? * from_scale;
                        let to_count = r.read_sbyte()?;
                        let mut to = Vec::with_capacity(to_count.max(0) as usize);
                        for _ in 0..to_count {
                            let to_type = r.read_sbyte()?;
                            let property = TransformProperty::from_index(i32::from(to_type))
                                .ok_or(BinaryError::UnknownDiscriminant {
                                    at: r.position(),
                                    entity: "transform to property",
                                    value: to_type as u32,
                                })?;
                            let to_scale = property_scale(property, scale);
                            to.push(ToProperty {
                                property,
                                offset: r.read_float()? * to_scale,
                                max: r.read_float()? * to_scale,
                                scale: r.read_float()? * to_scale / from_scale,
                            });
                        }
                        data.properties.push(FromProperty {
                            property: from,
                            offset,
                            to,
                        });
                    }
                    let flags = r.read_byte()?;
                    for (bit, property) in [
                        (1, TransformProperty::Rotate),
                        (2, TransformProperty::X),
                        (4, TransformProperty::Y),
                        (8, TransformProperty::ScaleX),
                        (16, TransformProperty::ScaleY),
                        (32, TransformProperty::ShearY),
                    ] {
                        if flags & bit != 0 {
                            data.offsets[property.offset_index()] =
                                r.read_float()? * property_scale(property, scale);
                        }
                    }
                    let flags = r.read_byte()?;
                    let setup = &mut data.setup;
                    if flags & 1 != 0 {
                        setup.mix_rotate = r.read_float()?;
                    }
                    if flags & 2 != 0 {
                        setup.mix_x = r.read_float()?;
                    }
                    if flags & 4 != 0 {
                        setup.mix_y = r.read_float()?;
                    }
                    if flags & 8 != 0 {
                        setup.mix_scale_x = r.read_float()?;
                    }
                    if flags & 16 != 0 {
                        setup.mix_scale_y = r.read_float()?;
                    }
                    if flags & 32 != 0 {
                        setup.mix_shear_y = r.read_float()?;
                    }
                    ConstraintData::Transform(data)
                }
                CONSTRAINT_PATH => {
                    let bones = read_bones(&mut r, &sd)?;
                    let slot = read_slot(&mut r, &sd)?;
                    let mut data = PathConstraintData::new(name, slot);
                    data.bones = bones;
                    let flags = r.read_byte()?;
                    data.skin_required = flags & 1 != 0;
                    data.position_mode = if (flags >> 1) & 1 == 0 {
                        PositionMode::Fixed
                    } else {
                        PositionMode::Percent
                    };
                    data.spacing_mode = match (flags >> 2) & 3 {
                        0 => SpacingMode::Length,
                        1 => SpacingMode::Fixed,
                        2 => SpacingMode::Percent,
                        _ => SpacingMode::Proportional,
                    };
                    data.rotate_mode = match (flags >> 4) & 3 {
                        0 => RotateMode::Tangent,
                        1 => RotateMode::Chain,
                        _ => RotateMode::ChainScale,
                    };
                    if flags & 128 != 0 {
                        data.offset_rotation = r.read_float()?;
                    }
                    let setup = &mut data.setup;
                    setup.position = r.read_float()?;
                    if data.position_mode == PositionMode::Fixed {
                        setup.position *= scale;
                    }
                    setup.spacing = r.read_float()?;
                    if matches!(data.spacing_mode, SpacingMode::Length | SpacingMode::Fixed) {
                        setup.spacing *= scale;
                    }
                    setup.mix_rotate = r.read_float()?;
                    setup.mix_x = r.read_float()?;
                    setup.mix_y = r.read_float()?;
                    ConstraintData::Path(data)
                }
                CONSTRAINT_PHYSICS => {
                    let bone = read_bone(&mut r, &sd)?;
                    let mut data = PhysicsConstraintData::new(name, bone);
                    let flags = r.read_byte()?;
                    data.skin_required = flags & 1 != 0;
                    if flags & 2 != 0 {
                        data.x = r.read_float()?;
                    }
                    if flags & 4 != 0 {
                        data.y = r.read_float()?;
                    }
                    if flags & 8 != 0 {
                        data.rotate = r.read_float()?;
                    }
                    if flags & 16 != 0 {
                        // Negative values carry the scale Y mode.
                        let mut scale_x = r.read_float()?;
                        if scale_x < -2.0 {
                            data.scale_y_mode = ScaleYMode::Volume;
                            scale_x = -2.0 - scale_x;
                        } else if scale_x < 0.0 {
                            data.scale_y_mode = ScaleYMode::Uniform;
                            scale_x = -1.0 - scale_x;
                        }
                        data.scale_x = scale_x;
                    }
                    if flags & 32 != 0 {
                        data.shear_x = r.read_float()?;
                    }
                    data.limit = if flags & 64 != 0 {
                        r.read_float()?
                    } else {
                        5000.0
                    } * scale;
                    data.step = 1.0 / f32::from(r.read_byte()?);
                    let setup = &mut data.setup;
                    setup.inertia = r.read_float()?;
                    setup.strength = r.read_float()?;
                    setup.damping = r.read_float()?;
                    setup.mass_inverse = if flags & 128 != 0 {
                        r.read_float()?
                    } else {
                        1.0
                    };
                    setup.wind = r.read_float()?;
                    setup.gravity = r.read_float()?;
                    let flags = r.read_byte()?;
                    data.inertia_global = flags & 1 != 0;
                    data.strength_global = flags & 2 != 0;
                    data.damping_global = flags & 4 != 0;
                    data.mass_global = flags & 8 != 0;
                    data.wind_global = flags & 16 != 0;
                    data.gravity_global = flags & 32 != 0;
                    data.mix_global = flags & 64 != 0;
                    data.setup.mix = if flags & 128 != 0 {
                        r.read_float()?
                    } else {
                        1.0
                    };
                    ConstraintData::Physics(data)
                }
                CONSTRAINT_SLIDER => {
                    let mut data = SliderData::new(name);
                    let flags = r.read_byte()?;
                    data.skin_required = flags & 1 != 0;
                    data.looping = flags & 2 != 0;
                    data.additive = flags & 4 != 0;
                    if flags & 8 != 0 {
                        let value = r.read_float()?;
                        if nonessential && flags & 64 != 0 {
                            data.max = value;
                        } else {
                            data.setup.time = value;
                        }
                    }
                    if flags & 16 != 0 {
                        data.setup.mix = if flags & 32 != 0 {
                            r.read_float()?
                        } else {
                            1.0
                        };
                    }
                    if flags & 64 != 0 {
                        data.local = flags & 128 != 0;
                        data.bone = Some(read_bone(&mut r, &sd)?);
                        let offset = r.read_float()?;
                        let kind = r.read_sbyte()?;
                        // Like spine-cpp, an unknown property still carries its
                        // offset and scale.
                        let property = TransformProperty::from_index(i32::from(kind));
                        let property_scale = property.map_or(1.0, |p| property_scale(p, scale));
                        data.property = property.map(|property| SliderProperty {
                            property,
                            offset: offset * property_scale,
                        });
                        data.offset = r.read_float()?;
                        data.scale = r.read_float()? / property_scale;
                    }
                    ConstraintData::Slider(data)
                }
                other => {
                    return Err(BinaryError::UnknownDiscriminant {
                        at: r.position(),
                        entity: "constraint type",
                        value: u32::from(other),
                    });
                }
            };
            sd.constraints.push(constraint);
        }

        if let Some(skin) = self.read_skin(&mut r, true, &mut sd, &strings, nonessential)? {
            sd.default_skin = Some(SkinId(sd.skins.len() as u16));
            sd.skins.push(Arc::new(skin));
        }

        let num_skins = r.read_uvarint()?;
        for _ in 0..num_skins {
            let skin = self
                .read_skin(&mut r, false, &mut sd, &strings, nonessential)?
                .expect("named skins always return Some");
            sd.skins.push(Arc::new(skin));
        }

        self.resolve_linked_meshes(&mut sd)?;

        // Events.
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

        // Animations.
        let num_anims = r.read_uvarint()?;
        sd.animations.reserve(num_anims);
        for _ in 0..num_anims {
            let name = r.read_string()?.unwrap_or_default();
            let anim = self.read_animation(&mut r, &sd, &strings, name, nonessential)?;
            sd.animations.push(anim);
        }

        // Slider animations are written after the animations they reference.
        for c in &mut sd.constraints {
            if let ConstraintData::Slider(slider) = c {
                let idx = r.read_uvarint()?;
                check_index(&r, "animation", idx, sd.animations.len())?;
                slider.animation = Some(AnimationId(idx as u16));
            }
        }

        sd.intern_attachment_keys();
        Ok(sd)
    }

    fn read_skin(
        &mut self,
        r: &mut BinaryReader<'_>,
        default_skin: bool,
        sd: &mut SkeletonData,
        strings: &[String],
        nonessential: bool,
    ) -> Result<Option<Skin>, BinaryError> {
        let (mut skin, slot_count) = if default_skin {
            let sc = r.read_uvarint()?;
            if sc == 0 {
                return Ok(None);
            }
            (Skin::new("default"), sc)
        } else {
            let mut skin = Skin::new(r.read_string()?.unwrap_or_default());
            if nonessential {
                skin.color = r.read_color()?;
            }
            skin.bones = read_bones(r, sd)?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let idx = r.read_uvarint()?;
                check_index(r, "constraint", idx, sd.constraints.len())?;
                skin.constraints.push(ConstraintId(idx as u16));
            }
            let sc = r.read_uvarint()?;
            (skin, sc)
        };

        for _ in 0..slot_count {
            let slot_idx = r.read_uvarint()?;
            check_index(r, "slot", slot_idx, sd.slots.len())?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let placeholder = r.read_string_ref(strings)?.unwrap_or_default();
                let attachment = self.read_attachment(
                    r,
                    slot_idx,
                    &skin.name,
                    &placeholder,
                    sd,
                    strings,
                    nonessential,
                )?;
                if let Some(attachment) = attachment {
                    let id = AttachmentId(sd.attachments.len() as u32);
                    sd.attachments.push(attachment);
                    let key = sd.skin_keys.intern(SlotId(slot_idx as u16), &placeholder);
                    skin.set(key, AttachmentRef::Data(id));
                }
            }
        }
        Ok(Some(skin))
    }

    fn read_sequence(
        &self,
        r: &mut BinaryReader<'_>,
        has_path_suffix: bool,
    ) -> Result<Sequence, BinaryError> {
        if !has_path_suffix {
            return Ok(Sequence::new(1, false));
        }
        let mut seq = Sequence::new(r.read_uvarint()?, true);
        seq.start = r.read_uvarint()? as i32;
        seq.digits = r.read_uvarint()? as i32;
        seq.setup_index = r.read_uvarint()? as i32;
        Ok(seq)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_attachment(
        &mut self,
        r: &mut BinaryReader<'_>,
        slot_idx: usize,
        skin_name: &str,
        placeholder: &str,
        sd: &SkeletonData,
        strings: &[String],
        nonessential: bool,
    ) -> Result<Option<Attachment>, BinaryError> {
        let scale = self.scale;
        let flags = r.read_byte()?;
        let name = if flags & 8 != 0 {
            r.read_string_ref(strings)?.unwrap_or_default()
        } else {
            placeholder.to_string()
        };

        Ok(match flags & 0x7 {
            // Region
            0 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?
                } else {
                    None
                };
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let sequence = self.read_sequence(r, flags & 64 != 0)?;
                let rotation = if flags & 128 != 0 {
                    r.read_float()?
                } else {
                    0.0
                };
                let x = r.read_float()?;
                let y = r.read_float()?;
                let scale_x = r.read_float()?;
                let scale_y = r.read_float()?;
                let width = r.read_float()?;
                let height = r.read_float()?;

                let path = path.unwrap_or_else(|| name.clone());
                let Some(mut region) = self.loader.new_region_attachment(
                    skin_name,
                    placeholder,
                    &name,
                    &path,
                    sequence,
                )?
                else {
                    return Ok(None);
                };
                region.path = path;
                region.x = x * scale;
                region.y = y * scale;
                region.scale_x = scale_x;
                region.scale_y = scale_y;
                region.rotation = rotation;
                region.width = width * scale;
                region.height = height * scale;
                region.color = color;
                region.update_sequence();
                Some(Attachment::Region(region))
            }

            // BoundingBox
            1 => {
                let vertex_data = self.read_vertices(r, flags & 16 != 0)?;
                let color = if nonessential {
                    Some(r.read_color()?)
                } else {
                    None
                };
                let Some(mut bb) =
                    self.loader
                        .new_bounding_box_attachment(skin_name, placeholder, &name)?
                else {
                    return Ok(None);
                };
                bb.vertex_data = vertex_data;
                if let Some(color) = color {
                    bb.color = color;
                }
                Some(Attachment::BoundingBox(bb))
            }

            // Mesh
            2 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?
                } else {
                    None
                }
                .unwrap_or_else(|| name.clone());
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let sequence = self.read_sequence(r, flags & 64 != 0)?;
                let hull_length = r.read_uvarint()?;
                let vertex_data = self.read_vertices(r, flags & 128 != 0)?;
                let vertices_len = vertex_data.world_vertices_length as usize;
                let uvs = read_float_array(r, vertices_len, 1.0)?;
                // `vertices_len` is twice the vertex count, so this is
                // 2V - hull - 2, the triangle count of a triangulated polygon.
                let tri_count = (vertices_len as i32 - hull_length as i32 - 2).max(0) as usize * 3;
                let triangles = read_short_array(r, tri_count)?;
                let n = r.read_uvarint()?;
                let mut timeline_slots = Vec::with_capacity(n);
                for _ in 0..n {
                    timeline_slots.push(read_slot(r, sd)?);
                }
                let (edges, width, height) = if nonessential {
                    let n = r.read_uvarint()?;
                    let e = read_short_array(r, n)?;
                    (e, r.read_float()?, r.read_float()?)
                } else {
                    (Vec::new(), 0.0, 0.0)
                };

                let Some(mut mesh) = self.loader.new_mesh_attachment(
                    skin_name,
                    placeholder,
                    &name,
                    &path,
                    sequence,
                )?
                else {
                    return Ok(None);
                };
                mesh.path = path;
                mesh.color = color;
                mesh.hull_length = (hull_length as u32) << 1;
                mesh.vertex_data = vertex_data;
                mesh.vertex_data.timeline.slots = timeline_slots;
                mesh.region_uvs = uvs;
                mesh.triangles = triangles;
                if nonessential {
                    mesh.edges = edges;
                    mesh.width = width * scale;
                    mesh.height = height * scale;
                }
                mesh.update_sequence();
                Some(Attachment::Mesh(mesh))
            }

            // LinkedMesh
            3 => {
                let path = if flags & 16 != 0 {
                    r.read_string_ref(strings)?
                } else {
                    None
                }
                .unwrap_or_else(|| name.clone());
                let color = if flags & 32 != 0 {
                    r.read_color()?
                } else {
                    crate::math::Color::WHITE
                };
                let sequence = self.read_sequence(r, flags & 64 != 0)?;
                let inherit_timelines = flags & 128 != 0;
                let source_slot = r.read_uvarint()?;
                check_index(r, "slot", source_slot, sd.slots.len())?;
                let skin_index = r.read_uvarint()?;
                let source = r.read_string_ref(strings)?.unwrap_or_default();
                let (width, height) = if nonessential {
                    (r.read_float()?, r.read_float()?)
                } else {
                    (0.0, 0.0)
                };

                let Some(mut mesh) = self.loader.new_mesh_attachment(
                    skin_name,
                    placeholder,
                    &name,
                    &path,
                    sequence,
                )?
                else {
                    return Ok(None);
                };
                mesh.path = path;
                mesh.color = color;
                if nonessential {
                    mesh.width = width * scale;
                    mesh.height = height * scale;
                }
                // The caller pushes this attachment next.
                self.linked_meshes.push(LinkedMesh {
                    mesh: AttachmentId(sd.attachments.len() as u32),
                    skin_index,
                    source_slot,
                    source,
                    inherit_timelines,
                });
                let _ = slot_idx;
                Some(Attachment::Mesh(mesh))
            }

            // Path
            4 => {
                let closed = flags & 16 != 0;
                let constant_speed = flags & 32 != 0;
                let vertex_data = self.read_vertices(r, flags & 64 != 0)?;
                let lengths =
                    read_float_array(r, vertex_data.world_vertices_length as usize / 6, scale)?;
                let color = if nonessential {
                    Some(r.read_color()?)
                } else {
                    None
                };
                let Some(mut path) =
                    self.loader
                        .new_path_attachment(skin_name, placeholder, &name)?
                else {
                    return Ok(None);
                };
                path.closed = closed;
                path.constant_speed = constant_speed;
                path.vertex_data = vertex_data;
                path.lengths = lengths;
                if let Some(color) = color {
                    path.color = color;
                }
                Some(Attachment::Path(path))
            }

            // Point
            5 => {
                let rotation = r.read_float()?;
                let x = r.read_float()?;
                let y = r.read_float()?;
                let color = if nonessential {
                    Some(r.read_color()?)
                } else {
                    None
                };
                let Some(mut point) =
                    self.loader
                        .new_point_attachment(skin_name, placeholder, &name)?
                else {
                    return Ok(None);
                };
                point.x = x * scale;
                point.y = y * scale;
                point.rotation = rotation;
                if let Some(color) = color {
                    point.color = color;
                }
                Some(Attachment::Point(point))
            }

            // Clipping
            6 => {
                let end_slot = read_slot(r, sd)?;
                let vertex_data = self.read_vertices(r, flags & 16 != 0)?;
                let color = if nonessential {
                    Some(r.read_color()?)
                } else {
                    None
                };
                let Some(mut clip) =
                    self.loader
                        .new_clipping_attachment(skin_name, placeholder, &name)?
                else {
                    return Ok(None);
                };
                clip.end_slot = Some(end_slot);
                clip.convex = flags & 32 != 0;
                clip.inverse = flags & 64 != 0;
                clip.vertex_data = vertex_data;
                if let Some(color) = color {
                    clip.color = color;
                }
                Some(Attachment::Clipping(clip))
            }

            other => {
                return Err(BinaryError::UnknownDiscriminant {
                    at: r.position(),
                    entity: "attachment type",
                    value: u32::from(other),
                });
            }
        })
    }

    fn read_vertices(
        &self,
        r: &mut BinaryReader<'_>,
        weighted: bool,
    ) -> Result<VertexData, BinaryError> {
        let vertex_count = r.read_uvarint()?;
        let mut vd = VertexData {
            world_vertices_length: (vertex_count * 2) as u32,
            ..VertexData::default()
        };
        if !weighted {
            vd.vertices = read_float_array(r, vertex_count * 2, self.scale)?;
            return Ok(vd);
        }
        let n = r.read_uvarint()?;
        vd.bones.reserve(n);
        while vd.bones.len() < n {
            let bone_count = r.read_uvarint()?;
            vd.bones.push(bone_count as i32);
            for _ in 0..bone_count {
                vd.bones.push(r.read_uvarint()? as i32);
                vd.vertices.push(r.read_float()? * self.scale);
                vd.vertices.push(r.read_float()? * self.scale);
                vd.vertices.push(r.read_float()?);
            }
        }
        Ok(vd)
    }

    fn resolve_linked_meshes(&mut self, sd: &mut SkeletonData) -> Result<(), BinaryError> {
        for lm in std::mem::take(&mut self.linked_meshes) {
            if lm.skin_index >= sd.skins.len() {
                return Err(BinaryError::IndexOutOfRange {
                    at: 0,
                    entity: "skin",
                    index: lm.skin_index,
                    len: sd.skins.len(),
                });
            }
            let source_id = sd
                .skin_attachment(
                    SkinId(lm.skin_index as u16),
                    SlotId(lm.source_slot as u16),
                    &lm.source,
                )
                .ok_or_else(|| BinaryError::LinkedMeshParentMissing {
                    at: 0,
                    skin: sd.skins[lm.skin_index].name.clone(),
                    slot: lm.source_slot,
                    parent: lm.source.clone(),
                })?;
            link_mesh(sd, lm.mesh, source_id, lm.inherit_timelines);
        }
        Ok(())
    }

    fn read_animation(
        &mut self,
        r: &mut BinaryReader<'_>,
        sd: &SkeletonData,
        strings: &[String],
        name: String,
        nonessential: bool,
    ) -> Result<Animation, BinaryError> {
        let mut anim = Animation::new(name, 0.0);
        let _num_timelines = r.read_uvarint()?;

        // Slot timelines.
        let slot_groups = r.read_uvarint()?;
        for _ in 0..slot_groups {
            let slot = read_slot(r, sd)?;
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let ttype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                let timeline = match ttype {
                    SLOT_ATTACHMENT => {
                        let mut frames = Vec::with_capacity(frame_count);
                        let mut names = Vec::with_capacity(frame_count);
                        for _ in 0..frame_count {
                            frames.push(r.read_float()?);
                            names.push(r.read_string_ref(strings)?);
                        }
                        Timeline::Attachment {
                            slot,
                            frames,
                            names,
                            keys: Vec::new(),
                        }
                    }
                    SLOT_RGBA | SLOT_RGB | SLOT_RGBA2 | SLOT_RGB2 | SLOT_ALPHA => {
                        let bezier_count = r.read_uvarint()?;
                        let channels = match ttype {
                            SLOT_RGBA => 4,
                            SLOT_RGB => 3,
                            SLOT_RGBA2 => 7,
                            SLOT_RGB2 => 6,
                            _ => 1,
                        };
                        let curves = read_color_timeline(r, frame_count, bezier_count, channels)?;
                        match ttype {
                            SLOT_RGBA => Timeline::Rgba { slot, curves },
                            SLOT_RGB => Timeline::Rgb { slot, curves },
                            SLOT_RGBA2 => Timeline::Rgba2 { slot, curves },
                            SLOT_RGB2 => Timeline::Rgb2 { slot, curves },
                            _ => Timeline::Alpha { slot, curves },
                        }
                    }
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "slot timeline",
                            value: u32::from(other),
                        });
                    }
                };
                anim.timelines.push(timeline);
            }
        }

        // Bone timelines.
        let bone_groups = r.read_uvarint()?;
        for _ in 0..bone_groups {
            let bone = read_bone(r, sd)?;
            anim.bones.push(bone);
            let n = r.read_uvarint()?;
            for _ in 0..n {
                let ttype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                if ttype == BONE_INHERIT {
                    let mut frames = Vec::with_capacity(frame_count);
                    let mut inherits = Vec::with_capacity(frame_count);
                    for _ in 0..frame_count {
                        frames.push(r.read_float()?);
                        inherits.push(read_inherit(r)?);
                    }
                    anim.timelines.push(Timeline::Inherit {
                        bone,
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
                    BONE_SCALE | BONE_SHEAR => (3, 1.0),
                    BONE_SCALE_X | BONE_SCALE_Y | BONE_SHEAR_X | BONE_SHEAR_Y => (2, 1.0),
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
                    BONE_ROTATE => Timeline::Rotate { bone, curves },
                    BONE_TRANSLATE => Timeline::Translate { bone, curves },
                    BONE_TRANSLATE_X => Timeline::TranslateX { bone, curves },
                    BONE_TRANSLATE_Y => Timeline::TranslateY { bone, curves },
                    BONE_SCALE => Timeline::Scale { bone, curves },
                    BONE_SCALE_X => Timeline::ScaleX { bone, curves },
                    BONE_SCALE_Y => Timeline::ScaleY { bone, curves },
                    BONE_SHEAR => Timeline::Shear { bone, curves },
                    BONE_SHEAR_X => Timeline::ShearX { bone, curves },
                    _ => Timeline::ShearY { bone, curves },
                });
            }
        }

        // IK constraint timelines.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let constraint = read_constraint(r, sd, "IK constraint", |c| c.as_ik().is_some())?;
            let frame_count = r.read_uvarint()?;
            let bezier_count = r.read_uvarint()?;
            let curves = read_ik_timeline(r, frame_count, bezier_count, self.scale)?;
            anim.timelines
                .push(Timeline::IkConstraint { constraint, curves });
        }

        // Transform constraint timelines.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let constraint = read_constraint(r, sd, "transform constraint", |c| {
                c.as_transform().is_some()
            })?;
            let frame_count = r.read_uvarint()?;
            let bezier_count = r.read_uvarint()?;
            let curves = read_curve_timeline(r, frame_count, bezier_count, 7, 1.0)?;
            anim.timelines
                .push(Timeline::TransformConstraint { constraint, curves });
        }

        // Path constraint timelines.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let constraint = read_constraint(r, sd, "path constraint", |c| c.as_path().is_some())?;
            let data = sd.constraints[constraint.index()]
                .as_path()
                .expect("checked");
            let sub_n = r.read_uvarint()?;
            for _ in 0..sub_n {
                let ptype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                let bezier_count = r.read_uvarint()?;
                let timeline = match ptype {
                    PATH_POSITION => {
                        let scale = if data.position_mode == PositionMode::Fixed {
                            self.scale
                        } else {
                            1.0
                        };
                        let curves = read_curve_timeline(r, frame_count, bezier_count, 2, scale)?;
                        Timeline::PathConstraintPosition { constraint, curves }
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
                        Timeline::PathConstraintSpacing { constraint, curves }
                    }
                    PATH_MIX => {
                        let curves = read_curve_timeline(r, frame_count, bezier_count, 4, 1.0)?;
                        Timeline::PathConstraintMix { constraint, curves }
                    }
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "path constraint timeline",
                            value: u32::from(other),
                        });
                    }
                };
                anim.timelines.push(timeline);
            }
        }

        // Physics timelines. Index 0 means every physics constraint.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let raw = r.read_uvarint()?;
            let constraint = if raw == 0 {
                None
            } else {
                check_index(r, "constraint", raw - 1, sd.constraints.len())?;
                Some(ConstraintId((raw - 1) as u16))
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

        // Slider timelines.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let constraint = read_constraint(r, sd, "slider", |c| c.as_slider().is_some())?;
            let sub_n = r.read_uvarint()?;
            for _ in 0..sub_n {
                let stype = r.read_byte()?;
                let frame_count = r.read_uvarint()?;
                let bezier_count = r.read_uvarint()?;
                let curves = read_curve_timeline(r, frame_count, bezier_count, 2, 1.0)?;
                anim.timelines.push(match stype {
                    SLIDER_TIME => Timeline::Slider { constraint, curves },
                    SLIDER_MIX => Timeline::SliderMix { constraint, curves },
                    other => {
                        return Err(BinaryError::UnknownDiscriminant {
                            at: r.position(),
                            entity: "slider timeline",
                            value: u32::from(other),
                        });
                    }
                });
            }
        }

        // Attachment timelines.
        let skin_groups = r.read_uvarint()?;
        for _ in 0..skin_groups {
            let skin_idx = r.read_uvarint()?;
            check_index(r, "skin", skin_idx, sd.skins.len())?;
            let slot_groups = r.read_uvarint()?;
            for _ in 0..slot_groups {
                let slot = read_slot(r, sd)?;
                let att_n = r.read_uvarint()?;
                for _ in 0..att_n {
                    let att_name = r.read_string_ref(strings)?.unwrap_or_default();
                    let attachment = sd
                        .skin_attachment(SkinId(skin_idx as u16), slot, &att_name)
                        .ok_or(BinaryError::LinkedMeshParentMissing {
                            at: r.position(),
                            skin: sd.skins[skin_idx].name.clone(),
                            slot: slot.index(),
                            parent: att_name.clone(),
                        })?;
                    let ttype = r.read_byte()?;
                    let frame_count = r.read_uvarint()?;
                    match ttype {
                        ATTACHMENT_DEFORM => {
                            let vertices_len = deform_frame_len(sd, attachment);
                            let (weighted, setup_vertices) = deform_context(sd, attachment);
                            let bezier_count = r.read_uvarint()?;
                            let (frames, curves, vertices) = read_deform_timeline(
                                r,
                                frame_count,
                                bezier_count,
                                vertices_len,
                                self.scale,
                                weighted,
                                &setup_vertices,
                            )?;
                            anim.timelines.push(Timeline::Deform {
                                slot,
                                attachment,
                                curves: CurveFrames { frames, curves },
                                vertices,
                            });
                        }
                        ATTACHMENT_SEQUENCE => {
                            let mut frames = Vec::with_capacity(frame_count * 3);
                            for _ in 0..frame_count {
                                frames.push(r.read_float()?);
                                frames.push(r.read_int()? as f32);
                                frames.push(r.read_float()?);
                            }
                            anim.timelines.push(Timeline::Sequence {
                                slot,
                                attachment,
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

        // Draw order timeline.
        let slot_count = sd.slots.len();
        let n = r.read_uvarint()?;
        if n > 0 {
            let mut frames = Vec::with_capacity(n);
            let mut draw_orders = Vec::with_capacity(n);
            for _ in 0..n {
                frames.push(r.read_float()?);
                draw_orders.push(
                    read_draw_order(r, slot_count)?
                        .map(|order| order.into_iter().map(|i| SlotId(i as u16)).collect()),
                );
            }
            anim.timelines.push(Timeline::DrawOrder {
                frames,
                draw_orders,
            });
        }

        // Draw order folder timelines.
        let n = r.read_uvarint()?;
        for _ in 0..n {
            let folder_len = r.read_uvarint()?;
            let mut slots = Vec::with_capacity(folder_len);
            for _ in 0..folder_len {
                slots.push(read_slot(r, sd)?);
            }
            let key_count = r.read_uvarint()?;
            let mut frames = Vec::with_capacity(key_count);
            let mut draw_orders = Vec::with_capacity(key_count);
            for _ in 0..key_count {
                frames.push(r.read_float()?);
                draw_orders.push(
                    read_draw_order(r, folder_len)?
                        .map(|order| order.into_iter().map(|i| i as u16).collect()),
                );
            }
            anim.timelines.push(Timeline::DrawOrderFolder {
                slots,
                frames,
                draw_orders,
            });
        }

        // Event timeline.
        let n = r.read_uvarint()?;
        if n > 0 {
            let mut frames = Vec::with_capacity(n);
            let mut events = Vec::with_capacity(n);
            for _ in 0..n {
                let time = r.read_float()?;
                let ei = r.read_uvarint()?;
                check_index(r, "event", ei, sd.events.len())?;
                let data = &sd.events[ei];
                let int_value = r.read_varint(false)?;
                let float_value = r.read_float()?;
                let string_value = Some(
                    r.read_string()?
                        .unwrap_or_else(|| data.string_value.clone())
                        .into(),
                );
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

        let timelines = std::mem::take(&mut anim.timelines);
        anim.set_timelines(timelines, &sd.attachments);
        anim.duration = timeline_duration(&anim);
        if nonessential {
            anim.color = r.read_color()?;
        }
        Ok(anim)
    }
}

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

fn read_bone(r: &mut BinaryReader<'_>, sd: &SkeletonData) -> Result<BoneId, BinaryError> {
    let idx = r.read_uvarint()?;
    check_index(r, "bone", idx, sd.bones.len())?;
    Ok(BoneId(idx as u16))
}

fn read_bones(r: &mut BinaryReader<'_>, sd: &SkeletonData) -> Result<Vec<BoneId>, BinaryError> {
    let n = r.read_uvarint()?;
    let mut bones = Vec::with_capacity(n);
    for _ in 0..n {
        bones.push(read_bone(r, sd)?);
    }
    Ok(bones)
}

fn read_slot(r: &mut BinaryReader<'_>, sd: &SkeletonData) -> Result<SlotId, BinaryError> {
    let idx = r.read_uvarint()?;
    check_index(r, "slot", idx, sd.slots.len())?;
    Ok(SlotId(idx as u16))
}

/// Reads a constraint index and checks it names the expected kind.
fn read_constraint(
    r: &mut BinaryReader<'_>,
    sd: &SkeletonData,
    entity: &'static str,
    is_kind: impl Fn(&ConstraintData) -> bool,
) -> Result<ConstraintId, BinaryError> {
    let idx = r.read_uvarint()?;
    check_index(r, "constraint", idx, sd.constraints.len())?;
    if !is_kind(&sd.constraints[idx]) {
        return Err(BinaryError::UnknownDiscriminant {
            at: r.position(),
            entity,
            value: idx as u32,
        });
    }
    Ok(ConstraintId(idx as u16))
}

/// Translation channels scale with the skeleton; the rest don't.
fn property_scale(property: TransformProperty, scale: f32) -> f32 {
    match property {
        TransformProperty::X | TransformProperty::Y => scale,
        _ => 1.0,
    }
}

fn read_inherit(r: &mut BinaryReader<'_>) -> Result<Inherit, BinaryError> {
    let v = u32::from(r.read_byte()?);
    Inherit::from_index(v).ok_or(BinaryError::UnknownDiscriminant {
        at: r.position(),
        entity: "inherit mode",
        value: v,
    })
}

/// Rebuilds a full order from `(index, shift)` changes. The shift is an
/// unsigned varint; negative shifts rely on 32-bit wraparound to `i32`, as in
/// spine-cpp.
fn read_draw_order(
    r: &mut BinaryReader<'_>,
    slot_count: usize,
) -> Result<Option<Vec<i32>>, BinaryError> {
    let change_count = r.read_uvarint()?;
    if change_count == 0 {
        return Ok(None);
    }
    let mut draw_order = vec![-1i32; slot_count];
    let mut unchanged = vec![0i32; slot_count.saturating_sub(change_count)];
    let mut original_index: i32 = 0;
    let mut unchanged_index = 0usize;
    for _ in 0..change_count {
        let slot_idx = r.read_uvarint()? as i32;
        while original_index != slot_idx {
            unchanged[unchanged_index] = original_index;
            unchanged_index += 1;
            original_index += 1;
        }
        let shift = r.read_varint(true)?;
        draw_order[(original_index + shift) as usize] = original_index;
        original_index += 1;
    }
    while (original_index as usize) < slot_count {
        unchanged[unchanged_index] = original_index;
        unchanged_index += 1;
        original_index += 1;
    }
    for i in (0..slot_count).rev() {
        if draw_order[i] == -1 {
            unchanged_index -= 1;
            draw_order[i] = unchanged[unchanged_index];
        }
    }
    Ok(Some(draw_order))
}

/// Copies `source`'s geometry into the linked `mesh`, points its deform
/// timelines at `source` when `inherit_timelines`, and recomputes its UVs.
/// Does nothing unless both are meshes. Shared by both loaders.
pub(crate) fn link_mesh(
    sd: &mut SkeletonData,
    mesh: AttachmentId,
    source: AttachmentId,
    inherit_timelines: bool,
) {
    let (m, s) = (mesh.index(), source.index());
    if m == s {
        return;
    }
    let (lo, hi) = sd.attachments.split_at_mut(m.max(s));
    let (mesh_att, source_att) = if m < s {
        (&mut lo[m], &hi[0])
    } else {
        (&mut hi[0], &lo[s])
    };
    let (Attachment::Mesh(mesh_att), Attachment::Mesh(source_att)) = (mesh_att, source_att) else {
        return;
    };
    mesh_att.vertex_data.timeline.attachment = inherit_timelines.then_some(source);
    mesh_att.set_source_mesh(source, source_att);
    mesh_att.update_sequence();
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

/// Reads a curve timeline whose frames are `entries` floats (time, then
/// values). `scale` applies to values, not time.
///
/// `curves` holds a type code per frame in `[0, frame_count)`, then
/// `bezier_count * BEZIER_SIZE` bezier samples. A bezier frame's code is
/// `CURVE_BEZIER` plus the offset of its first channel's samples; later
/// channels follow at `BEZIER_SIZE` strides.
fn read_curve_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    bezier_count: usize,
    entries: usize,
    scale: f32,
) -> Result<CurveFrames, BinaryError> {
    let channels = entries - 1;
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * entries);
    let curves_len = frame_count + bezier_count * BEZIER_SIZE;
    let mut curves: Vec<f32> = vec![0.0_f32; curves_len];

    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    // As in spine-cpp's CurveTimeline constructor: a linear last frame would
    // read past the end of `frames`.
    curves[frame_count - 1] = CURVE_STEPPED as f32;

    let frame_last = frame_count - 1;
    let mut time = r.read_float()?;
    let mut values: Vec<f32> = Vec::with_capacity(channels);
    for _ in 0..channels {
        values.push(r.read_float()? * scale);
    }

    // Counts bezier segments, one per channel per bezier frame.
    let mut bezier_seg_idx: usize = 0;

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
            values2.push(r.read_float()? * scale);
        }
        let ctype = r.read_sbyte()?;
        match ctype {
            CURVE_LINEAR => curves[frame] = CURVE_LINEAR as f32,
            CURVE_STEPPED => curves[frame] = CURVE_STEPPED as f32,
            CURVE_BEZIER => {
                let first_channel_abs = frame_count + bezier_seg_idx * BEZIER_SIZE;
                curves[frame] = (i32::from(CURVE_BEZIER) + first_channel_abs as i32) as f32;
                for k in 0..channels {
                    let cx1 = r.read_float()?;
                    let cy1 = r.read_float()? * scale;
                    let cx2 = r.read_float()?;
                    let cy2 = r.read_float()? * scale;
                    let samples = compute_bezier_samples(
                        time, values[k], cx1, cy1, cx2, cy2, time2, values2[k],
                    );
                    let dst = frame_count + (bezier_seg_idx + k) * BEZIER_SIZE;
                    curves[dst..dst + BEZIER_SIZE].copy_from_slice(&samples);
                }
                bezier_seg_idx += channels;
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

    Ok(CurveFrames { frames, curves })
}

/// Like [`read_curve_timeline`], but values are bytes normalized to
/// `u8 / 255`. `channels` is 1 for alpha, 3 for RGB, 4 for RGBA, 6 for RGB2,
/// 7 for RGBA2.
fn read_color_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    bezier_count: usize,
    channels: usize,
) -> Result<CurveFrames, BinaryError> {
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * (1 + channels));
    let curves_len = frame_count + bezier_count * BEZIER_SIZE;
    let mut curves: Vec<f32> = vec![0.0_f32; curves_len];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED as f32;
    let frame_last = frame_count - 1;

    let mut time = r.read_float()?;
    let mut values: Vec<f32> = Vec::with_capacity(channels);
    for _ in 0..channels {
        values.push(f32::from(r.read_byte()?) / 255.0);
    }
    let mut bezier_seg_idx: usize = 0;
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
        match ctype {
            CURVE_LINEAR => curves[frame] = CURVE_LINEAR as f32,
            CURVE_STEPPED => curves[frame] = CURVE_STEPPED as f32,
            CURVE_BEZIER => {
                let first_channel_abs = frame_count + bezier_seg_idx * BEZIER_SIZE;
                curves[frame] = (i32::from(CURVE_BEZIER) + first_channel_abs as i32) as f32;
                for k in 0..channels {
                    let cx1 = r.read_float()?;
                    let cy1 = r.read_float()?;
                    let cx2 = r.read_float()?;
                    let cy2 = r.read_float()?;
                    let samples = compute_bezier_samples(
                        time, values[k], cx1, cy1, cx2, cy2, time2, values2[k],
                    );
                    let dst = frame_count + (bezier_seg_idx + k) * BEZIER_SIZE;
                    curves[dst..dst + BEZIER_SIZE].copy_from_slice(&samples);
                }
                bezier_seg_idx += channels;
            }
            other => {
                return Err(BinaryError::UnknownDiscriminant {
                    at: r.position(),
                    entity: "color curve type",
                    value: other as u32,
                });
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Reads an IK timeline into frames of `[time, mix, softness,
/// bend_direction, compress, stretch]`. Bezier frames carry two channels, mix
/// and softness. `curves` uses the [`read_curve_timeline`] layout.
fn read_ik_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    bezier_count: usize,
    scale: f32,
) -> Result<CurveFrames, BinaryError> {
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 6);
    let curves_len = frame_count + bezier_count * BEZIER_SIZE;
    let mut curves: Vec<f32> = vec![0.0_f32; curves_len];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED as f32;
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
    let mut bezier_seg_idx: usize = 0;
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
            curves[frame] = CURVE_STEPPED as f32;
        } else if flags & 128 != 0 {
            let first_channel_abs = frame_count + bezier_seg_idx * BEZIER_SIZE;
            curves[frame] = (i32::from(CURVE_BEZIER) + first_channel_abs as i32) as f32;
            let channels = [(mix, mix2, 1.0), (softness, softness2, scale)];
            for (k, (value1, value2, value_scale)) in channels.iter().enumerate() {
                let cx1 = r.read_float()?;
                let cy1 = r.read_float()? * value_scale;
                let cx2 = r.read_float()?;
                let cy2 = r.read_float()? * value_scale;
                let samples =
                    compute_bezier_samples(time, *value1, cx1, cy1, cx2, cy2, time2, *value2);
                let dst = frame_count + (bezier_seg_idx + k) * BEZIER_SIZE;
                curves[dst..dst + BEZIER_SIZE].copy_from_slice(&samples);
            }
            bezier_seg_idx += 2;
        } else {
            curves[frame] = CURVE_LINEAR as f32;
        }
        time = time2;
        mix = mix2;
        softness = softness2;
    }
    Ok(CurveFrames { frames, curves })
}

/// `(frame_times, curves, per_frame_vertices)`.
type DeformTimelineData = (Vec<f32>, Vec<f32>, Vec<Vec<f32>>);

/// Reads a deform timeline. Frames are stored sparsely on the wire and
/// expanded to `deform_length` floats. Weighted frames hold offsets, zero
/// where absent. Unweighted frames hold absolute positions: the setup
/// vertices are added at load time, as spine-cpp does, and apply relies on
/// it. `curves` uses the [`read_curve_timeline`] layout with one channel
/// running 0 to 1.
fn read_deform_timeline(
    r: &mut BinaryReader<'_>,
    frame_count: usize,
    bezier_count: usize,
    deform_length: usize,
    scale: f32,
    weighted: bool,
    setup_vertices: &[f32],
) -> Result<DeformTimelineData, BinaryError> {
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count);
    let curves_len = frame_count + bezier_count * BEZIER_SIZE;
    let mut curves: Vec<f32> = vec![0.0_f32; curves_len];
    let mut vertices: Vec<Vec<f32>> = Vec::with_capacity(frame_count);
    if frame_count == 0 {
        return Ok((frames, curves, vertices));
    }
    curves[frame_count - 1] = CURVE_STEPPED as f32;

    let frame_last = frame_count - 1;
    let mut time = r.read_float()?;
    let mut bezier_seg_idx: usize = 0;

    for frame in 0..frame_count {
        let end = r.read_uvarint()?;
        let deform = if end == 0 {
            if weighted {
                vec![0.0_f32; deform_length]
            } else {
                setup_vertices.to_vec()
            }
        } else {
            let start = r.read_uvarint()?;
            let mut deform = vec![0.0_f32; deform_length];
            let read_end = start + end;
            if (scale - 1.0).abs() < f32::EPSILON {
                for slot in &mut deform[start..read_end] {
                    *slot = r.read_float()?;
                }
            } else {
                for slot in &mut deform[start..read_end] {
                    *slot = r.read_float()? * scale;
                }
            }
            if !weighted {
                for (d, s) in deform.iter_mut().zip(setup_vertices.iter()) {
                    *d += *s;
                }
            }
            deform
        };
        frames.push(time);
        vertices.push(deform);
        if frame == frame_last {
            break;
        }
        let time2 = r.read_float()?;
        let ctype = r.read_sbyte()?;
        match ctype {
            CURVE_LINEAR => curves[frame] = CURVE_LINEAR as f32,
            CURVE_STEPPED => curves[frame] = CURVE_STEPPED as f32,
            CURVE_BEZIER => {
                let tail_offset = frame_count + bezier_seg_idx * BEZIER_SIZE;
                curves[frame] = (i32::from(CURVE_BEZIER) + tail_offset as i32) as f32;
                let cx1 = r.read_float()?;
                let cy1 = r.read_float()?;
                let cx2 = r.read_float()?;
                let cy2 = r.read_float()?;
                let samples = compute_bezier_samples(time, 0.0, cx1, cy1, cx2, cy2, time2, 1.0);
                curves[tail_offset..tail_offset + BEZIER_SIZE].copy_from_slice(&samples);
                bezier_seg_idx += 1;
            }
            other => {
                return Err(BinaryError::UnknownDiscriminant {
                    at: r.position(),
                    entity: "deform curve type",
                    value: other as u32,
                });
            }
        }
        time = time2;
    }
    Ok((frames, curves, vertices))
}

/// `(weighted, setup_vertices)` for an attachment. Setup vertices are cloned
/// only when unweighted, since weighted vertices are bone-local triples, not
/// positions.
fn deform_context(sd: &SkeletonData, att: AttachmentId) -> (bool, Vec<f32>) {
    let Some(attachment) = sd.attachments.get(att.index()) else {
        return (false, Vec::new());
    };
    let vd: &VertexData = match attachment {
        Attachment::Mesh(m) => &m.vertex_data,
        Attachment::BoundingBox(b) => &b.vertex_data,
        Attachment::Path(p) => &p.vertex_data,
        Attachment::Clipping(c) => &c.vertex_data,
        _ => return (false, Vec::new()),
    };
    if vd.bones.is_empty() {
        (false, vd.vertices.clone())
    } else {
        (true, Vec::new())
    }
}

/// Floats per deform frame: `vertices.len() / 3 * 2` for weighted
/// attachments, `vertices.len()` otherwise.
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

/// The latest last-frame time across an animation's timelines.
pub(crate) fn timeline_duration(anim: &Animation) -> f32 {
    fn last_time_stride(frames: &[f32], stride: usize) -> f32 {
        if frames.len() < stride {
            return 0.0;
        }
        frames[frames.len() - stride]
    }

    let mut max = 0.0f32;
    for t in &anim.timelines {
        let last = match t {
            // Time plus one value.
            Timeline::Rotate { curves, .. }
            | Timeline::TranslateX { curves, .. }
            | Timeline::TranslateY { curves, .. }
            | Timeline::ScaleX { curves, .. }
            | Timeline::ScaleY { curves, .. }
            | Timeline::ShearX { curves, .. }
            | Timeline::ShearY { curves, .. }
            | Timeline::Alpha { curves, .. }
            | Timeline::PathConstraintPosition { curves, .. }
            | Timeline::PathConstraintSpacing { curves, .. }
            | Timeline::Physics { curves, .. }
            | Timeline::Slider { curves, .. }
            | Timeline::SliderMix { curves, .. } => last_time_stride(&curves.frames, 2),

            // Time plus two values.
            Timeline::Translate { curves, .. }
            | Timeline::Scale { curves, .. }
            | Timeline::Shear { curves, .. } => last_time_stride(&curves.frames, 3),

            Timeline::Rgba { curves, .. } => last_time_stride(&curves.frames, 5),
            Timeline::Rgb { curves, .. } => last_time_stride(&curves.frames, 4),
            Timeline::Rgba2 { curves, .. } => last_time_stride(&curves.frames, 8),
            Timeline::Rgb2 { curves, .. } => last_time_stride(&curves.frames, 7),
            Timeline::IkConstraint { curves, .. } => last_time_stride(&curves.frames, 6),
            Timeline::TransformConstraint { curves, .. } => last_time_stride(&curves.frames, 7),
            Timeline::PathConstraintMix { curves, .. } => last_time_stride(&curves.frames, 4),

            Timeline::Inherit { frames, .. }
            | Timeline::PhysicsReset { frames, .. }
            | Timeline::DrawOrder { frames, .. }
            | Timeline::DrawOrderFolder { frames, .. }
            | Timeline::Attachment { frames, .. }
            | Timeline::Event { frames, .. } => frames.last().copied().unwrap_or(0.0),

            Timeline::Deform { curves, .. } => last_time_stride(&curves.frames, 1),

            // Time, packed index and mode, delay.
            Timeline::Sequence { frames, .. } => last_time_stride(frames, 3),
        };
        max = max.max(last);
    }
    max
}
