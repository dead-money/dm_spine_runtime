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

//! JSON `.json` skeleton parser — port of `spine-cpp/SkeletonJson.cpp`.
//!
//! The public entry point is [`SkeletonJson`]. Instantiate with a mutable
//! [`AttachmentLoader`] and call [`SkeletonJson::read`] on a JSON byte slice
//! or string to produce a [`SkeletonData`].
//!
//! The implementation mirrors the spine-cpp single-file approach so that a
//! reader comparing line-for-line sees roughly matching structure. Sections
//! are tagged with `// --- Section name` banners.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::match_same_arms,
    clippy::needless_pass_by_ref_mut,
    clippy::unused_self,
    clippy::assigning_clones,
    clippy::doc_markdown,
    clippy::missing_panics_doc,
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::unnecessary_wraps
)]

use serde_json::Value;
use thiserror::Error;

use crate::animation::{BEZIER_SIZE, compute_bezier_samples};
use crate::data::attachment::{Attachment, Sequence, VertexData};
use crate::data::{
    Animation, AnimationEvent, AttachmentId, BlendMode, BoneData, BoneId, CurveFrames, EventData,
    EventId, IkConstraintData, IkConstraintId, Inherit, PathConstraintData, PathConstraintId,
    PhysicsConstraintData, PhysicsConstraintId, PhysicsProperty, PositionMode, RotateMode,
    SkeletonData, Skin, SkinId, SlotData, SlotId, SpacingMode, Timeline, TransformConstraintData,
    TransformConstraintId,
};
use crate::load::AttachmentLoader;
use crate::load::AttachmentLoaderError;
use crate::math::Color;

/// Spine editor version this runtime is built for. Must prefix the skeleton's
/// embedded `spine` version field.
pub const TARGET_VERSION: &str = "4.2";

const CURVE_LINEAR: f32 = 0.0;
const CURVE_STEPPED: f32 = 1.0;
const CURVE_BEZIER: f32 = 2.0;

