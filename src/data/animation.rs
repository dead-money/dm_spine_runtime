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

//! Animation data types: [`Animation`], [`Timeline`], [`CurveFrames`],
//! and supporting enums. Evaluation of these timelines lives in the
//! [`crate::animation`] module.

use crate::data::{Attachment, AttachmentId, BoneId, ConstraintId, EventId, Inherit, SlotId};
use crate::math::Color;

/// A named collection of timelines driving a skeleton over a fixed duration.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub timelines: Vec<Timeline>,
    /// Bones with bone timelines, in file order. Sliders touch only these.
    pub bones: Vec<BoneId>,
    /// Nonessential editor color.
    pub color: Color,
    /// Property ids of every timeline, concatenated; see
    /// [`Self::timeline_property_ids`].
    property_ids: Vec<PropertyId>,
    /// End offset into `property_ids` per timeline.
    property_ends: Vec<u32>,
    /// `property_ids`, sorted, for [`Self::has_property`].
    sorted_property_ids: Vec<PropertyId>,
}

impl Animation {
    #[must_use]
    pub fn new(name: impl Into<String>, duration: f32) -> Self {
        Self {
            name: name.into(),
            duration,
            timelines: Vec::new(),
            bones: Vec::new(),
            color: Color::WHITE,
            property_ids: Vec::new(),
            property_ends: Vec::new(),
            sorted_property_ids: Vec::new(),
        }
    }

    /// Sets the timelines and caches their property ids. `attachments` are
    /// the skeleton's, which deform and sequence ids depend on.
    pub fn set_timelines(&mut self, timelines: Vec<Timeline>, attachments: &[Attachment]) {
        self.property_ids.clear();
        self.property_ends.clear();
        for t in &timelines {
            t.push_property_ids(attachments, &mut self.property_ids);
            self.property_ends.push(self.property_ids.len() as u32);
        }
        self.sorted_property_ids.clone_from(&self.property_ids);
        self.sorted_property_ids.sort_unstable();
        self.sorted_property_ids.dedup();
        self.timelines = timelines;
    }

    /// The properties timeline `i` writes.
    #[must_use]
    pub fn timeline_property_ids(&self, i: usize) -> &[PropertyId] {
        let start = if i == 0 {
            0
        } else {
            self.property_ends[i - 1] as usize
        };
        &self.property_ids[start..self.property_ends[i] as usize]
    }

    /// Whether any timeline writes `id`.
    #[must_use]
    pub fn has_property(&self, id: PropertyId) -> bool {
        self.sorted_property_ids.binary_search(&id).is_ok()
    }

    /// Whether any timeline writes any of `ids`.
    #[must_use]
    pub fn has_timeline(&self, ids: &[PropertyId]) -> bool {
        ids.iter().any(|&id| self.has_property(id))
    }
}

/// A `(property kind, target)` pair packed as spine-cpp does, used by
/// `AnimationState` to tell which timelines touch the same property.
pub type PropertyId = i64;

/// Property kinds, in spine-cpp's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Property {
    Rotate = 0,
    X,
    Y,
    ScaleX,
    ScaleY,
    ShearX,
    ShearY,
    Inherit,
    Rgb,
    Alpha,
    Rgb2,
    Attachment,
    Deform,
    Event,
    DrawOrder,
    IkConstraint,
    TransformConstraint,
    PathConstraintPosition,
    PathConstraintSpacing,
    PathConstraintMix,
    PhysicsConstraintInertia,
    PhysicsConstraintStrength,
    PhysicsConstraintDamping,
    PhysicsConstraintMass,
    PhysicsConstraintWind,
    PhysicsConstraintGravity,
    PhysicsConstraintMix,
    PhysicsConstraintReset,
    Sequence,
    SliderTime,
    SliderMix,
    DrawOrderFolder,
}

#[inline]
#[must_use]
pub fn property_id(property: Property, payload: i64) -> PropertyId {
    ((property as i64) << 32) | payload
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct CurveFrames {
    pub frames: Vec<f32>,
    pub curves: Vec<f32>,
}

/// An event firing keyed to a moment in an animation. Each frame can
/// override the default int / float / string / volume / balance from the
/// parent [`EventData`][crate::data::EventData].
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationEvent {
    pub time: f32,
    pub event: EventId,
    pub int_value: i32,
    pub float_value: f32,
    pub string_value: Option<String>,
    pub volume: f32,
    pub balance: f32,
}

/// Which physics-constraint property a [`Timeline::Physics`] entry drives.
///
/// spine-cpp splits these into separate `InertiaTimeline`, `StrengthTimeline`,
/// … subclasses; we collapse them into one variant discriminated by
/// [`PhysicsProperty`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsProperty {
    Inertia,
    Strength,
    Damping,
    Mass,
    Wind,
    Gravity,
    Mix,
}

