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

//! Setup-pose data for the five constraint kinds. 4.3 keeps them in one
//! ordered list: list position is update order, and skins and timelines
//! index into it with [`ConstraintId`][crate::data::ConstraintId].

use crate::data::{AnimationId, BoneId, SlotId};

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
    #[default]
    None,
    Uniform,
    Volume,
}

impl ScaleYMode {
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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct IkConstraintPose {
    pub bend_direction: i32,
    pub compress: bool,
    pub stretch: bool,
    pub mix: f32,
    pub softness: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IkConstraintData {
    pub name: String,
    pub skin_required: bool,
    pub bones: Vec<BoneId>,
    pub target: BoneId,
    pub scale_y_mode: ScaleYMode,
    pub setup: IkConstraintPose,
}

impl IkConstraintData {
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

/// A bone transform channel a transform constraint reads from its source
/// or writes to its targets. Discriminants are the wire values.
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

#[derive(Debug, Clone, PartialEq)]
pub struct FromProperty {
    pub property: TransformProperty,
    pub offset: f32,
    pub to: Vec<ToProperty>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToProperty {
    pub property: TransformProperty,
    pub offset: f32,
    pub max: f32,
    pub scale: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TransformConstraintPose {
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub mix_scale_x: f32,
    pub mix_scale_y: f32,
    pub mix_shear_y: f32,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // wire flags
pub struct TransformConstraintData {
    pub name: String,
    pub skin_required: bool,
    pub bones: Vec<BoneId>,
    pub source: BoneId,
    /// Indexed by [`TransformProperty::offset_index`].
    pub offsets: [f32; 6],
    pub local_source: bool,
    pub local_target: bool,
    pub additive: bool,
    pub clamp: bool,
    pub properties: Vec<FromProperty>,
    pub setup: TransformConstraintPose,
}

impl TransformConstraintData {
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PositionMode {
    #[default]
    Fixed,
    Percent,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SpacingMode {
    #[default]
    Length,
    Fixed,
    Percent,
    Proportional,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RotateMode {
    #[default]
    Tangent,
    Chain,
    ChainScale,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PathConstraintPose {
    pub position: f32,
    pub spacing: f32,
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathConstraintData {
    pub name: String,
    pub skin_required: bool,
    pub bones: Vec<BoneId>,
    pub slot: SlotId,
    pub position_mode: PositionMode,
    pub spacing_mode: SpacingMode,
    pub rotate_mode: RotateMode,
    pub offset_rotation: f32,
    pub setup: PathConstraintPose,
}

impl PathConstraintData {
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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PhysicsConstraintPose {
    pub inertia: f32,
    pub strength: f32,
    pub damping: f32,
    pub mass_inverse: f32,
    pub wind: f32,
    pub gravity: f32,
    pub mix: f32,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // one flag per global parameter
pub struct PhysicsConstraintData {
    pub name: String,
    pub skin_required: bool,
    pub bone: BoneId,
    pub x: f32,
    pub y: f32,
    pub rotate: f32,
    pub scale_x: f32,
    pub shear_x: f32,
    pub limit: f32,
    pub step: f32,
    pub scale_y_mode: ScaleYMode,
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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SliderPose {
    pub time: f32,
    pub mix: f32,
}

/// Bone channel that drives a slider's time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderProperty {
    pub property: TransformProperty,
    pub offset: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SliderData {
    pub name: String,
    pub skin_required: bool,
    /// Set after the animations are loaded.
    pub animation: Option<AnimationId>,
    pub additive: bool,
    pub looping: bool,
    pub bone: Option<BoneId>,
    pub property: Option<SliderProperty>,
    pub scale: f32,
    pub offset: f32,
    pub local: bool,
    pub max: f32,
    pub setup: SliderPose,
}

impl SliderData {
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