/// Errors produced while parsing a `.json` skeleton file.
#[derive(Debug, Error)]
pub enum JsonError {
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("skeleton version mismatch: file reports {found:?}, runtime targets {expected:?}")]
    UnsupportedVersion { found: String, expected: String },

    #[error("missing required field {path:?}")]
    MissingField { path: String },

    #[error("field {path:?} has unexpected type: {message}")]
    BadType { path: String, message: String },

    #[error("unknown {entity} value: {value:?}")]
    UnknownValue { entity: &'static str, value: String },

    #[error("named entity not found: {entity} {name:?}")]
    NotFound { entity: &'static str, name: String },

    #[error("invalid color string {value:?}: {message}")]
    InvalidColor { value: String, message: String },

    #[error("attachment loader error: {0}")]
    AttachmentLoader(#[from] AttachmentLoaderError),
}

/// Record of a mesh attachment whose vertex data is inherited from a parent
/// mesh in another skin. Resolved after all skins load.
struct LinkedMesh {
    mesh: AttachmentId,
    skin_name: Option<String>,
    slot_index: usize,
    parent_name: String,
    inherit_timeline: bool,
}

/// Stateful parser for the JSON format. Keeps scratch state for linked-mesh
/// resolution between top-level sections.
pub struct SkeletonJson<'loader> {
    loader: &'loader mut dyn AttachmentLoader,
    scale: f32,
    linked_meshes: Vec<LinkedMesh>,
}

impl<'loader> SkeletonJson<'loader> {
    /// Build a parser that resolves attachments through `loader`.
    pub fn with_loader(loader: &'loader mut dyn AttachmentLoader) -> Self {
        Self {
            loader,
            scale: 1.0,
            linked_meshes: Vec::new(),
        }
    }

    /// Override the load-time world-space scale (default `1.0`).
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Parse a JSON skeleton from a byte slice.
    ///
    /// # Errors
    /// Returns [`JsonError`] on malformed JSON, schema violations, version
    /// mismatch, or attachment-loader failures.
    pub fn read_slice(self, bytes: &[u8]) -> Result<SkeletonData, JsonError> {
        let root: Value = serde_json::from_slice(bytes)?;
        self.read_value(root)
    }

    /// Parse a JSON skeleton from a string.
    ///
    /// # Errors
    /// Returns [`JsonError`] on malformed JSON, schema violations, version
    /// mismatch, or attachment-loader failures.
    pub fn read_str(self, json: &str) -> Result<SkeletonData, JsonError> {
        let root: Value = serde_json::from_str(json)?;
        self.read_value(root)
    }

    /// Parse an already-deserialised JSON value.
    ///
    /// # Errors
    /// Returns [`JsonError`] on schema violations, version mismatch, or
    /// attachment-loader failures.
    pub fn read_value(mut self, root: Value) -> Result<SkeletonData, JsonError> {
        let mut sd = SkeletonData::default();
        self.linked_meshes.clear();

        // --- Header (skeleton object) --------------------------------------
        if let Some(sk) = root.get("skeleton") {
            sd.hash = get_str(sk, "hash").unwrap_or("").to_string();
            sd.version = get_str(sk, "spine").unwrap_or("").to_string();
            if !sd.version.is_empty() && !sd.version.starts_with(TARGET_VERSION) {
                return Err(JsonError::UnsupportedVersion {
                    found: sd.version.clone(),
                    expected: TARGET_VERSION.to_string(),
                });
            }
            sd.x = get_f32(sk, "x", 0.0);
            sd.y = get_f32(sk, "y", 0.0);
            sd.width = get_f32(sk, "width", 0.0);
            sd.height = get_f32(sk, "height", 0.0);
            sd.reference_scale = get_f32(sk, "referenceScale", 100.0) * self.scale;
            sd.fps = get_f32(sk, "fps", 30.0);
            sd.audio_path = get_str(sk, "audio").unwrap_or("").to_string();
            sd.images_path = get_str(sk, "images").unwrap_or("").to_string();
        }

        // --- Bones ---------------------------------------------------------
        if let Some(bones) = root.get("bones").and_then(Value::as_array) {
            sd.bones.reserve(bones.len());
            for (i, bone) in bones.iter().enumerate() {
                let name = get_str(bone, "name").unwrap_or("").to_string();
                let parent = if let Some(pname) = get_str(bone, "parent") {
                    let idx = sd
                        .bones
                        .iter()
                        .position(|b| b.name == pname)
                        .ok_or_else(|| JsonError::NotFound {
                            entity: "parent bone",
                            name: pname.to_string(),
                        })?;
                    Some(BoneId(idx as u16))
                } else {
                    None
                };
                let id = BoneId(i as u16);
                let mut b = BoneData::new(id, name, parent);
                b.length = get_f32(bone, "length", 0.0) * self.scale;
                b.x = get_f32(bone, "x", 0.0) * self.scale;
                b.y = get_f32(bone, "y", 0.0) * self.scale;
                b.rotation = get_f32(bone, "rotation", 0.0);
                b.scale_x = get_f32(bone, "scaleX", 1.0);
                b.scale_y = get_f32(bone, "scaleY", 1.0);
                b.shear_x = get_f32(bone, "shearX", 0.0);
                b.shear_y = get_f32(bone, "shearY", 0.0);
                b.inherit = parse_inherit(get_str(bone, "inherit").unwrap_or("normal"))?;
                b.skin_required = get_bool(bone, "skin", false);
                if let Some(color) = get_str(bone, "color") {
                    b.color = parse_color(color, true)?;
                }
                b.icon = get_str(bone, "icon").unwrap_or("").to_string();
                b.visible = get_bool(bone, "visible", true);
                sd.bones.push(b);
            }
        }

        // --- Slots ---------------------------------------------------------
        if let Some(slots) = root.get("slots").and_then(Value::as_array) {
            sd.slots.reserve(slots.len());
            for (i, slot) in slots.iter().enumerate() {
                let name = get_str(slot, "name").unwrap_or("").to_string();
                let bone_name = get_str(slot, "bone").ok_or_else(|| JsonError::MissingField {
                    path: format!("slots[{i}].bone"),
                })?;
                let bone_idx = sd
                    .bones
                    .iter()
                    .position(|b| b.name == bone_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "slot bone",
                        name: bone_name.to_string(),
                    })?;
                let mut s = SlotData::new(SlotId(i as u16), name, BoneId(bone_idx as u16));
                if let Some(c) = get_str(slot, "color") {
                    s.color = parse_color(c, true)?;
                }
                if let Some(d) = get_str(slot, "dark") {
                    let mut dc = parse_color(d, false)?;
                    dc.a = 1.0;
                    s.dark_color = Some(dc);
                }
                if let Some(att) = get_str(slot, "attachment") {
                    s.attachment_name = Some(att.to_string());
                }
                if let Some(blend) = get_str(slot, "blend") {
                    s.blend_mode = match blend {
                        "normal" => BlendMode::Normal,
                        "additive" => BlendMode::Additive,
                        "multiply" => BlendMode::Multiply,
                        "screen" => BlendMode::Screen,
                        other => {
                            return Err(JsonError::UnknownValue {
                                entity: "blend mode",
                                value: other.to_string(),
                            });
                        }
                    };
                }
                s.visible = get_bool(slot, "visible", true);
                sd.slots.push(s);
            }
        }

        // --- IK constraints ------------------------------------------------
        if let Some(ik) = root.get("ik").and_then(Value::as_array) {
            sd.ik_constraints.reserve(ik.len());
            for (i, c) in ik.iter().enumerate() {
                let name = get_str(c, "name").unwrap_or("").to_string();
                let target_name = get_str(c, "target").ok_or_else(|| JsonError::MissingField {
                    path: format!("ik[{i}].target"),
                })?;
                let target_idx = sd
                    .bones
                    .iter()
                    .position(|b| b.name == target_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "IK target bone",
                        name: target_name.to_string(),
                    })?;
                let mut data = IkConstraintData::new(
                    IkConstraintId(i as u16),
                    name,
                    BoneId(target_idx as u16),
                );
                data.order = get_int(c, "order", 0) as u32;
                data.skin_required = get_bool(c, "skin", false);
                if let Some(bones) = c.get("bones").and_then(Value::as_array) {
                    data.bones.reserve(bones.len());
                    for b in bones {
                        let bname = b.as_str().ok_or_else(|| JsonError::BadType {
                            path: "ik.bones".to_string(),
                            message: "expected string".to_string(),
                        })?;
                        let idx =
                            sd.bones
                                .iter()
                                .position(|x| x.name == bname)
                                .ok_or_else(|| JsonError::NotFound {
                                    entity: "IK bone",
                                    name: bname.to_string(),
                                })?;
                        data.bones.push(BoneId(idx as u16));
                    }
                }
                data.mix = get_f32(c, "mix", 1.0);
                data.softness = get_f32(c, "softness", 0.0) * self.scale;
                data.bend_direction = if get_bool(c, "bendPositive", true) {
                    1
                } else {
                    -1
                };
                data.compress = get_bool(c, "compress", false);
                data.stretch = get_bool(c, "stretch", false);
                data.uniform = get_bool(c, "uniform", false);
                sd.ik_constraints.push(data);
            }
        }

        // --- Transform constraints -----------------------------------------
        if let Some(tc) = root.get("transform").and_then(Value::as_array) {
            sd.transform_constraints.reserve(tc.len());
            for (i, c) in tc.iter().enumerate() {
                let name = get_str(c, "name").unwrap_or("").to_string();
                let target_name = get_str(c, "target").ok_or_else(|| JsonError::MissingField {
                    path: format!("transform[{i}].target"),
                })?;
                let target_idx = sd
                    .bones
                    .iter()
                    .position(|b| b.name == target_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "transform target bone",
                        name: target_name.to_string(),
                    })?;
                let mut data = TransformConstraintData::new(
                    TransformConstraintId(i as u16),
                    name,
                    BoneId(target_idx as u16),
                );
                data.order = get_int(c, "order", 0) as u32;
                data.skin_required = get_bool(c, "skin", false);
                if let Some(bones) = c.get("bones").and_then(Value::as_array) {
                    for b in bones {
                        let bname = b.as_str().ok_or_else(|| JsonError::BadType {
                            path: "transform.bones".to_string(),
                            message: "expected string".to_string(),
                        })?;
                        let idx =
                            sd.bones
                                .iter()
                                .position(|x| x.name == bname)
                                .ok_or_else(|| JsonError::NotFound {
                                    entity: "transform bone",
                                    name: bname.to_string(),
                                })?;
                        data.bones.push(BoneId(idx as u16));
                    }
                }
                data.local = get_bool(c, "local", false);
                data.relative = get_bool(c, "relative", false);
                data.offset_rotation = get_f32(c, "rotation", 0.0);
                data.offset_x = get_f32(c, "x", 0.0) * self.scale;
                data.offset_y = get_f32(c, "y", 0.0) * self.scale;
                data.offset_scale_x = get_f32(c, "scaleX", 0.0);
                data.offset_scale_y = get_f32(c, "scaleY", 0.0);
                data.offset_shear_y = get_f32(c, "shearY", 0.0);
                data.mix_rotate = get_f32(c, "mixRotate", 1.0);
                data.mix_x = get_f32(c, "mixX", 1.0);
                data.mix_y = get_f32(c, "mixY", data.mix_x);
                data.mix_scale_x = get_f32(c, "mixScaleX", 1.0);
                data.mix_scale_y = get_f32(c, "mixScaleY", data.mix_scale_x);
                data.mix_shear_y = get_f32(c, "mixShearY", 1.0);
                sd.transform_constraints.push(data);
            }
        }

        // --- Path constraints ----------------------------------------------
        if let Some(pc) = root.get("path").and_then(Value::as_array) {
            sd.path_constraints.reserve(pc.len());
            for (i, c) in pc.iter().enumerate() {
                let name = get_str(c, "name").unwrap_or("").to_string();
                let target_name = get_str(c, "target").ok_or_else(|| JsonError::MissingField {
                    path: format!("path[{i}].target"),
                })?;
                let target_idx = sd
                    .slots
                    .iter()
                    .position(|s| s.name == target_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "path target slot",
                        name: target_name.to_string(),
                    })?;
                let mut data = PathConstraintData::new(
                    PathConstraintId(i as u16),
                    name,
                    SlotId(target_idx as u16),
                );
                data.order = get_int(c, "order", 0) as u32;
                data.skin_required = get_bool(c, "skin", false);
                if let Some(bones) = c.get("bones").and_then(Value::as_array) {
                    for b in bones {
                        let bname = b.as_str().ok_or_else(|| JsonError::BadType {
                            path: "path.bones".to_string(),
                            message: "expected string".to_string(),
                        })?;
                        let idx =
                            sd.bones
                                .iter()
                                .position(|x| x.name == bname)
                                .ok_or_else(|| JsonError::NotFound {
                                    entity: "path bone",
                                    name: bname.to_string(),
                                })?;
                        data.bones.push(BoneId(idx as u16));
                    }
                }
                data.position_mode = match get_str(c, "positionMode").unwrap_or("percent") {
                    "fixed" => PositionMode::Fixed,
                    "percent" => PositionMode::Percent,
                    other => {
                        return Err(JsonError::UnknownValue {
                            entity: "position mode",
                            value: other.to_string(),
                        });
                    }
                };
                data.spacing_mode = match get_str(c, "spacingMode").unwrap_or("length") {
                    "length" => SpacingMode::Length,
                    "fixed" => SpacingMode::Fixed,
                    "percent" => SpacingMode::Percent,
                    "proportional" => SpacingMode::Proportional,
                    other => {
                        return Err(JsonError::UnknownValue {
                            entity: "spacing mode",
                            value: other.to_string(),
                        });
                    }
                };
                data.rotate_mode = match get_str(c, "rotateMode").unwrap_or("tangent") {
                    "tangent" => RotateMode::Tangent,
                    "chain" => RotateMode::Chain,
                    "chainScale" => RotateMode::ChainScale,
                    other => {
                        return Err(JsonError::UnknownValue {
                            entity: "rotate mode",
                            value: other.to_string(),
                        });
                    }
                };
                data.offset_rotation = get_f32(c, "rotation", 0.0);
                data.position = get_f32(c, "position", 0.0);
                if data.position_mode == PositionMode::Fixed {
                    data.position *= self.scale;
                }
                data.spacing = get_f32(c, "spacing", 0.0);
                if matches!(data.spacing_mode, SpacingMode::Length | SpacingMode::Fixed) {
                    data.spacing *= self.scale;
                }
                data.mix_rotate = get_f32(c, "mixRotate", 1.0);
                data.mix_x = get_f32(c, "mixX", 1.0);
                data.mix_y = get_f32(c, "mixY", data.mix_x);
                sd.path_constraints.push(data);
            }
        }

        // --- Physics constraints -------------------------------------------
        if let Some(ph) = root.get("physics").and_then(Value::as_array) {
            sd.physics_constraints.reserve(ph.len());
            for (i, c) in ph.iter().enumerate() {
                let name = get_str(c, "name").unwrap_or("").to_string();
                let bone_name = get_str(c, "bone").ok_or_else(|| JsonError::MissingField {
                    path: format!("physics[{i}].bone"),
                })?;
                let bone_idx = sd
                    .bones
                    .iter()
                    .position(|b| b.name == bone_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "physics bone",
                        name: bone_name.to_string(),
                    })?;
                let mut data = PhysicsConstraintData::new(
                    PhysicsConstraintId(i as u16),
                    name,
                    BoneId(bone_idx as u16),
                );
                data.order = get_int(c, "order", 0) as u32;
                data.skin_required = get_bool(c, "skin", false);
                data.x = get_f32(c, "x", 0.0);
                data.y = get_f32(c, "y", 0.0);
                data.rotate = get_f32(c, "rotate", 0.0);
                data.scale_x = get_f32(c, "scaleX", 0.0);
                data.shear_x = get_f32(c, "shearX", 0.0);
                data.limit = get_f32(c, "limit", 5000.0) * self.scale;
                data.step = 1.0 / get_f32(c, "fps", 60.0);
                data.inertia = get_f32(c, "inertia", 1.0);
                data.strength = get_f32(c, "strength", 100.0);
                data.damping = get_f32(c, "damping", 1.0);
                data.mass_inverse = 1.0 / get_f32(c, "mass", 1.0);
                data.wind = get_f32(c, "wind", 0.0);
                data.gravity = get_f32(c, "gravity", 0.0);
                data.mix = get_f32(c, "mix", 1.0);
                data.inertia_global = get_bool(c, "inertiaGlobal", false);
                data.strength_global = get_bool(c, "strengthGlobal", false);
                data.damping_global = get_bool(c, "dampingGlobal", false);
                data.mass_global = get_bool(c, "massGlobal", false);
                data.wind_global = get_bool(c, "windGlobal", false);
                data.gravity_global = get_bool(c, "gravityGlobal", false);
                data.mix_global = get_bool(c, "mixGlobal", false);
                sd.physics_constraints.push(data);
            }
        }

        // --- Skins ---------------------------------------------------------
        if let Some(skins) = root.get("skins").and_then(Value::as_array) {
            sd.skins.reserve(skins.len());
            for skin_map in skins {
                self.read_skin(skin_map, &mut sd)?;
            }
        }

        // --- Linked mesh resolution ---------------------------------------
        self.resolve_linked_meshes(&mut sd)?;

        // --- Events --------------------------------------------------------
        if let Some(events) = root.get("events").and_then(Value::as_object) {
            sd.events.reserve(events.len());
            for (i, (name, e)) in events.iter().enumerate() {
                let mut data = EventData::new(EventId(i as u16), name.clone());
                data.int_value = get_int(e, "int", 0);
                data.float_value = get_f32(e, "float", 0.0);
                data.string_value = get_str(e, "string").unwrap_or("").to_string();
                data.audio_path = get_str(e, "audio").unwrap_or("").to_string();
                if !data.audio_path.is_empty() {
                    data.volume = get_f32(e, "volume", 1.0);
                    data.balance = get_f32(e, "balance", 0.0);
                }
                sd.events.push(data);
            }
        }

        // --- Animations ----------------------------------------------------
        if let Some(anims) = root.get("animations").and_then(Value::as_object) {
            sd.animations.reserve(anims.len());
            for (name, a) in anims {
                let anim = self.read_animation(name, a, &sd)?;
                sd.animations.push(anim);
            }
        }

        Ok(sd)
    }

    // -----------------------------------------------------------------------
    // Skins + attachments
    // -----------------------------------------------------------------------

    fn read_skin(&mut self, skin_map: &Value, sd: &mut SkeletonData) -> Result<(), JsonError> {
        let skin_name = get_str(skin_map, "name").unwrap_or("").to_string();
        let mut skin = Skin::new(skin_name.clone());

        if let Some(bones) = skin_map.get("bones").and_then(Value::as_array) {
            for b in bones {
                let bname = b.as_str().ok_or_else(|| JsonError::BadType {
                    path: "skin.bones".to_string(),
                    message: "expected string".to_string(),
                })?;
                let idx = sd
                    .bones
                    .iter()
                    .position(|x| x.name == bname)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "skin bone",
                        name: bname.to_string(),
                    })?;
                skin.bones.push(BoneId(idx as u16));
            }
        }
        if let Some(ik) = skin_map.get("ik").and_then(Value::as_array) {
            for b in ik {
                let n = b.as_str().ok_or_else(|| JsonError::BadType {
                    path: "skin.ik".to_string(),
                    message: "expected string".to_string(),
                })?;
                let idx = sd
                    .ik_constraints
                    .iter()
                    .position(|x| x.name == n)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "skin IK constraint",
                        name: n.to_string(),
                    })?;
                skin.ik_constraints.push(IkConstraintId(idx as u16));
            }
        }
        if let Some(tc) = skin_map.get("transform").and_then(Value::as_array) {
            for b in tc {
                let n = b.as_str().ok_or_else(|| JsonError::BadType {
                    path: "skin.transform".to_string(),
                    message: "expected string".to_string(),
                })?;
                let idx = sd
                    .transform_constraints
                    .iter()
                    .position(|x| x.name == n)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "skin transform constraint",
                        name: n.to_string(),
                    })?;
                skin.transform_constraints
                    .push(TransformConstraintId(idx as u16));
            }
        }
        if let Some(pc) = skin_map.get("path").and_then(Value::as_array) {
            for b in pc {
                let n = b.as_str().ok_or_else(|| JsonError::BadType {
                    path: "skin.path".to_string(),
                    message: "expected string".to_string(),
                })?;
                let idx = sd
                    .path_constraints
                    .iter()
                    .position(|x| x.name == n)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "skin path constraint",
                        name: n.to_string(),
                    })?;
                skin.path_constraints.push(PathConstraintId(idx as u16));
            }
        }
        if let Some(phc) = skin_map.get("physics").and_then(Value::as_array) {
            for b in phc {
                let n = b.as_str().ok_or_else(|| JsonError::BadType {
                    path: "skin.physics".to_string(),
                    message: "expected string".to_string(),
                })?;
                let idx = sd
                    .physics_constraints
                    .iter()
                    .position(|x| x.name == n)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "skin physics constraint",
                        name: n.to_string(),
                    })?;
                skin.physics_constraints
                    .push(PhysicsConstraintId(idx as u16));
            }
        }

        let is_default = skin_name == "default";

        if let Some(atts) = skin_map.get("attachments").and_then(Value::as_object) {
            for (slot_name, slot_obj) in atts {
                let slot_idx = sd
                    .slots
                    .iter()
                    .position(|s| s.name == *slot_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "attachment slot",
                        name: slot_name.clone(),
                    })?;
                let slot_obj = slot_obj.as_object().ok_or_else(|| JsonError::BadType {
                    path: format!("skins.{skin_name}.attachments.{slot_name}"),
                    message: "expected object".to_string(),
                })?;
                for (skin_att_name, att_map) in slot_obj {
                    let attachment =
                        self.read_attachment(att_map, &skin_name, slot_idx, skin_att_name, sd)?;
                    if let Some(mut a) = attachment {
                        let att_id = AttachmentId(sd.attachments.len() as u32);
                        // For linked meshes we stashed the anticipated id when
                        // we pushed the LinkedMesh record — patch it now.
                        if let Some(lm) = self
                            .linked_meshes
                            .iter_mut()
                            .rev()
                            .find(|lm| lm.mesh == AttachmentId(u32::MAX))
                        {
                            lm.mesh = att_id;
                        }
                        // read_attachment ran update_region before the attachment
                        // id was known; nothing more to do here for non-linked.
                        let _ = &mut a;
                        sd.attachments.push(a);
                        skin.set_attachment(SlotId(slot_idx as u16), skin_att_name, att_id);
                    }
                }
            }
        }

        if is_default {
            let id = SkinId(sd.skins.len() as u16);
            sd.default_skin = Some(id);
        }
        sd.skins.push(skin);
        Ok(())
    }

    fn read_attachment(
        &mut self,
        att_map: &Value,
        skin_name: &str,
        slot_idx: usize,
        skin_att_name: &str,
        sd: &SkeletonData,
    ) -> Result<Option<Attachment>, JsonError> {
        let attachment_name = get_str(att_map, "name")
            .unwrap_or(skin_att_name)
            .to_string();
        let attachment_path = get_str(att_map, "path")
            .map(str::to_string)
            .unwrap_or_else(|| attachment_name.clone());
        let type_str = get_str(att_map, "type").unwrap_or("region");
        let slot_name = sd.slots[slot_idx].name.clone();

        match type_str {
            "region" => {
                let mut sequence = read_sequence(att_map.get("sequence"));
                let mut attachment = self.loader.new_region_attachment(
                    skin_name,
                    &slot_name,
                    &attachment_name,
                    &attachment_path,
                    sequence.as_mut(),
                )?;
                if let Attachment::Region(r) = &mut attachment {
                    r.path = attachment_path;
                    r.x = get_f32(att_map, "x", 0.0) * self.scale;
                    r.y = get_f32(att_map, "y", 0.0) * self.scale;
                    r.scale_x = get_f32(att_map, "scaleX", 1.0);
                    r.scale_y = get_f32(att_map, "scaleY", 1.0);
                    r.rotation = get_f32(att_map, "rotation", 0.0);
                    r.width = get_f32(att_map, "width", 32.0) * self.scale;
                    r.height = get_f32(att_map, "height", 32.0) * self.scale;
                    r.sequence = sequence;
                    if let Some(color) = get_str(att_map, "color") {
                        r.color = parse_color(color, true)?;
                    }
                    r.update_region();
                }
                Ok(Some(attachment))
            }

            "mesh" | "linkedmesh" => {
                let mut sequence = read_sequence(att_map.get("sequence"));
                let mut attachment = self.loader.new_mesh_attachment(
                    skin_name,
                    &slot_name,
                    &attachment_name,
                    &attachment_path,
                    sequence.as_mut(),
                )?;
                if let Attachment::Mesh(m) = &mut attachment {
                    m.path = attachment_path;
                    if let Some(color) = get_str(att_map, "color") {
                        m.color = parse_color(color, true)?;
                    }
                    m.width = get_f32(att_map, "width", 32.0) * self.scale;
                    m.height = get_f32(att_map, "height", 32.0) * self.scale;
                    m.sequence = sequence;

                    if let Some(parent) = get_str(att_map, "parent") {
                        // Linked mesh — defer vertex population to the
                        // post-pass resolver. Record the link now; the
                        // anticipated AttachmentId is patched by read_skin
                        // immediately after this returns.
                        let inherit_timelines = get_bool(att_map, "timelines", true);
                        let link_skin = get_str(att_map, "skin").map(str::to_string);
                        self.linked_meshes.push(LinkedMesh {
                            mesh: AttachmentId(u32::MAX),
                            skin_name: link_skin,
                            slot_index: slot_idx,
                            parent_name: parent.to_string(),
                            inherit_timeline: inherit_timelines,
                        });
                        return Ok(Some(attachment));
                    }

                    // Non-linked mesh.
                    let triangles = att_map
                        .get("triangles")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().map(|v| v.as_i64().unwrap_or(0) as u16).collect())
                        .unwrap_or_default();
                    let uvs: Vec<f32> = att_map
                        .get("uvs")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect())
                        .unwrap_or_default();
                    let vertices_length = uvs.len();
                    let (bones, vertices) = self.read_vertices(att_map, vertices_length)?;
                    m.vertex_data.bones = bones;
                    m.vertex_data.vertices = vertices;
                    m.vertex_data.world_vertices_length = vertices_length as u32;
                    m.region_uvs = uvs;
                    m.triangles = triangles;
                    m.hull_length = get_int(att_map, "hull", 0) as u32;
                    if let Some(edges) = att_map.get("edges").and_then(Value::as_array) {
                        m.edges = edges
                            .iter()
                            .map(|v| v.as_i64().unwrap_or(0) as u16)
                            .collect();
                    }
                    m.update_region();
                }
                Ok(Some(attachment))
            }

            "boundingbox" => {
                let mut attachment = self.loader.new_bounding_box_attachment(
                    skin_name,
                    &slot_name,
                    &attachment_name,
                )?;
                let vertex_count = get_int(att_map, "vertexCount", 0) as usize * 2;
                let (bones, vertices) = self.read_vertices(att_map, vertex_count)?;
                if let Attachment::BoundingBox(bb) = &mut attachment {
                    bb.vertex_data.bones = bones;
                    bb.vertex_data.vertices = vertices;
                    bb.vertex_data.world_vertices_length = vertex_count as u32;
                    if let Some(color) = get_str(att_map, "color") {
                        bb.color = parse_color(color, true)?;
                    }
                }
                Ok(Some(attachment))
            }

            "path" => {
                let mut attachment =
                    self.loader
                        .new_path_attachment(skin_name, &slot_name, &attachment_name)?;
                if let Attachment::Path(pa) = &mut attachment {
                    pa.closed = get_bool(att_map, "closed", false);
                    pa.constant_speed = get_bool(att_map, "constantSpeed", true);
                    let vertex_count = get_int(att_map, "vertexCount", 0) as usize;
                    let (bones, vertices) = self.read_vertices(att_map, vertex_count * 2)?;
                    pa.vertex_data.bones = bones;
                    pa.vertex_data.vertices = vertices;
                    pa.vertex_data.world_vertices_length = (vertex_count * 2) as u32;
                    if let Some(lengths) = att_map.get("lengths").and_then(Value::as_array) {
                        pa.lengths = lengths
                            .iter()
                            .map(|v| v.as_f64().unwrap_or(0.0) as f32 * self.scale)
                            .collect();
                    }
                    if let Some(color) = get_str(att_map, "color") {
                        pa.color = parse_color(color, true)?;
                    }
                }
                Ok(Some(attachment))
            }

            "point" => {
                let mut attachment =
                    self.loader
                        .new_point_attachment(skin_name, &slot_name, &attachment_name)?;
                if let Attachment::Point(pt) = &mut attachment {
                    pt.x = get_f32(att_map, "x", 0.0) * self.scale;
                    pt.y = get_f32(att_map, "y", 0.0) * self.scale;
                    pt.rotation = get_f32(att_map, "rotation", 0.0);
                    if let Some(color) = get_str(att_map, "color") {
                        pt.color = parse_color(color, true)?;
                    }
                }
                Ok(Some(attachment))
            }

            "clipping" => {
                let end_name = get_str(att_map, "end").unwrap_or("");
                let end_slot = sd
                    .slots
                    .iter()
                    .position(|s| s.name == end_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "clipping end slot",
                        name: end_name.to_string(),
                    })?;
                let mut attachment = self.loader.new_clipping_attachment(
                    skin_name,
                    &slot_name,
                    &attachment_name,
                    SlotId(end_slot as u16),
                )?;
                if let Attachment::Clipping(cl) = &mut attachment {
                    let vertex_count = get_int(att_map, "vertexCount", 0) as usize * 2;
                    let (bones, vertices) = self.read_vertices(att_map, vertex_count)?;
                    cl.vertex_data.bones = bones;
                    cl.vertex_data.vertices = vertices;
                    cl.vertex_data.world_vertices_length = vertex_count as u32;
                    if let Some(color) = get_str(att_map, "color") {
                        cl.color = parse_color(color, true)?;
                    }
                }
                Ok(Some(attachment))
            }

            other => Err(JsonError::UnknownValue {
                entity: "attachment type",
                value: other.to_string(),
            }),
        }
    }

    /// Port of spine-cpp `readVertices`. `vertices_length` is the *unweighted*
    /// float count (2 * vertex_count).
    fn read_vertices(
        &self,
        att_map: &Value,
        vertices_length: usize,
    ) -> Result<(Vec<i32>, Vec<f32>), JsonError> {
        let raw = att_map
            .get("vertices")
            .and_then(Value::as_array)
            .ok_or_else(|| JsonError::MissingField {
                path: "attachment.vertices".to_string(),
            })?;
        let entry_size = raw.len();
        let flat: Vec<f32> = raw
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        if vertices_length == entry_size {
            // Unweighted.
            if (self.scale - 1.0).abs() < f32::EPSILON {
                return Ok((Vec::new(), flat));
            }
            let scaled = flat.into_iter().map(|f| f * self.scale).collect();
            return Ok((Vec::new(), scaled));
        }

        // Weighted — stride is `(1 + 4 * boneCount)` per vertex.
        let mut bones: Vec<i32> = Vec::new();
        let mut verts: Vec<f32> = Vec::new();
        let mut i = 0usize;
        while i < entry_size {
            let bone_count = flat[i] as i32;
            bones.push(bone_count);
            i += 1;
            let limit = i + (bone_count as usize) * 4;
            while i < limit && i + 3 < entry_size {
                bones.push(flat[i] as i32);
                verts.push(flat[i + 1] * self.scale);
                verts.push(flat[i + 2] * self.scale);
                verts.push(flat[i + 3]);
                i += 4;
            }
        }
        Ok((bones, verts))
    }

    fn resolve_linked_meshes(&mut self, sd: &mut SkeletonData) -> Result<(), JsonError> {
        for lm in std::mem::take(&mut self.linked_meshes) {
            if lm.mesh == AttachmentId(u32::MAX) {
                continue;
            }
            let skin =
                match lm.skin_name.as_deref() {
                    None | Some("") => sd
                        .default_skin
                        .and_then(|id| sd.skins.get(id.index()))
                        .ok_or_else(|| JsonError::NotFound {
                            entity: "linked mesh skin",
                            name: "<default>".to_string(),
                        })?,
                    Some(name) => sd.skins.iter().find(|s| s.name == name).ok_or_else(|| {
                        JsonError::NotFound {
                            entity: "linked mesh skin",
                            name: name.to_string(),
                        }
                    })?,
                };
            let parent_id = skin
                .get_attachment(SlotId(lm.slot_index as u16), &lm.parent_name)
                .ok_or_else(|| JsonError::NotFound {
                    entity: "linked mesh parent",
                    name: lm.parent_name.clone(),
                })?;
            let parent = sd.attachments[parent_id.index()].clone();
            let Attachment::Mesh(parent_mesh) = parent else {
                continue;
            };
            if let Attachment::Mesh(child) = &mut sd.attachments[lm.mesh.index()] {
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
                child.update_region();
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Animations
    // -----------------------------------------------------------------------

    fn read_animation(
        &self,
        name: &str,
        root: &Value,
        sd: &SkeletonData,
    ) -> Result<Animation, JsonError> {
        let mut anim = Animation::new(name.to_string(), 0.0);

        // --- Slot timelines ------------------------------------------------
        if let Some(slots) = root.get("slots").and_then(Value::as_object) {
            for (slot_name, slot_timelines) in slots {
                let slot_idx = sd
                    .slots
                    .iter()
                    .position(|s| s.name == *slot_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "slot",
                        name: slot_name.clone(),
                    })?;
                let slot = SlotId(slot_idx as u16);
                for (tname, tm) in slot_timelines
                    .as_object()
                    .ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.slots.{slot_name}"),
                        message: "expected object".to_string(),
                    })?
                {
                    let keys = tm.as_array().ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.slots.{slot_name}.{tname}"),
                        message: "expected array".to_string(),
                    })?;
                    match tname.as_str() {
                        "attachment" => {
                            let mut frames = Vec::with_capacity(keys.len());
                            let mut names = Vec::with_capacity(keys.len());
                            for k in keys {
                                frames.push(get_f32(k, "time", 0.0));
                                names.push(get_str(k, "name").map(str::to_string));
                            }
                            anim.timelines.push(Timeline::Attachment {
                                slot,
                                frames,
                                names,
                            });
                        }
                        "rgba" => {
                            let curves = read_color_timeline_json(keys, 4, "color", true)?;
                            anim.timelines.push(Timeline::Rgba { slot, curves });
                        }
                        "rgb" => {
                            let curves = read_color_timeline_json(keys, 3, "color", false)?;
                            anim.timelines.push(Timeline::Rgb { slot, curves });
                        }
                        "alpha" => {
                            let curves = read_alpha_timeline_json(keys)?;
                            anim.timelines.push(Timeline::Alpha { slot, curves });
                        }
                        "rgba2" => {
                            let curves = read_rgb2_timeline_json(keys, true)?;
                            anim.timelines.push(Timeline::Rgba2 { slot, curves });
                        }
                        "rgb2" => {
                            let curves = read_rgb2_timeline_json(keys, false)?;
                            anim.timelines.push(Timeline::Rgb2 { slot, curves });
                        }
                        other => {
                            return Err(JsonError::UnknownValue {
                                entity: "slot timeline",
                                value: other.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // --- Bone timelines -----------------------------------------------
        if let Some(bones) = root.get("bones").and_then(Value::as_object) {
            for (bone_name, bone_timelines) in bones {
                let bone_idx = sd
                    .bones
                    .iter()
                    .position(|b| b.name == *bone_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "bone",
                        name: bone_name.clone(),
                    })?;
                let bone = BoneId(bone_idx as u16);
                for (tname, tm) in bone_timelines
                    .as_object()
                    .ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.bones.{bone_name}"),
                        message: "expected object".to_string(),
                    })?
                {
                    let keys = tm.as_array().ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.bones.{bone_name}.{tname}"),
                        message: "expected array".to_string(),
                    })?;
                    if keys.is_empty() {
                        continue;
                    }
                    match tname.as_str() {
                        "rotate" => {
                            let curves = read_timeline1(keys, "value", 0.0, 1.0)?;
                            anim.timelines.push(Timeline::Rotate { bone, curves });
                        }
                        "translate" => {
                            let curves = read_timeline2(keys, "x", "y", 0.0, self.scale)?;
                            anim.timelines.push(Timeline::Translate { bone, curves });
                        }
                        "translatex" => {
                            let curves = read_timeline1(keys, "value", 0.0, self.scale)?;
                            anim.timelines.push(Timeline::TranslateX { bone, curves });
                        }
                        "translatey" => {
                            let curves = read_timeline1(keys, "value", 0.0, self.scale)?;
                            anim.timelines.push(Timeline::TranslateY { bone, curves });
                        }
                        "scale" => {
                            let curves = read_timeline2(keys, "x", "y", 1.0, 1.0)?;
                            anim.timelines.push(Timeline::Scale { bone, curves });
                        }
                        "scalex" => {
                            let curves = read_timeline1(keys, "value", 1.0, 1.0)?;
                            anim.timelines.push(Timeline::ScaleX { bone, curves });
                        }
                        "scaley" => {
                            let curves = read_timeline1(keys, "value", 1.0, 1.0)?;
                            anim.timelines.push(Timeline::ScaleY { bone, curves });
                        }
                        "shear" => {
                            let curves = read_timeline2(keys, "x", "y", 0.0, 1.0)?;
                            anim.timelines.push(Timeline::Shear { bone, curves });
                        }
                        "shearx" => {
                            let curves = read_timeline1(keys, "value", 0.0, 1.0)?;
                            anim.timelines.push(Timeline::ShearX { bone, curves });
                        }
                        "sheary" => {
                            let curves = read_timeline1(keys, "value", 0.0, 1.0)?;
                            anim.timelines.push(Timeline::ShearY { bone, curves });
                        }
                        "inherit" => {
                            let mut frames = Vec::with_capacity(keys.len());
                            let mut inherits = Vec::with_capacity(keys.len());
                            for k in keys {
                                frames.push(get_f32(k, "time", 0.0));
                                inherits.push(parse_inherit(
                                    get_str(k, "inherit").unwrap_or("normal"),
                                )?);
                            }
                            anim.timelines.push(Timeline::Inherit {
                                bone,
                                frames,
                                inherits,
                            });
                        }
                        other => {
                            return Err(JsonError::UnknownValue {
                                entity: "bone timeline",
                                value: other.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // --- IK constraint timelines --------------------------------------
        if let Some(ik) = root.get("ik").and_then(Value::as_object) {
            for (cname, keys_val) in ik {
                let idx = sd.ik_constraints.iter().position(|c| c.name == *cname);
                let Some(idx) = idx else {
                    continue;
                };
                let keys = keys_val.as_array().ok_or_else(|| JsonError::BadType {
                    path: format!("animations.{name}.ik.{cname}"),
                    message: "expected array".to_string(),
                })?;
                if keys.is_empty() {
                    continue;
                }
                let curves = read_ik_timeline_json(keys, self.scale)?;
                anim.timelines.push(Timeline::IkConstraint {
                    constraint: IkConstraintId(idx as u16),
                    curves,
                });
            }
        }

        // --- Transform constraint timelines -------------------------------
        if let Some(tc) = root.get("transform").and_then(Value::as_object) {
            for (cname, keys_val) in tc {
                let idx = sd
                    .transform_constraints
                    .iter()
                    .position(|c| c.name == *cname);
                let Some(idx) = idx else {
                    continue;
                };
                let keys = keys_val.as_array().ok_or_else(|| JsonError::BadType {
                    path: format!("animations.{name}.transform.{cname}"),
                    message: "expected array".to_string(),
                })?;
                if keys.is_empty() {
                    continue;
                }
                let curves = read_transform_timeline_json(keys)?;
                anim.timelines.push(Timeline::TransformConstraint {
                    constraint: TransformConstraintId(idx as u16),
                    curves,
                });
            }
        }

        // --- Path constraint timelines ------------------------------------
        if let Some(paths) = root.get("path").and_then(Value::as_object) {
            for (cname, sub) in paths {
                let idx = sd
                    .path_constraints
                    .iter()
                    .position(|c| c.name == *cname)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "path constraint",
                        name: cname.clone(),
                    })?;
                let data = &sd.path_constraints[idx];
                let tmaps = sub.as_object().ok_or_else(|| JsonError::BadType {
                    path: format!("animations.{name}.path.{cname}"),
                    message: "expected object".to_string(),
                })?;
                for (tname, keys_val) in tmaps {
                    let keys = keys_val.as_array().ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.path.{cname}.{tname}"),
                        message: "expected array".to_string(),
                    })?;
                    if keys.is_empty() {
                        continue;
                    }
                    match tname.as_str() {
                        "position" => {
                            let scale = if data.position_mode == PositionMode::Fixed {
                                self.scale
                            } else {
                                1.0
                            };
                            let curves = read_timeline1(keys, "value", 0.0, scale)?;
                            anim.timelines.push(Timeline::PathConstraintPosition {
                                constraint: PathConstraintId(idx as u16),
                                curves,
                            });
                        }
                        "spacing" => {
                            let scale = if matches!(
                                data.spacing_mode,
                                SpacingMode::Length | SpacingMode::Fixed
                            ) {
                                self.scale
                            } else {
                                1.0
                            };
                            let curves = read_timeline1(keys, "value", 0.0, scale)?;
                            anim.timelines.push(Timeline::PathConstraintSpacing {
                                constraint: PathConstraintId(idx as u16),
                                curves,
                            });
                        }
                        "mix" => {
                            let curves = read_path_mix_timeline_json(keys)?;
                            anim.timelines.push(Timeline::PathConstraintMix {
                                constraint: PathConstraintId(idx as u16),
                                curves,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }

        // --- Physics constraint timelines ---------------------------------
        if let Some(phys) = root.get("physics").and_then(Value::as_object) {
            for (cname, sub) in phys {
                let constraint = if cname.is_empty() {
                    None
                } else {
                    sd.physics_constraints
                        .iter()
                        .position(|c| c.name == *cname)
                        .map(|i| PhysicsConstraintId(i as u16))
                };
                let tmaps = sub.as_object().ok_or_else(|| JsonError::BadType {
                    path: format!("animations.{name}.physics.{cname}"),
                    message: "expected object".to_string(),
                })?;
                for (tname, keys_val) in tmaps {
                    let keys = keys_val.as_array().ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.physics.{cname}.{tname}"),
                        message: "expected array".to_string(),
                    })?;
                    if keys.is_empty() {
                        continue;
                    }
                    if tname == "reset" {
                        let frames = keys
                            .iter()
                            .map(|k| get_f32(k, "time", 0.0))
                            .collect::<Vec<_>>();
                        anim.timelines
                            .push(Timeline::PhysicsReset { constraint, frames });
                        continue;
                    }
                    let property = match tname.as_str() {
                        "inertia" => PhysicsProperty::Inertia,
                        "strength" => PhysicsProperty::Strength,
                        "damping" => PhysicsProperty::Damping,
                        "mass" => PhysicsProperty::Mass,
                        "wind" => PhysicsProperty::Wind,
                        "gravity" => PhysicsProperty::Gravity,
                        "mix" => PhysicsProperty::Mix,
                        _ => continue,
                    };
                    let curves = read_timeline1(keys, "value", 0.0, 1.0)?;
                    anim.timelines.push(Timeline::Physics {
                        constraint,
                        property,
                        curves,
                    });
                }
            }
        }

        // --- Attachment timelines (deform + sequence) ---------------------
        if let Some(skins) = root.get("attachments").and_then(Value::as_object) {
            for (skin_name, slots_obj) in skins {
                let skin = sd
                    .skins
                    .iter()
                    .find(|s| s.name == *skin_name)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "animation skin",
                        name: skin_name.clone(),
                    })?;
                let slots = slots_obj.as_object().ok_or_else(|| JsonError::BadType {
                    path: format!("animations.{name}.attachments.{skin_name}"),
                    message: "expected object".to_string(),
                })?;
                for (slot_name, atts) in slots {
                    let slot_idx = sd
                        .slots
                        .iter()
                        .position(|s| s.name == *slot_name)
                        .ok_or_else(|| JsonError::NotFound {
                            entity: "animation slot",
                            name: slot_name.clone(),
                        })?;
                    let slot = SlotId(slot_idx as u16);
                    let att_map = atts.as_object().ok_or_else(|| JsonError::BadType {
                        path: format!("animations.{name}.attachments.{skin_name}.{slot_name}"),
                        message: "expected object".to_string(),
                    })?;
                    for (att_name, tmap) in att_map {
                        let attachment_id =
                            skin.get_attachment(slot, att_name).ok_or_else(|| {
                                JsonError::NotFound {
                                    entity: "animation attachment",
                                    name: att_name.clone(),
                                }
                            })?;
                        let tobj = tmap.as_object().ok_or_else(|| JsonError::BadType {
                            path: format!(
                                "animations.{name}.attachments.{skin_name}.{slot_name}.{att_name}"
                            ),
                            message: "expected object".to_string(),
                        })?;
                        for (tname, keys_val) in tobj {
                            let keys = keys_val.as_array().ok_or_else(|| JsonError::BadType {
                                path: format!(
                                    "animations.{name}.attachments.{skin_name}.{slot_name}.{att_name}.{tname}"
                                ),
                                message: "expected array".to_string(),
                            })?;
                            if keys.is_empty() {
                                continue;
                            }
                            match tname.as_str() {
                                "deform" => {
                                    let (weighted, setup) = deform_context(sd, attachment_id);
                                    let deform_length = deform_frame_len(sd, attachment_id);
                                    let (frames, curves, vertices) = read_deform_timeline_json(
                                        keys,
                                        deform_length,
                                        self.scale,
                                        weighted,
                                        &setup,
                                    )?;
                                    anim.timelines.push(Timeline::Deform {
                                        slot,
                                        attachment: attachment_id,
                                        curves: CurveFrames { frames, curves },
                                        vertices,
                                    });
                                }
                                "sequence" => {
                                    let mut frames = Vec::with_capacity(keys.len() * 3);
                                    let mut last_delay = 0.0_f32;
                                    for k in keys {
                                        let time = get_f32(k, "time", 0.0);
                                        let delay = get_f32(k, "delay", last_delay);
                                        last_delay = delay;
                                        let index = get_int(k, "index", 0);
                                        let mode_str = get_str(k, "mode").unwrap_or("hold");
                                        let mode = match mode_str {
                                            "hold" => 0u32,
                                            "once" => 1,
                                            "loop" => 2,
                                            "pingpong" => 3,
                                            "onceReverse" => 4,
                                            "loopReverse" => 5,
                                            "pingpongReverse" => 6,
                                            other => {
                                                return Err(JsonError::UnknownValue {
                                                    entity: "sequence mode",
                                                    value: other.to_string(),
                                                });
                                            }
                                        };
                                        // Pack mode + index into a single f32
                                        // using the same bit layout as the
                                        // binary reader: `(index << 4) | mode`.
                                        let packed = ((index << 4) as u32 | mode) as i32;
                                        frames.push(time);
                                        frames.push(packed as f32);
                                        frames.push(delay);
                                    }
                                    anim.timelines.push(Timeline::Sequence {
                                        slot,
                                        attachment: attachment_id,
                                        frames,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // --- Draw order timeline ------------------------------------------
        if let Some(draw_order) = root.get("drawOrder").and_then(Value::as_array) {
            let slot_count = sd.slots.len();
            let mut frames = Vec::with_capacity(draw_order.len());
            let mut draw_orders: Vec<Option<Vec<SlotId>>> = Vec::with_capacity(draw_order.len());
            for k in draw_order {
                frames.push(get_f32(k, "time", 0.0));
                let offsets = k.get("offsets").and_then(Value::as_array);
                let Some(offsets) = offsets else {
                    draw_orders.push(None);
                    continue;
                };
                if slot_count < offsets.len() {
                    draw_orders.push(None);
                    continue;
                }
                let mut draw_order2: Vec<i32> = vec![-1; slot_count];
                let mut unchanged: Vec<i32> = vec![0; slot_count - offsets.len()];
                let mut unchanged_idx = 0usize;
                let mut original_idx: i32 = 0;
                for off in offsets {
                    let slot_name = get_str(off, "slot").unwrap_or("");
                    let slot_idx = sd
                        .slots
                        .iter()
                        .position(|s| s.name == slot_name)
                        .ok_or_else(|| JsonError::NotFound {
                            entity: "draw-order slot",
                            name: slot_name.to_string(),
                        })? as i32;
                    while original_idx != slot_idx {
                        unchanged[unchanged_idx] = original_idx;
                        unchanged_idx += 1;
                        original_idx += 1;
                    }
                    let offset = get_int(off, "offset", 0);
                    let target = (original_idx + offset) as usize;
                    draw_order2[target] = original_idx;
                    original_idx += 1;
                }
                while (original_idx as usize) < slot_count {
                    unchanged[unchanged_idx] = original_idx;
                    unchanged_idx += 1;
                    original_idx += 1;
                }
                for ii in (0..slot_count).rev() {
                    if draw_order2[ii] == -1 {
                        unchanged_idx -= 1;
                        draw_order2[ii] = unchanged[unchanged_idx];
                    }
                }
                draw_orders.push(Some(
                    draw_order2.into_iter().map(|x| SlotId(x as u16)).collect(),
                ));
            }
            anim.timelines.push(Timeline::DrawOrder {
                frames,
                draw_orders,
            });
        }

        // --- Event timeline -----------------------------------------------
        if let Some(events) = root.get("events").and_then(Value::as_array) {
            let mut frames = Vec::with_capacity(events.len());
            let mut out = Vec::with_capacity(events.len());
            for k in events {
                let ename = get_str(k, "name").unwrap_or("");
                let ei = sd
                    .events
                    .iter()
                    .position(|e| e.name == ename)
                    .ok_or_else(|| JsonError::NotFound {
                        entity: "event",
                        name: ename.to_string(),
                    })?;
                let data = &sd.events[ei];
                let time = get_f32(k, "time", 0.0);
                let int_value = get_int(k, "int", data.int_value);
                let float_value = get_f32(k, "float", data.float_value);
                let string_value = get_str(k, "string")
                    .map(str::to_string)
                    .or_else(|| Some(data.string_value.clone()));
                let (volume, balance) = if data.audio_path.is_empty() {
                    (data.volume, data.balance)
                } else {
                    (get_f32(k, "volume", 1.0), get_f32(k, "balance", 0.0))
                };
                frames.push(time);
                out.push(AnimationEvent {
                    time,
                    event: EventId(ei as u16),
                    int_value,
                    float_value,
                    string_value,
                    volume,
                    balance,
                });
            }
            anim.timelines.push(Timeline::Event {
                frames,
                events: out,
            });
        }

        anim.duration = timeline_duration(&anim);
        Ok(anim)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn get_f32(v: &Value, key: &str, default: f32) -> f32 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f as f32)
        .unwrap_or(default)
}

fn get_int(v: &Value, key: &str, default: i32) -> i32 {
    v.get(key)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .map(|i| i as i32)
        .unwrap_or(default)
}

fn get_bool(v: &Value, key: &str, default: bool) -> bool {
    v.get(key)
        .and_then(|x| x.as_bool().or_else(|| x.as_i64().map(|n| n != 0)))
        .unwrap_or(default)
}

fn parse_inherit(s: &str) -> Result<Inherit, JsonError> {
    Ok(match s {
        "normal" => Inherit::Normal,
        "onlyTranslation" => Inherit::OnlyTranslation,
        "noRotationOrReflection" => Inherit::NoRotationOrReflection,
        "noScale" => Inherit::NoScale,
        "noScaleOrReflection" => Inherit::NoScaleOrReflection,
        other => {
            return Err(JsonError::UnknownValue {
                entity: "inherit",
                value: other.to_string(),
            });
        }
    })
}

/// Parse a spine hex color. With `has_alpha`, the string is 8 hex chars
/// ("RRGGBBAA"); otherwise 6 ("RRGGBB") with alpha defaulting to 1.
fn parse_color(s: &str, has_alpha: bool) -> Result<Color, JsonError> {
    fn component(src: &str, i: usize) -> Result<f32, JsonError> {
        let start = i * 2;
        let slice = src
            .get(start..start + 2)
            .ok_or_else(|| JsonError::InvalidColor {
                value: src.to_string(),
                message: format!("missing pair at offset {start}"),
            })?;
        u32::from_str_radix(slice, 16)
            .map(|x| x as f32 / 255.0)
            .map_err(|_| JsonError::InvalidColor {
                value: src.to_string(),
                message: format!("invalid hex pair {slice:?}"),
            })
    }
    let r = component(s, 0)?;
    let g = component(s, 1)?;
    let b = component(s, 2)?;
    let a = if has_alpha { component(s, 3)? } else { 1.0 };
    Ok(Color::new(r, g, b, a))
}

fn read_sequence(v: Option<&Value>) -> Option<Sequence> {
    let item = v?;
    let count = get_int(item, "count", 0);
    let mut seq = Sequence::new(count);
    seq.start = get_int(item, "start", 1);
    seq.digits = get_int(item, "digits", 0);
    seq.setup_index = get_int(item, "setupIndex", 0);
    Some(seq)
}

/// Extract a single bezier segment `(cx1, cy1, cx2, cy2)` for channel
/// `value_index` from a curve field. The `curve` array, when set, lays out
/// control points channel-major: `[cx1_0, cy1_0, cx2_0, cy2_0, cx1_1, ...]`.
fn curve_segment(curve: &Value, value_index: usize) -> Option<(f32, f32, f32, f32)> {
    let arr = curve.as_array()?;
    let base = value_index * 4;
    if arr.len() < base + 4 {
        return None;
    }
    let cx1 = arr.get(base)?.as_f64()? as f32;
    let cy1 = arr.get(base + 1)?.as_f64()? as f32;
    let cx2 = arr.get(base + 2)?.as_f64()? as f32;
    let cy2 = arr.get(base + 3)?.as_f64()? as f32;
    Some((cx1, cy1, cx2, cy2))
}

/// Mark frame `frame` as linear in the given curves tail.
fn set_linear(curves: &mut [f32], frame: usize) {
    curves[frame] = CURVE_LINEAR;
}

fn set_stepped(curves: &mut [f32], frame: usize) {
    curves[frame] = CURVE_STEPPED;
}

/// Record a bezier for `frame`/`value_index` at `bezier_seg_idx` slot into
/// `curves`. Returns the new `bezier_seg_idx` if this was the first channel
/// of the frame (caller increments once per frame after all channels are
/// written), otherwise ignores.
fn set_bezier_sample(
    curves: &mut [f32],
    frame_count: usize,
    frame: usize,
    value_index: usize,
    bezier_seg_idx: usize,
    samples: [f32; BEZIER_SIZE],
) {
    if value_index == 0 {
        let first_channel_abs = frame_count + bezier_seg_idx * BEZIER_SIZE;
        curves[frame] = CURVE_BEZIER + first_channel_abs as f32;
    }
    let dst = frame_count + (bezier_seg_idx + value_index) * BEZIER_SIZE;
    curves[dst..dst + BEZIER_SIZE].copy_from_slice(&samples);
}

/// Process a single-channel CurveTimeline1 by reading `time`/`value_key` from
/// each key. `default` matches `spine-cpp`'s `defaultValue`.
fn read_timeline1(
    keys: &[Value],
    value_key: &str,
    default: f32,
    scale: f32,
) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let (mut frames, mut curves, mut bezier_count) = (
        Vec::with_capacity(frame_count * 2),
        Vec::<f32>::new(),
        0usize,
    );
    // First pass: count bezier channels to size the curves tail.
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 1;
            }
        }
    }
    curves.resize(frame_count + bezier_count * BEZIER_SIZE, 0.0);
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut value = get_f32(&keys[0], value_key, default) * scale;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.push(value);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let value2 = get_f32(next, value_key, default) * scale;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                let (cx1, cy1, cx2, cy2) =
                    curve_segment(v, 0).unwrap_or((time, value, time2, value2));
                let samples = compute_bezier_samples(
                    time,
                    value,
                    cx1,
                    cy1 * scale,
                    cx2,
                    cy2 * scale,
                    time2,
                    value2,
                );
                set_bezier_sample(&mut curves, frame_count, frame, 0, bezier_seg_idx, samples);
                bezier_seg_idx += 1;
            }
        }
        time = time2;
        value = value2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Two-channel CurveTimeline2. `name1` / `name2` select which JSON fields are
/// the channel values (e.g. "x", "y"). `default` is the fallback for missing
/// channel keys.
fn read_timeline2(
    keys: &[Value],
    name1: &str,
    name2: &str,
    default: f32,
    scale: f32,
) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 2;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 3);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut v1 = get_f32(&keys[0], name1, default) * scale;
    let mut v2 = get_f32(&keys[0], name2, default) * scale;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.push(v1);
        frames.push(v2);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let nv1 = get_f32(next, name1, default) * scale;
        let nv2 = get_f32(next, name2, default) * scale;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                for (k_idx, (val1, val2)) in [(v1, nv1), (v2, nv2)].iter().enumerate() {
                    let (cx1, cy1, cx2, cy2) =
                        curve_segment(v, k_idx).unwrap_or((time, *val1, time2, *val2));
                    let samples = compute_bezier_samples(
                        time,
                        *val1,
                        cx1,
                        cy1 * scale,
                        cx2,
                        cy2 * scale,
                        time2,
                        *val2,
                    );
                    set_bezier_sample(
                        &mut curves,
                        frame_count,
                        frame,
                        k_idx,
                        bezier_seg_idx,
                        samples,
                    );
                }
                bezier_seg_idx += 2;
            }
        }
        time = time2;
        v1 = nv1;
        v2 = nv2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Color timelines with per-frame hex string. `channels` is 3 (RGB) or 4 (RGBA).
fn read_color_timeline_json(
    keys: &[Value],
    channels: usize,
    color_field: &str,
    has_alpha: bool,
) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += channels;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * (1 + channels));
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let parse_colors = |k: &Value| -> Result<Vec<f32>, JsonError> {
        let s = get_str(k, color_field).unwrap_or("");
        let c = parse_color(s, has_alpha)?;
        let mut out = vec![c.r, c.g, c.b];
        if channels == 4 {
            out.push(c.a);
        }
        Ok(out)
    };

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut values = parse_colors(&keys[0])?;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.extend_from_slice(&values);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let values2 = parse_colors(next)?;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                for k_idx in 0..channels {
                    let (cx1, cy1, cx2, cy2) = curve_segment(v, k_idx).unwrap_or((
                        time,
                        values[k_idx],
                        time2,
                        values2[k_idx],
                    ));
                    let samples = compute_bezier_samples(
                        time,
                        values[k_idx],
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        time2,
                        values2[k_idx],
                    );
                    set_bezier_sample(
                        &mut curves,
                        frame_count,
                        frame,
                        k_idx,
                        bezier_seg_idx,
                        samples,
                    );
                }
                bezier_seg_idx += channels;
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

fn read_alpha_timeline_json(keys: &[Value]) -> Result<CurveFrames, JsonError> {
    // spine-cpp reads alpha via readTimeline (single-channel) with default 0
    // and scale 1, but pulls `value` from a color-like field. Spine 4.2's
    // JSON for alpha timelines actually uses `color` (single-channel hex).
    // Detect either: numeric `value` (shape from readTimeline) or string `color`.
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 1;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 2);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    fn alpha_of(k: &Value) -> Result<f32, JsonError> {
        if let Some(v) = k.get("value").and_then(Value::as_f64) {
            return Ok(v as f32);
        }
        if let Some(s) = get_str(k, "color") {
            // Single-channel alpha encoded as first hex pair.
            let byte = u32::from_str_radix(s.get(0..2).unwrap_or("00"), 16).map_err(|_| {
                JsonError::InvalidColor {
                    value: s.to_string(),
                    message: "invalid alpha hex".to_string(),
                }
            })?;
            return Ok(byte as f32 / 255.0);
        }
        Ok(0.0)
    }

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut value = alpha_of(&keys[0])?;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.push(value);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let value2 = alpha_of(next)?;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                let (cx1, cy1, cx2, cy2) =
                    curve_segment(v, 0).unwrap_or((time, value, time2, value2));
                let samples =
                    compute_bezier_samples(time, value, cx1, cy1, cx2, cy2, time2, value2);
                set_bezier_sample(&mut curves, frame_count, frame, 0, bezier_seg_idx, samples);
                bezier_seg_idx += 1;
            }
        }
        time = time2;
        value = value2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Dual-color timelines (rgba2/rgb2). `has_alpha` true = rgba2 (7 channels:
/// light RGBA + dark RGB), false = rgb2 (6 channels).
fn read_rgb2_timeline_json(keys: &[Value], has_alpha: bool) -> Result<CurveFrames, JsonError> {
    let channels = if has_alpha { 7 } else { 6 };
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += channels;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * (1 + channels));
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let parse = |k: &Value| -> Result<Vec<f32>, JsonError> {
        let light = parse_color(get_str(k, "light").unwrap_or(""), has_alpha)?;
        let dark = parse_color(get_str(k, "dark").unwrap_or(""), false)?;
        let mut v = vec![light.r, light.g, light.b];
        if has_alpha {
            v.push(light.a);
        }
        v.extend_from_slice(&[dark.r, dark.g, dark.b]);
        Ok(v)
    };

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut values = parse(&keys[0])?;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.extend_from_slice(&values);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let values2 = parse(next)?;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                for k_idx in 0..channels {
                    let (cx1, cy1, cx2, cy2) = curve_segment(v, k_idx).unwrap_or((
                        time,
                        values[k_idx],
                        time2,
                        values2[k_idx],
                    ));
                    let samples = compute_bezier_samples(
                        time,
                        values[k_idx],
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        time2,
                        values2[k_idx],
                    );
                    set_bezier_sample(
                        &mut curves,
                        frame_count,
                        frame,
                        k_idx,
                        bezier_seg_idx,
                        samples,
                    );
                }
                bezier_seg_idx += channels;
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

/// IK constraint timeline: stride-6 frames (time + mix + softness + flags
/// bend/compress/stretch). Two bezier channels on bezier frames (mix, softness).
fn read_ik_timeline_json(keys: &[Value], scale: f32) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 2;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 6);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut mix = get_f32(&keys[0], "mix", 1.0);
    let mut softness = get_f32(&keys[0], "softness", 0.0) * scale;
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        let bend = if get_bool(&keys[frame], "bendPositive", true) {
            1.0
        } else {
            -1.0
        };
        let compress = if get_bool(&keys[frame], "compress", false) {
            1.0
        } else {
            0.0
        };
        let stretch = if get_bool(&keys[frame], "stretch", false) {
            1.0
        } else {
            0.0
        };
        frames.push(time);
        frames.push(mix);
        frames.push(softness);
        frames.push(bend);
        frames.push(compress);
        frames.push(stretch);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let mix2 = get_f32(next, "mix", 1.0);
        let softness2 = get_f32(next, "softness", 0.0) * scale;
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                let (cx1, cy1, cx2, cy2) = curve_segment(v, 0).unwrap_or((time, mix, time2, mix2));
                let s0 = compute_bezier_samples(time, mix, cx1, cy1, cx2, cy2, time2, mix2);
                set_bezier_sample(&mut curves, frame_count, frame, 0, bezier_seg_idx, s0);

                let (cx1, cy1, cx2, cy2) =
                    curve_segment(v, 1).unwrap_or((time, softness, time2, softness2));
                let s1 = compute_bezier_samples(
                    time,
                    softness,
                    cx1,
                    cy1 * scale,
                    cx2,
                    cy2 * scale,
                    time2,
                    softness2,
                );
                set_bezier_sample(&mut curves, frame_count, frame, 1, bezier_seg_idx, s1);
                bezier_seg_idx += 2;
            }
        }
        time = time2;
        mix = mix2;
        softness = softness2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Transform constraint timeline — stride 7 (time + 6 mixes).