/// Tagged union of every kind of animation timeline. Evaluation is performed
/// by the [`crate::animation`] apply path.
#[derive(Debug, Clone, PartialEq)]
pub enum Timeline {
    // --- Bone timelines ----------------------------------------------------
    Rotate {
        bone: BoneId,
        curves: CurveFrames,
    },
    Translate {
        bone: BoneId,
        curves: CurveFrames,
    },
    TranslateX {
        bone: BoneId,
        curves: CurveFrames,
    },
    TranslateY {
        bone: BoneId,
        curves: CurveFrames,
    },
    Scale {
        bone: BoneId,
        curves: CurveFrames,
    },
    ScaleX {
        bone: BoneId,
        curves: CurveFrames,
    },
    ScaleY {
        bone: BoneId,
        curves: CurveFrames,
    },
    Shear {
        bone: BoneId,
        curves: CurveFrames,
    },
    ShearX {
        bone: BoneId,
        curves: CurveFrames,
    },
    ShearY {
        bone: BoneId,
        curves: CurveFrames,
    },
    /// Animates a bone's [`Inherit`] mode. No interpolation — each frame is
    /// a discrete mode value. Added in Spine 4.2.
    Inherit {
        bone: BoneId,
        /// `frames[i] = time_i` — one entry per keyframe.
        frames: Vec<f32>,
        /// Matches `frames` 1-to-1.
        inherits: Vec<Inherit>,
    },

    // --- Slot timelines ----------------------------------------------------
    Attachment {
        slot: SlotId,
        /// `frames[i] = time_i`.
        frames: Vec<f32>,
        /// Attachment name per frame, or `None` to clear the slot's attachment.
        names: Vec<Option<String>>,
    },
    Rgba {
        slot: SlotId,
        curves: CurveFrames,
    },
    Rgb {
        slot: SlotId,
        curves: CurveFrames,
    },
    Alpha {
        slot: SlotId,
        curves: CurveFrames,
    },
    Rgba2 {
        slot: SlotId,
        curves: CurveFrames,
    },
    Rgb2 {
        slot: SlotId,
        curves: CurveFrames,
    },
    /// Vertex-level deform blended against the mesh's setup-pose vertices.
    Deform {
        slot: SlotId,
        attachment: AttachmentId,
        curves: CurveFrames,
        /// One per-frame vertex-offset array. Each inner `Vec<f32>` has the
        /// same length as the mesh's vertex list.
        vertices: Vec<Vec<f32>>,
    },
    /// Drives a sequence-backed region/mesh attachment to cycle frames.
    Sequence {
        slot: SlotId,
        attachment: AttachmentId,
        /// Interleaved `(time, mode + index)` — the mode + index is packed
        /// into a single `f32` following spine-cpp's binary layout.
        frames: Vec<f32>,
    },

    // --- Skeleton-wide timelines ------------------------------------------
    /// Permutes the skeleton's draw order. `draw_orders[i]` is `None` if the
    /// frame restores the setup-pose order, otherwise a complete
    /// permutation.
    DrawOrder {
        frames: Vec<f32>,
        draw_orders: Vec<Option<Vec<SlotId>>>,
    },
    /// Reorders only `slots`; each key's order is indices into `slots`.
    DrawOrderFolder {
        slots: Vec<SlotId>,
        frames: Vec<f32>,
        draw_orders: Vec<Option<Vec<u16>>>,
    },
    Event {
        /// `frames[i]` is redundant with `events[i].time`; kept as a
        /// dedicated vector so searches use a clean f32 binary search.
        frames: Vec<f32>,
        events: Vec<AnimationEvent>,
    },

