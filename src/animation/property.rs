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

//! Timeline property-ID encoding.
//!
//! spine-cpp packs a `(property_kind, index_or_payload)` pair into a
//! 64-bit integer. `AnimationState::computeHold` reads these to detect
//! which timelines on a mixing-out entry also appear on lower tracks, so
//! it can choose the right blend mode per timeline.
//!
//! Encoding: `(property_kind as i64) << 32 | (payload & 0xffffffff)`.
//! Matches `spine::Property` bit-flag values exactly so cross-referencing
//! against spine-cpp is mechanical.

use crate::data::{PhysicsProperty, Timeline};

/// Packed `(property_kind, payload)` pair identifying a timeline's target.
/// 64-bit so Deform can stash two 16-bit indices (slot, attachment) in
/// the low half without collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropertyId(pub i64);

/// `spine::Property` bit flags. The discriminant values are identical so
/// the packed `PropertyId`s match spine-cpp byte-for-byte for a given
/// timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Property {
    Rotate = 1 << 0,
    X = 1 << 1,
    Y = 1 << 2,
    ScaleX = 1 << 3,
    ScaleY = 1 << 4,
    ShearX = 1 << 5,
    ShearY = 1 << 6,
    Inherit = 1 << 7,
    Rgb = 1 << 8,
    Alpha = 1 << 9,
    Rgb2 = 1 << 10,
    Attachment = 1 << 11,
    Deform = 1 << 12,
    Event = 1 << 13,
    DrawOrder = 1 << 14,
    IkConstraint = 1 << 15,
    TransformConstraint = 1 << 16,
    PathConstraintPosition = 1 << 17,
    PathConstraintSpacing = 1 << 18,
    PathConstraintMix = 1 << 19,
    PhysicsConstraintInertia = 1 << 20,
    PhysicsConstraintStrength = 1 << 21,
    PhysicsConstraintDamping = 1 << 22,
    PhysicsConstraintMass = 1 << 23,
    PhysicsConstraintWind = 1 << 24,
    PhysicsConstraintGravity = 1 << 25,
    PhysicsConstraintMix = 1 << 26,
    PhysicsConstraintReset = 1 << 27,
    Sequence = 1 << 28,
}

#[inline]
#[must_use]
fn id(property: Property, payload: u32) -> PropertyId {
    PropertyId((property as i64) << 32 | i64::from(payload))
}

#[must_use]
fn physics_property_kind(prop: PhysicsProperty) -> Property {
    match prop {
        PhysicsProperty::Inertia => Property::PhysicsConstraintInertia,
        PhysicsProperty::Strength => Property::PhysicsConstraintStrength,
        PhysicsProperty::Damping => Property::PhysicsConstraintDamping,
        PhysicsProperty::Mass => Property::PhysicsConstraintMass,
        PhysicsProperty::Wind => Property::PhysicsConstraintWind,
        PhysicsProperty::Gravity => Property::PhysicsConstraintGravity,
        PhysicsProperty::Mix => Property::PhysicsConstraintMix,
    }
}