fn read_transform_timeline_json(keys: &[Value]) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 6;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 7);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let parse = |k: &Value| {
        let mix_rotate = get_f32(k, "mixRotate", 1.0);
        let mix_shear_y = get_f32(k, "mixShearY", 1.0);
        let mix_x = get_f32(k, "mixX", 1.0);
        let mix_y = get_f32(k, "mixY", mix_x);
        let mix_scale_x = get_f32(k, "mixScaleX", 1.0);
        let mix_scale_y = get_f32(k, "mixScaleY", mix_scale_x);
        // Frame order matches spine-cpp's setFrame signature:
        // (time, mixRotate, mixX, mixY, mixScaleX, mixScaleY, mixShearY).
        [
            mix_rotate,
            mix_x,
            mix_y,
            mix_scale_x,
            mix_scale_y,
            mix_shear_y,
        ]
    };

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut values = parse(&keys[0]);
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.extend_from_slice(&values);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let values2 = parse(next);
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                for k_idx in 0..6 {
                    let (cx1, cy1, cx2, cy2) = curve_segment(v, k_idx).unwrap_or((
                        time,
                        values[k_idx],
                        time2,
                        values2[k_idx],
                    ));
                    let samples = compute_bezier_samples(
                        time,
                        values[k_idx],
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        time2,
                        values2[k_idx],
                    );
                    set_bezier_sample(
                        &mut curves,
                        frame_count,
                        frame,
                        k_idx,
                        bezier_seg_idx,
                        samples,
                    );
                }
                bezier_seg_idx += 6;
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Path-constraint mix timeline — stride 4 (time + mixRotate + mixX + mixY).
fn read_path_mix_timeline_json(keys: &[Value]) -> Result<CurveFrames, JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 3;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count * 4);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    if frame_count == 0 {
        return Ok(CurveFrames { frames, curves });
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let parse = |k: &Value| {
        let mix_rotate = get_f32(k, "mixRotate", 1.0);
        let mix_x = get_f32(k, "mixX", 1.0);
        let mix_y = get_f32(k, "mixY", mix_x);
        [mix_rotate, mix_x, mix_y]
    };

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut values = parse(&keys[0]);
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        frames.extend_from_slice(&values);
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let values2 = parse(next);
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                for k_idx in 0..3 {
                    let (cx1, cy1, cx2, cy2) = curve_segment(v, k_idx).unwrap_or((
                        time,
                        values[k_idx],
                        time2,
                        values2[k_idx],
                    ));
                    let samples = compute_bezier_samples(
                        time,
                        values[k_idx],
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        time2,
                        values2[k_idx],
                    );
                    set_bezier_sample(
                        &mut curves,
                        frame_count,
                        frame,
                        k_idx,
                        bezier_seg_idx,
                        samples,
                    );
                }
                bezier_seg_idx += 3;
            }
        }
        time = time2;
        values = values2;
    }
    Ok(CurveFrames { frames, curves })
}

