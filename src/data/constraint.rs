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

//! Setup-pose data for the five constraint kinds: IK, transform, path,
//! physics and slider. They share one ordered list,
//! [`SkeletonData::constraints`](crate::data::SkeletonData::constraints),
//! whose order is update order; skins and timelines index into it with
//! [`ConstraintId`](crate::data::ConstraintId).
//!
//! Each `*Data` type holds the fixed configuration plus a `setup` pose. The
//! pose is the part timelines animate; a skeleton keeps its own copy.
//! A mix blends from the unconstrained (0) to the constrained (1) pose.

use crate::data::{AnimationId, BoneId, SlotId};

/// One constraint of any kind.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintData {
    Ik(IkConstraintData),
    Transform(TransformConstraintData),
    Path(PathConstraintData),
    Physics(PhysicsConstraintData),
    Slider(SliderData),
}

impl ConstraintData {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Ik(c) => &c.name,
            Self::Transform(c) => &c.name,
            Self::Path(c) => &c.name,
            Self::Physics(c) => &c.name,
            Self::Slider(c) => &c.name,
        }
    }

    /// Whether the constraint is active only while a skin listing it is
    /// worn.
    #[must_use]
    pub fn skin_required(&self) -> bool {
        match self {
            Self::Ik(c) => c.skin_required,
            Self::Transform(c) => c.skin_required,
            Self::Path(c) => c.skin_required,
            Self::Physics(c) => c.skin_required,
            Self::Slider(c) => c.skin_required,
        }
    }
}

/// How a constraint that stretches a bone's X scale treats its Y scale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScaleYMode {
    /// Y scale is left alone.
    #[default]
    None,
    /// Y scales by the same factor as X.
    Uniform,
    /// Y scales inversely to X, roughly preserving area.
    Volume,
}

impl ScaleYMode {
    /// Decodes the wire value; `None` if out of range.
    #[must_use]
    pub fn from_index(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::None,
            1 => Self::Uniform,
            2 => Self::Volume,
            _ => return None,
        })
    }
}

/// Animatable state of an IK constraint.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct IkConstraintPose {
    /// Two-bone IK: which way the joint bends, `1` or `-1`.
    pub bend_direction: i32,
    /// One-bone IK: scale the bone down when the target is too close.
    pub compress: bool,
    /// Scale the bones up when the target is out of reach.
    pub stretch: bool,
    /// Rotation mix, 0..1.
    pub mix: f32,
    /// Two-bone IK: distance from full extension, in skeleton units, over
    /// which the bones slow down instead of snapping straight.
    pub softness: f32,
}

/// Rotates one or two bones so the last one points at `target`.
#[derive(Debug, Clone, PartialEq)]
pub struct IkConstraintData {
    pub name: String,
    pub skin_required: bool,
    /// The constrained parent and, for two-bone IK, its child.
    pub bones: Vec<BoneId>,
    pub target: BoneId,
    /// What compress and stretch do to Y scale.
    pub scale_y_mode: ScaleYMode,
    pub setup: IkConstraintPose,
}

impl IkConstraintData {
    /// No bones, default pose.
    #[must_use]
    pub fn new(name: impl Into<String>, target: BoneId) -> Self {
        Self {
            name: name.into(),
            skin_required: false,
            bones: Vec::new(),
            target,
            scale_y_mode: ScaleYMode::None,
            setup: IkConstraintPose::default(),
        }
    }
}

/// A bone transform channel that a transform constraint reads or writes, or
/// that drives a slider. Discriminants are the wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformProperty {
    Rotate = 0,
    X = 1,
    Y = 2,
    ScaleX = 3,
    ScaleY = 4,
    ShearY = 5,
}

impl TransformProperty {
    /// Decodes the wire value; `None` if out of range.
    #[must_use]
    pub fn from_index(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Rotate,
            1 => Self::X,
            2 => Self::Y,
            3 => Self::ScaleX,
            4 => Self::ScaleY,
            5 => Self::ShearY,
            _ => return None,
        })
    }

    /// Index into [`TransformConstraintData::offsets`].
    #[must_use]
    pub fn offset_index(self) -> usize {
        self as usize
    }
}

/// A source channel a transform constraint reads, and the target channels
/// it maps onto.
#[derive(Debug, Clone, PartialEq)]
pub struct FromProperty {
    pub property: TransformProperty,
    /// Source value that maps to each [`ToProperty::offset`].
    pub offset: f32,
    pub to: Vec<ToProperty>,
}