/// Generate the property IDs this timeline writes. Multiple IDs indicate
/// the timeline covers several properties (e.g. RGBA writes both Rgb and
/// Alpha; Rgba2 writes Rgb, Alpha, and Rgb2).
///
/// Ports the per-timeline `setPropertyIds` calls in spine-cpp.
#[must_use]
pub fn property_ids(timeline: &Timeline) -> Vec<PropertyId> {
    match timeline {
        // --- Bone timelines -------------------------------------------
        Timeline::Rotate { bone, .. } => vec![id(Property::Rotate, u32::from(bone.0))],
        Timeline::Translate { bone, .. } => vec![
            id(Property::X, u32::from(bone.0)),
            id(Property::Y, u32::from(bone.0)),
        ],
        Timeline::TranslateX { bone, .. } => vec![id(Property::X, u32::from(bone.0))],
        Timeline::TranslateY { bone, .. } => vec![id(Property::Y, u32::from(bone.0))],
        Timeline::Scale { bone, .. } => vec![
            id(Property::ScaleX, u32::from(bone.0)),
            id(Property::ScaleY, u32::from(bone.0)),
        ],
        Timeline::ScaleX { bone, .. } => vec![id(Property::ScaleX, u32::from(bone.0))],
        Timeline::ScaleY { bone, .. } => vec![id(Property::ScaleY, u32::from(bone.0))],
        Timeline::Shear { bone, .. } => vec![
            id(Property::ShearX, u32::from(bone.0)),
            id(Property::ShearY, u32::from(bone.0)),
        ],
        Timeline::ShearX { bone, .. } => vec![id(Property::ShearX, u32::from(bone.0))],
        Timeline::ShearY { bone, .. } => vec![id(Property::ShearY, u32::from(bone.0))],
        Timeline::Inherit { bone, .. } => vec![id(Property::Inherit, u32::from(bone.0))],

        // --- Slot timelines -------------------------------------------
        // RGBA writes both Rgb and Alpha; RGB only Rgb; Alpha only Alpha.
        // RGBA2 writes Rgb, Alpha, Rgb2; RGB2 writes Rgb + Rgb2.
        Timeline::Rgba { slot, .. } => vec![
            id(Property::Rgb, u32::from(slot.0)),
            id(Property::Alpha, u32::from(slot.0)),
        ],
        Timeline::Rgb { slot, .. } => vec![id(Property::Rgb, u32::from(slot.0))],
        Timeline::Alpha { slot, .. } => vec![id(Property::Alpha, u32::from(slot.0))],
        Timeline::Rgba2 { slot, .. } => vec![
            id(Property::Rgb, u32::from(slot.0)),
            id(Property::Alpha, u32::from(slot.0)),
            id(Property::Rgb2, u32::from(slot.0)),
        ],
        Timeline::Rgb2 { slot, .. } => vec![
            id(Property::Rgb, u32::from(slot.0)),
            id(Property::Rgb2, u32::from(slot.0)),
        ],
        Timeline::Attachment { slot, .. } => vec![id(Property::Attachment, u32::from(slot.0))],
        Timeline::Deform {
            slot, attachment, ..
        } => {
            let payload: u32 = (u32::from(slot.0) << 16) | (attachment.0 & 0xffff);
            vec![PropertyId(
                (Property::Deform as i64) << 32 | i64::from(payload),
            )]
        }
        // Sequence doesn't expose its slot index directly here in the
        // enum shape; spine-cpp keys by slot. Matching that convention.
        Timeline::Sequence { slot, .. } => vec![id(Property::Sequence, u32::from(slot.0))],

        // --- Skeleton-wide timelines ----------------------------------
        Timeline::DrawOrder { .. } => vec![id(Property::DrawOrder, 0)],
        Timeline::Event { .. } => vec![id(Property::Event, 0)],

        // --- Constraints ----------------------------------------------
        Timeline::IkConstraint { constraint, .. } => {
            vec![id(Property::IkConstraint, u32::from(constraint.0))]
        }
        Timeline::TransformConstraint { constraint, .. } => {
            vec![id(Property::TransformConstraint, u32::from(constraint.0))]
        }
        Timeline::PathConstraintPosition { constraint, .. } => {
            vec![id(
                Property::PathConstraintPosition,
                u32::from(constraint.0),
            )]
        }
        Timeline::PathConstraintSpacing { constraint, .. } => {
            vec![id(Property::PathConstraintSpacing, u32::from(constraint.0))]
        }
        Timeline::PathConstraintMix { constraint, .. } => {
            vec![id(Property::PathConstraintMix, u32::from(constraint.0))]
        }
        Timeline::Physics {
            constraint,
            property,
            ..
        } => {
            let kind = physics_property_kind(*property);
            // constraint = None means "all physics constraints with the
            // matching `_global` flag"; spine-cpp encodes that as index -1,
            // which casts to u32 = 0xffffffff.
            let payload = constraint.map_or(u32::MAX, |c| u32::from(c.0));
            vec![id(kind, payload)]
        }
        Timeline::PhysicsReset { constraint, .. } => {
            let payload = constraint.map_or(u32::MAX, |c| u32::from(c.0));
            vec![id(Property::PhysicsConstraintReset, payload)]
        }
    }
}

/// `true` if the animation has any timeline writing one of `ids`.
/// Matches `spine::Animation::hasTimeline`.
#[must_use]
pub fn animation_has_timeline(animation: &crate::data::Animation, ids: &[PropertyId]) -> bool {
    animation
        .timelines
        .iter()
        .any(|tl| property_ids(tl).iter().any(|pid| ids.contains(pid)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BoneId, CurveFrames, IkConstraintId, SlotId};

    #[test]
    fn rotate_timeline_encodes_property_rotate_and_bone() {
        let tl = Timeline::Rotate {
            bone: BoneId(3),
            curves: CurveFrames::default(),
        };
        assert_eq!(
            property_ids(&tl),
            vec![PropertyId((Property::Rotate as i64) << 32 | 3)]
        );
    }

    #[test]
    fn rgba_timeline_emits_rgb_and_alpha() {
        let tl = Timeline::Rgba {
            slot: SlotId(7),
            curves: CurveFrames::default(),
        };
        let ids = property_ids(&tl);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&PropertyId((Property::Rgb as i64) << 32 | 7)));
        assert!(ids.contains(&PropertyId((Property::Alpha as i64) << 32 | 7)));
    }

    #[test]
    fn ik_constraint_timeline_encodes_index() {
        let tl = Timeline::IkConstraint {
            constraint: IkConstraintId(5),
            curves: CurveFrames::default(),
        };
        assert_eq!(
            property_ids(&tl),
            vec![PropertyId((Property::IkConstraint as i64) << 32 | 5)]
        );
    }
}