/// Deform timeline. Returns `(frame_times, curve_data, per_frame_vertices)`
/// in the same layout used by `load::binary::parse::read_deform_timeline`.
fn read_deform_timeline_json(
    keys: &[Value],
    deform_length: usize,
    scale: f32,
    weighted: bool,
    setup_vertices: &[f32],
) -> Result<(Vec<f32>, Vec<f32>, Vec<Vec<f32>>), JsonError> {
    let frame_count = keys.len();
    let mut bezier_count = 0usize;
    for k in keys.iter().take(frame_count.saturating_sub(1)) {
        if let Some(curve) = k.get("curve") {
            if !curve.is_string() {
                bezier_count += 1;
            }
        }
    }
    let mut frames: Vec<f32> = Vec::with_capacity(frame_count);
    let mut curves: Vec<f32> = vec![0.0; frame_count + bezier_count * BEZIER_SIZE];
    let mut vertices: Vec<Vec<f32>> = Vec::with_capacity(frame_count);
    if frame_count == 0 {
        return Ok((frames, curves, vertices));
    }
    curves[frame_count - 1] = CURVE_STEPPED;

    let deform_of = |k: &Value| -> Vec<f32> {
        match k.get("vertices").and_then(Value::as_array) {
            None => {
                if weighted {
                    vec![0.0; deform_length]
                } else {
                    setup_vertices.to_vec()
                }
            }
            Some(verts) => {
                let start = get_int(k, "offset", 0) as usize;
                let mut deform = vec![0.0_f32; deform_length];
                for (i, v) in verts.iter().enumerate() {
                    let idx = start + i;
                    if idx >= deform_length {
                        break;
                    }
                    deform[idx] = v.as_f64().unwrap_or(0.0) as f32 * scale;
                }
                if !weighted {
                    for (d, s) in deform.iter_mut().zip(setup_vertices.iter()) {
                        *d += *s;
                    }
                }
                deform
            }
        }
    };

    let mut time = get_f32(&keys[0], "time", 0.0);
    let mut bezier_seg_idx = 0usize;
    for frame in 0..frame_count {
        frames.push(time);
        vertices.push(deform_of(&keys[frame]));
        if frame + 1 >= frame_count {
            break;
        }
        let next = &keys[frame + 1];
        let time2 = get_f32(next, "time", 0.0);
        let curve = keys[frame].get("curve");
        match curve {
            None => set_linear(&mut curves, frame),
            Some(v) if v.as_str() == Some("stepped") => set_stepped(&mut curves, frame),
            Some(v) => {
                let (cx1, cy1, cx2, cy2) = curve_segment(v, 0).unwrap_or((time, 0.0, time2, 1.0));
                // Deform timelines pass (value1=0, value2=1) so the bezier
                // samples carry a 0→1 progression across the segment.
                let samples = compute_bezier_samples(time, 0.0, cx1, cy1, cx2, cy2, time2, 1.0);
                let tail_offset = frame_count + bezier_seg_idx * BEZIER_SIZE;
                curves[frame] = CURVE_BEZIER + tail_offset as f32;
                curves[tail_offset..tail_offset + BEZIER_SIZE].copy_from_slice(&samples);
                bezier_seg_idx += 1;
            }
        }
        time = time2;
    }
    Ok((frames, curves, vertices))
}

/// Pull the deform-relevant `(weighted, setup_vertices_clone)` from an
/// attachment. Mirrors the helper in `load::binary::parse`.
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

fn timeline_duration(anim: &Animation) -> f32 {
    fn last_time_stride(frames: &[f32], stride: usize) -> f32 {
        if frames.len() < stride {
            return 0.0;
        }
        frames[frames.len() - stride]
    }
    let mut max = 0.0f32;
    for t in &anim.timelines {
        let last = match t {
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
            | Timeline::Physics { curves, .. } => last_time_stride(&curves.frames, 2),
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
            | Timeline::Attachment { frames, .. }
            | Timeline::Event { frames, .. } => frames.last().copied().unwrap_or(0.0),
            Timeline::Deform { curves, .. } => last_time_stride(&curves.frames, 2),
            Timeline::Sequence { frames, .. } => last_time_stride(frames, 3),
        };
        max = max.max(last);
    }
    max
}
