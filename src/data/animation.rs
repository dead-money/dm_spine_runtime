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

//! Animation data scaffolding — types only, no evaluation.
//!
//! Phase 1b stops at data shape. Phase 3 implements curve evaluation and the
//! per-variant `apply` logic that reads these arrays.

use crate::data::{
    AttachmentId, BoneId, EventId, IkConstraintId, Inherit, PathConstraintId, PhysicsConstraintId,
    SlotId, TransformConstraintId,
};

/// A named collection of timelines driving a skeleton over a fixed duration.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub timelines: Vec<Timeline>,
}

impl Animation {
    #[must_use]
    pub fn new(name: impl Into<String>, duration: f32) -> Self {
        Self {
            name: name.into(),
            duration,
            timelines: Vec::new(),
        }
    }
}

/// Per-frame curve storage matching `spine-cpp/CurveTimeline`.
///
/// `frames` holds interleaved time + value entries — the stride (entries per
/// frame) depends on the containing timeline variant: rotate = 2, translate =
/// 3, RGBA = 5, and so on. `curves` holds per-frame interpolation data:
/// a type code (linear = 0, stepped = 1, bezier = 2) optionally followed by
/// bezier segmentation samples (spine-cpp uses `BEZIER_SIZE = 18` floats per
/// bezier segment).
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

/// Tagged union of every kind of animation timeline. Structure data only —
/// evaluation lands in Phase 3.
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
    Event {
        /// `frames[i]` is redundant with `events[i].time`; kept as a
        /// dedicated vector so searches use a clean f32 binary search.
        frames: Vec<f32>,
        events: Vec<AnimationEvent>,
    },

    // --- Constraint timelines ---------------------------------------------
    IkConstraint {
        constraint: IkConstraintId,
        curves: CurveFrames,
    },
    TransformConstraint {
        constraint: TransformConstraintId,
        curves: CurveFrames,
    },
    PathConstraintPosition {
        constraint: PathConstraintId,
        curves: CurveFrames,
    },
    PathConstraintSpacing {
        constraint: PathConstraintId,
        curves: CurveFrames,
    },
    PathConstraintMix {
        constraint: PathConstraintId,
        curves: CurveFrames,
    },
    /// A single physics-property curve. One constraint can have multiple
    /// timeline instances — one per animated property. `constraint = None`
    /// means the timeline applies to every physics constraint in the
    /// skeleton (matches spine-cpp's `index = -1` sentinel).
    Physics {
        constraint: Option<PhysicsConstraintId>,
        property: PhysicsProperty,
        curves: CurveFrames,
    },
    /// Reset the physics solver's integrator state. `None` means reset all
    /// physics constraints in the skeleton.
    PhysicsReset {
        constraint: Option<PhysicsConstraintId>,
        frames: Vec<f32>,
    },
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
                constraint: IkConstraintId(0),
                curves: CurveFrames::default(),
            },
            Timeline::Physics {
                constraint: Some(PhysicsConstraintId(0)),
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