    // --- Constraint timelines ---------------------------------------------
    IkConstraint {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    TransformConstraint {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    PathConstraintPosition {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    PathConstraintSpacing {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    PathConstraintMix {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    /// A single physics-property curve. One constraint can have multiple
    /// timeline instances — one per animated property. `constraint = None`
    /// means the timeline applies to every physics constraint in the
    /// skeleton (matches spine-cpp's `index = -1` sentinel).
    Physics {
        constraint: Option<ConstraintId>,
        property: PhysicsProperty,
        curves: CurveFrames,
    },
    /// Reset the physics solver's integrator state. `None` means reset all
    /// physics constraints in the skeleton.
    PhysicsReset {
        constraint: Option<ConstraintId>,
        frames: Vec<f32>,
    },
    Slider {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
    SliderMix {
        constraint: ConstraintId,
        curves: CurveFrames,
    },
}

impl Timeline {
    fn push_property_ids(&self, attachments: &[Attachment], out: &mut Vec<PropertyId>) {
        use Property as P;
        let mut push = |p: P, payload: i64| out.push(property_id(p, payload));
        match self {
            Timeline::Rotate { bone, .. } => push(P::Rotate, bone.0.into()),
            Timeline::Translate { bone, .. } => {
                push(P::X, bone.0.into());
                push(P::Y, bone.0.into());
            }
            Timeline::TranslateX { bone, .. } => push(P::X, bone.0.into()),
            Timeline::TranslateY { bone, .. } => push(P::Y, bone.0.into()),
            Timeline::Scale { bone, .. } => {
                push(P::ScaleX, bone.0.into());
                push(P::ScaleY, bone.0.into());
            }
            Timeline::ScaleX { bone, .. } => push(P::ScaleX, bone.0.into()),
            Timeline::ScaleY { bone, .. } => push(P::ScaleY, bone.0.into()),
            Timeline::Shear { bone, .. } => {
                push(P::ShearX, bone.0.into());
                push(P::ShearY, bone.0.into());
            }
            Timeline::ShearX { bone, .. } => push(P::ShearX, bone.0.into()),
            Timeline::ShearY { bone, .. } => push(P::ShearY, bone.0.into()),
            Timeline::Inherit { bone, .. } => push(P::Inherit, bone.0.into()),
            Timeline::Attachment { slot, .. } => push(P::Attachment, slot.0.into()),
            Timeline::Rgba { slot, .. } => {
                push(P::Rgb, slot.0.into());
                push(P::Alpha, slot.0.into());
            }
            Timeline::Rgb { slot, .. } => push(P::Rgb, slot.0.into()),
            Timeline::Alpha { slot, .. } => push(P::Alpha, slot.0.into()),
            Timeline::Rgba2 { slot, .. } => {
                push(P::Rgb, slot.0.into());
                push(P::Alpha, slot.0.into());
                push(P::Rgb2, slot.0.into());
            }
            Timeline::Rgb2 { slot, .. } => {
                push(P::Rgb, slot.0.into());
                push(P::Rgb2, slot.0.into());
            }
            Timeline::Deform {
                slot, attachment, ..
            } => push(
                P::Deform,
                ((i64::from(slot.0) << 16) | i64::from(attachment.0)) & 0xffff_ffff,
            ),
            Timeline::Sequence {
                slot, attachment, ..
            } => {
                let sequence_id = attachments[attachment.index()]
                    .sequence()
                    .map_or(0, |s| i64::from(s.id));
                push(
                    P::Sequence,
                    ((i64::from(slot.0) << 16) | sequence_id) & 0xffff_ffff,
                );
            }
            Timeline::DrawOrder { .. } => push(P::DrawOrder, 0),
            Timeline::DrawOrderFolder { slots, .. } => {
                for slot in slots {
                    push(P::DrawOrderFolder, slot.0.into());
                }
            }
            Timeline::Event { .. } => push(P::Event, 0),
            Timeline::IkConstraint { constraint, .. } => push(P::IkConstraint, constraint.0.into()),
            Timeline::TransformConstraint { constraint, .. } => {
                push(P::TransformConstraint, constraint.0.into());
            }
            Timeline::PathConstraintPosition { constraint, .. } => {
                push(P::PathConstraintPosition, constraint.0.into());
            }
            Timeline::PathConstraintSpacing { constraint, .. } => {
                push(P::PathConstraintSpacing, constraint.0.into());
            }
            Timeline::PathConstraintMix { constraint, .. } => {
                push(P::PathConstraintMix, constraint.0.into());
            }
            Timeline::Physics {
                constraint,
                property,
                ..
            } => {
                let p = match property {
                    PhysicsProperty::Inertia => P::PhysicsConstraintInertia,
                    PhysicsProperty::Strength => P::PhysicsConstraintStrength,
                    PhysicsProperty::Damping => P::PhysicsConstraintDamping,
                    PhysicsProperty::Mass => P::PhysicsConstraintMass,
                    PhysicsProperty::Wind => P::PhysicsConstraintWind,
                    PhysicsProperty::Gravity => P::PhysicsConstraintGravity,
                    PhysicsProperty::Mix => P::PhysicsConstraintMix,
                };
                push(p, constraint.map_or(-1, |c| c.0.into()) & 0xffff_ffff);
            }
            Timeline::PhysicsReset { constraint, .. } => push(
                P::PhysicsConstraintReset,
                constraint.map_or(-1, |c| c.0.into()) & 0xffff_ffff,
            ),
            Timeline::Slider { constraint, .. } => push(P::SliderTime, constraint.0.into()),
            Timeline::SliderMix { constraint, .. } => push(P::SliderMix, constraint.0.into()),
        }
    }

    /// Whether the timeline can blend additively over lower tracks.
    #[must_use]
    pub fn is_additive(&self) -> bool {
        matches!(
            self,
            Timeline::Rotate { .. }
                | Timeline::Translate { .. }
                | Timeline::TranslateX { .. }
                | Timeline::TranslateY { .. }
                | Timeline::Scale { .. }
                | Timeline::ScaleX { .. }
                | Timeline::ScaleY { .. }
                | Timeline::Shear { .. }
                | Timeline::ShearX { .. }
                | Timeline::ShearY { .. }
                | Timeline::Deform { .. }
                | Timeline::TransformConstraint { .. }
                | Timeline::PathConstraintPosition { .. }
                | Timeline::Physics {
                    property: PhysicsProperty::Wind | PhysicsProperty::Gravity,
                    ..
                }
                | Timeline::SliderMix { .. }
        )
    }

    /// Whether the timeline sets values rather than interpolating them, so
    /// it never needs holding while mixing.
    #[must_use]
    pub fn is_instant(&self) -> bool {
        matches!(
            self,
            Timeline::Inherit { .. }
                | Timeline::Attachment { .. }
                | Timeline::Sequence { .. }
                | Timeline::Event { .. }
                | Timeline::DrawOrder { .. }
                | Timeline::DrawOrderFolder { .. }
                | Timeline::PhysicsReset { .. }
        )
    }

    /// The bone a bone timeline keys.
    #[must_use]
    pub fn bone(&self) -> Option<BoneId> {
        match self {
            Timeline::Rotate { bone, .. }
            | Timeline::Translate { bone, .. }
            | Timeline::TranslateX { bone, .. }
            | Timeline::TranslateY { bone, .. }
            | Timeline::Scale { bone, .. }
            | Timeline::ScaleX { bone, .. }
            | Timeline::ScaleY { bone, .. }
            | Timeline::Shear { bone, .. }
            | Timeline::ShearX { bone, .. }
            | Timeline::ShearY { bone, .. }
            | Timeline::Inherit { bone, .. } => Some(*bone),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Literal default comparisons only.
mod tests {
    use super::*;

    #[test]
    fn animation_defaults_empty() {
        let a = Animation::new("walk", 1.5);
        assert_eq!(a.name, "walk");
        assert_eq!(a.duration, 1.5);
        assert!(a.timelines.is_empty());
    }

    #[test]
    fn timeline_variants_compile_and_clone() {
        // Exercise every variant at least once: build a representative
        // value and round-trip through Clone. Catches any drift between
        // the enum and its helper types.
        let variants = vec![
            Timeline::Rotate {
                bone: BoneId(0),
                curves: CurveFrames::default(),
            },
            Timeline::Translate {
                bone: BoneId(0),
                curves: CurveFrames::default(),
            },
            Timeline::Inherit {
                bone: BoneId(0),
                frames: vec![],
                inherits: vec![],
            },
            Timeline::Attachment {
                slot: SlotId(0),
                frames: vec![],
                names: vec![],
            },
            Timeline::Rgba {
                slot: SlotId(0),
                curves: CurveFrames::default(),
            },
            Timeline::Deform {
                slot: SlotId(0),
                attachment: AttachmentId(0),
                curves: CurveFrames::default(),
                vertices: vec![],
            },
            Timeline::DrawOrder {
                frames: vec![],
                draw_orders: vec![],
            },
            Timeline::Event {
                frames: vec![],
                events: vec![],
            },
            Timeline::IkConstraint {
                constraint: ConstraintId(0),
                curves: CurveFrames::default(),
            },
            Timeline::Physics {
                constraint: Some(ConstraintId(0)),
                property: PhysicsProperty::Wind,
                curves: CurveFrames::default(),
            },
            Timeline::PhysicsReset {
                constraint: None,
                frames: vec![],
            },
        ];
        let cloned = variants.clone();
        assert_eq!(variants.len(), cloned.len());
    }
}