/// A target channel: `to.offset + (source - from.offset) * to.scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToProperty {
    pub property: TransformProperty,
    /// Target value when the source equals [`FromProperty::offset`].
    pub offset: f32,
    /// Upper bound when [`TransformConstraintData::clamp`] is set.
    pub max: f32,
    /// Target change per unit of source change.
    pub scale: f32,
}

/// Animatable mixes of a transform constraint, one per channel, each 0..1.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TransformConstraintPose {
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub mix_scale_x: f32,
    pub mix_scale_y: f32,
    pub mix_shear_y: f32,
}

/// Drives the `bones`' transforms from `source`'s transform.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // wire flags
pub struct TransformConstraintData {
    pub name: String,
    pub skin_required: bool,
    /// The constrained bones.
    pub bones: Vec<BoneId>,
    pub source: BoneId,
    /// Offsets added to the source values as they are read, indexed by
    /// [`TransformProperty::offset_index`]. Rotation is in degrees.
    pub offsets: [f32; 6],
    /// Read the source's local transform instead of its world transform.
    pub local_source: bool,
    /// Write the bones' local transforms instead of their world transforms.
    pub local_target: bool,
    /// Add to the bones' transforms instead of replacing them.
    pub additive: bool,
    /// Keep each target value between [`ToProperty::offset`] and
    /// [`ToProperty::max`].
    pub clamp: bool,
    pub properties: Vec<FromProperty>,
    pub setup: TransformConstraintPose,
}

impl TransformConstraintData {
    /// No bones or properties, default pose.
    #[must_use]
    pub fn new(name: impl Into<String>, source: BoneId) -> Self {
        Self {
            name: name.into(),
            skin_required: false,
            bones: Vec::new(),
            source,
            offsets: [0.0; 6],
            local_source: false,
            local_target: false,
            additive: false,
            clamp: false,
            properties: Vec::new(),
            setup: TransformConstraintPose::default(),
        }
    }
}

/// How [`PathConstraintPose::position`] is measured.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PositionMode {
    /// Skeleton units along the path.
    #[default]
    Fixed,
    /// Fraction of the path length, 0..1.
    Percent,
}

/// How [`PathConstraintPose::spacing`] separates the bones along the path.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SpacingMode {
    /// Each bone's length plus the spacing, in skeleton units.
    #[default]
    Length,
    /// The spacing alone, in skeleton units.
    Fixed,
    /// Fraction of the path length.
    Percent,
    /// Fraction of the path length, split between bones in proportion to
    /// their lengths.
    Proportional,
}

/// How bones are rotated to follow the path.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RotateMode {
    /// Along the path's tangent.
    #[default]
    Tangent,
    /// Toward the next bone's position.
    Chain,
    /// As `Chain`, and scaled in X to reach the next bone.
    ChainScale,
}

/// Animatable state of a path constraint.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PathConstraintPose {
    /// Where the first bone sits; units per [`PositionMode`].
    pub position: f32,
    /// Distance between bones; units per [`SpacingMode`].
    pub spacing: f32,
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
}

/// Moves `bones` along the [`PathAttachment`](crate::data::PathAttachment)
/// shown by `slot`.
#[derive(Debug, Clone, PartialEq)]
pub struct PathConstraintData {
    pub name: String,
    pub skin_required: bool,
    /// The constrained bones, in order along the path.
    pub bones: Vec<BoneId>,
    pub slot: SlotId,
    pub position_mode: PositionMode,
    pub spacing_mode: SpacingMode,
    pub rotate_mode: RotateMode,
    /// Degrees added to each bone's rotation.
    pub offset_rotation: f32,
    pub setup: PathConstraintPose,
}

impl PathConstraintData {
    /// No bones, default modes and pose.
    #[must_use]
    pub fn new(name: impl Into<String>, slot: SlotId) -> Self {
        Self {
            name: name.into(),
            skin_required: false,
            bones: Vec::new(),
            slot,
            position_mode: PositionMode::Fixed,
            spacing_mode: SpacingMode::Length,
            rotate_mode: RotateMode::Tangent,
            offset_rotation: 0.0,
            setup: PathConstraintPose::default(),
        }
    }
}

/// Animatable state of a physics constraint.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PhysicsConstraintPose {
    /// How much bone movement becomes physics movement.
    pub inertia: f32,
    /// Spring force pulling back to the unconstrained pose.
    pub strength: f32,
    /// Fraction of velocity kept per 1/60 second; 1 is no damping.
    pub damping: f32,
    /// `1 / mass`.
    pub mass_inverse: f32,
    /// Force along the skeleton's wind vector.
    pub wind: f32,
    /// Force along the skeleton's gravity vector.
    pub gravity: f32,
    /// Blend from the unconstrained (0) to the simulated (1) pose.
    pub mix: f32,
}

/// Simulates spring physics on one bone.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // one flag per global parameter
pub struct PhysicsConstraintData {
    pub name: String,
    pub skin_required: bool,
    pub bone: BoneId,
    /// Physics influence on each channel, 0..1.
    pub x: f32,
    pub y: f32,
    pub rotate: f32,
    pub scale_x: f32,
    pub shear_x: f32,
    /// Movement faster than this, in skeleton units per second, adds no
    /// more to the simulation.
    pub limit: f32,
    /// Simulation step, in seconds.
    pub step: f32,
    pub scale_y_mode: ScaleYMode,
    /// The `*_global` flags mark parameters that the global physics
    /// timelines ([`Timeline::Physics`](crate::data::Timeline::Physics) with
    /// no constraint) drive.
    pub inertia_global: bool,
    pub strength_global: bool,
    pub damping_global: bool,
    pub mass_global: bool,
    pub wind_global: bool,
    pub gravity_global: bool,
    pub mix_global: bool,
    pub setup: PhysicsConstraintPose,
}

impl PhysicsConstraintData {
    /// Zero influence on every channel, default pose.
    #[must_use]
    pub fn new(name: impl Into<String>, bone: BoneId) -> Self {
        Self {
            name: name.into(),
            skin_required: false,
            bone,
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            scale_x: 0.0,
            shear_x: 0.0,
            limit: 0.0,
            step: 0.0,
            scale_y_mode: ScaleYMode::None,
            inertia_global: false,
            strength_global: false,
            damping_global: false,
            mass_global: false,
            wind_global: false,
            gravity_global: false,
            mix_global: false,
            setup: PhysicsConstraintPose::default(),
        }
    }
}

/// Animatable state of a slider.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SliderPose {
    /// Seconds into [`SliderData::animation`].
    pub time: f32,
    /// Blend from the unconstrained (0) to the animated (1) pose.
    pub mix: f32,
}

/// Bone channel that drives a slider's time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderProperty {
    pub property: TransformProperty,
    /// Subtracted from the channel value before scaling.
    pub offset: f32,
}

/// Applies an animation at a time set by its pose or by a bone. With a
/// bone, time is `offset + (value - property.offset) * scale`.
#[derive(Debug, Clone, PartialEq)]
pub struct SliderData {
    pub name: String,
    pub skin_required: bool,
    /// Set after the animations are loaded.
    pub animation: Option<AnimationId>,
    /// Add the animation to the current pose instead of replacing it.
    pub additive: bool,
    /// Repeat the animation past its duration; otherwise hold the last
    /// frame.
    pub looping: bool,
    /// Bone whose channel drives the time, if any.
    pub bone: Option<BoneId>,
    pub property: Option<SliderProperty>,
    pub scale: f32,
    pub offset: f32,
    /// Read the bone's local transform instead of its world transform.
    pub local: bool,
    /// Nonessential: upper end of the editor's slider range; 0 if not
    /// exported.
    pub max: f32,
    pub setup: SliderPose,
}

impl SliderData {
    /// No animation or bone, default pose.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            skin_required: false,
            animation: None,
            additive: false,
            looping: false,
            bone: None,
            property: None,
            scale: 0.0,
            offset: 0.0,
            local: false,
            max: 0.0,
            setup: SliderPose::default(),
        }
    }
}

/// Defines `as_*`, which returns the inner data if `self` is that kind.
macro_rules! constraint_accessor {
    ($fn:ident, $variant:ident, $ty:ty) => {
        /// The inner data if the constraint is this kind.
        #[must_use]
        pub fn $fn(&self) -> Option<&$ty> {
            match self {
                Self::$variant(c) => Some(c),
                _ => None,
            }
        }
    };
}

impl ConstraintData {
    constraint_accessor!(as_ik, Ik, IkConstraintData);
    constraint_accessor!(as_transform, Transform, TransformConstraintData);
    constraint_accessor!(as_path, Path, PathConstraintData);
    constraint_accessor!(as_physics, Physics, PhysicsConstraintData);
    constraint_accessor!(as_slider, Slider, SliderData);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_property_wire_values_round_trip() {
        for v in 0..6 {
            assert_eq!(TransformProperty::from_index(v).unwrap() as i32, v);
        }
        assert!(TransformProperty::from_index(6).is_none());
    }

    #[test]
    fn unified_accessors_select_variant() {
        let c = ConstraintData::Ik(IkConstraintData::new("ik", BoneId(0)));
        assert_eq!(c.name(), "ik");
        assert!(c.as_ik().is_some());
        assert!(c.as_path().is_none());
    }
}
