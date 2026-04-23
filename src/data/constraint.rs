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

//! Setup-pose data for the four constraint kinds: IK, Transform, Path,
//! Physics. The actual solvers land in Phase 5.

use crate::data::{
    BoneId, IkConstraintId, PathConstraintId, PhysicsConstraintId, SlotId, TransformConstraintId,
};

// --- PathConstraint modes ---------------------------------------------------

/// How a path constraint positions bones along its target path.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PositionMode {
    Fixed,
    #[default]
    Percent,
}

/// How spacing between consecutive constrained bones is interpreted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SpacingMode {
    #[default]
    Length,
    Fixed,
    Percent,
    /// New in 4.1: spacing scales proportionally with the path length.
    Proportional,
}

/// How a path constraint rotates its constrained bones.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RotateMode {
    #[default]
    Tangent,
    Chain,
    ChainScale,
}

// --- IkConstraintData -------------------------------------------------------

/// Setup-pose data for an IK constraint: pulls one or two bones toward a
/// target bone.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // compress/stretch/uniform are domain flags on the IK solver.
pub struct IkConstraintData {
    pub index: IkConstraintId,
    pub name: String,
    /// Update order within the combined bone + constraint cache. Lower runs
    /// earlier.
    pub order: u32,
    pub skin_required: bool,

    /// Bones constrained by this IK (1 for single-bone, 2 for two-bone IK).
    pub bones: Vec<BoneId>,
    pub target: BoneId,

    /// +1 or -1. Controls which elbow direction the two-bone solver prefers.
    pub bend_direction: i8,
    pub compress: bool,
    pub stretch: bool,
    pub uniform: bool,

    pub mix: f32,
    pub softness: f32,
}

impl IkConstraintData {
    /// Defaults match `spine-cpp` `IkConstraintData` constructor. The
    /// binary loader always writes `bend_direction` and `mix` explicitly
    /// (see `SkeletonBinary.cpp`), so those zeros are only visible to
    /// code that builds a `IkConstraintData` directly (tests, future JSON).
    #[must_use]
    pub fn new(index: IkConstraintId, name: impl Into<String>, target: BoneId) -> Self {
        Self {
            index,
            name: name.into(),
            order: 0,
            skin_required: false,
            bones: Vec::new(),
            target,
            bend_direction: 0,
            compress: false,
            stretch: false,
            uniform: false,
            mix: 0.0,
            softness: 0.0,
        }
    }
}

// --- TransformConstraintData -----------------------------------------------

/// Setup-pose data for a transform constraint: copies some combination of
/// translation / rotation / scale / shear from a target bone onto one or
/// more constrained bones, with configurable offsets and per-axis mix.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformConstraintData {
    pub index: TransformConstraintId,
    pub name: String,
    pub order: u32,
    pub skin_required: bool,

    pub bones: Vec<BoneId>,
    pub target: BoneId,

    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub mix_scale_x: f32,
    pub mix_scale_y: f32,
    pub mix_shear_y: f32,

    pub offset_rotation: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub offset_scale_x: f32,
    pub offset_scale_y: f32,
    pub offset_shear_y: f32,

    pub relative: bool,
    pub local: bool,
}

impl TransformConstraintData {
    /// Defaults match `spine-cpp` `TransformConstraintData` constructor:
    /// all mixes and offsets are zero. The binary loader only reads a mix
    /// value when its flag bit is set, so these zeros are the
    /// authoritative "field not serialized" value — a non-zero default
    /// here silently activates constraints that were meant to stay off
    /// until an animation timeline ramps their mix up.
    #[must_use]
    pub fn new(index: TransformConstraintId, name: impl Into<String>, target: BoneId) -> Self {
        Self {
            index,
            name: name.into(),
            order: 0,
            skin_required: false,
            bones: Vec::new(),
            target,
            mix_rotate: 0.0,
            mix_x: 0.0,
            mix_y: 0.0,
            mix_scale_x: 0.0,
            mix_scale_y: 0.0,
            mix_shear_y: 0.0,
            offset_rotation: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            offset_scale_x: 0.0,
            offset_scale_y: 0.0,
            offset_shear_y: 0.0,
            relative: false,
            local: false,
        }
    }
}

// --- PathConstraintData -----------------------------------------------------

/// Setup-pose data for a path constraint: places constrained bones along a
/// [`PathAttachment`][crate::data::attachment::PathAttachment] carried by the
/// target slot.
#[derive(Debug, Clone, PartialEq)]
pub struct PathConstraintData {
    pub index: PathConstraintId,
    pub name: String,
    pub order: u32,
    pub skin_required: bool,

    pub bones: Vec<BoneId>,
    /// Slot whose active attachment must be a `PathAttachment`.
    pub target: SlotId,

    pub position_mode: PositionMode,
    pub spacing_mode: SpacingMode,
    pub rotate_mode: RotateMode,

    pub offset_rotation: f32,
    pub position: f32,
    pub spacing: f32,

    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
}

impl PathConstraintData {
    /// Defaults match `spine-cpp` `PathConstraintData` constructor. The
    /// binary loader always writes `position_mode`, `mix_rotate`,
    /// `mix_x`, and `mix_y` explicitly, so those defaults are only
    /// visible to tests / future JSON.
    #[must_use]
    pub fn new(index: PathConstraintId, name: impl Into<String>, target: SlotId) -> Self {
        Self {
            index,
            name: name.into(),
            order: 0,
            skin_required: false,
            bones: Vec::new(),
            target,
            position_mode: PositionMode::Fixed,
            spacing_mode: SpacingMode::Length,
            rotate_mode: RotateMode::Tangent,
            offset_rotation: 0.0,
            position: 0.0,
            spacing: 0.0,
            mix_rotate: 0.0,
            mix_x: 0.0,
            mix_y: 0.0,
        }
    }
}

// --- PhysicsConstraintData --------------------------------------------------

/// Setup-pose data for a physics constraint (new in Spine 4.2). Simulates
/// damped inertia on one bone with configurable gravity and wind.
///
/// "Global" booleans on each dynamic parameter mean the parameter is sampled
/// from a skeleton-wide setting rather than from this constraint's own value.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Seven bools, one per simulation parameter — domain model.
pub struct PhysicsConstraintData {
    pub index: PhysicsConstraintId,
    pub name: String,
    pub order: u32,
    pub skin_required: bool,

    pub bone: BoneId,

    // Mix fractions per channel.
    pub x: f32,
    pub y: f32,
    pub rotate: f32,
    pub scale_x: f32,
    pub shear_x: f32,
    pub limit: f32,

    // Simulation parameters.
    pub step: f32,
    pub inertia: f32,
    pub strength: f32,
    pub damping: f32,
    pub mass_inverse: f32,
    pub wind: f32,
    pub gravity: f32,
    pub mix: f32,

    pub inertia_global: bool,
    pub strength_global: bool,
    pub damping_global: bool,
    pub mass_global: bool,
    pub wind_global: bool,
    pub gravity_global: bool,
    pub mix_global: bool,
}

impl PhysicsConstraintData {
    /// Defaults match `spine-cpp` `PhysicsConstraintData` constructor
    /// (all zero). The binary loader always writes the simulation
    /// parameters (`step`, `inertia`, `strength`, `damping`, `wind`,
    /// `gravity`) and has explicit fallbacks for `limit` (5000),
    /// `mass_inverse` (1), and `mix` (1), so these zeros are only
    /// visible to tests / future JSON.
    #[must_use]
    pub fn new(index: PhysicsConstraintId, name: impl Into<String>, bone: BoneId) -> Self {
        Self {
            index,
            name: name.into(),
            order: 0,
            skin_required: false,
            bone,
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            scale_x: 0.0,
            shear_x: 0.0,
            limit: 0.0,
            step: 0.0,
            inertia: 0.0,
            strength: 0.0,
            damping: 0.0,
            mass_inverse: 0.0,
            wind: 0.0,
            gravity: 0.0,
            mix: 0.0,
            inertia_global: false,
            strength_global: false,
            damping_global: false,
            mass_global: false,
            wind_global: false,
            gravity_global: false,
            mix_global: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Literal default comparisons only.
mod tests {
    use super::*;

    #[test]
    fn ik_defaults_match_spine_cpp() {
        let ik = IkConstraintData::new(IkConstraintId(0), "ik", BoneId(1));
        assert_eq!(ik.mix, 0.0);
        assert_eq!(ik.bend_direction, 0);
        assert_eq!(ik.softness, 0.0);
    }

    #[test]
    fn transform_defaults_match_spine_cpp() {
        let tc = TransformConstraintData::new(TransformConstraintId(0), "tc", BoneId(1));
        assert_eq!(tc.mix_rotate, 0.0);
        assert_eq!(tc.mix_shear_y, 0.0);
        assert!(!tc.relative);
    }

    #[test]
    fn path_defaults_match_spine_cpp() {
        let pc = PathConstraintData::new(PathConstraintId(0), "pc", SlotId(2));
        assert_eq!(pc.position_mode, PositionMode::Fixed);
        assert_eq!(pc.spacing_mode, SpacingMode::Length);
        assert_eq!(pc.rotate_mode, RotateMode::Tangent);
        assert_eq!(pc.mix_rotate, 0.0);
    }

    #[test]
    fn physics_defaults_match_spine_cpp() {
        let ph = PhysicsConstraintData::new(PhysicsConstraintId(0), "ph", BoneId(3));
        assert_eq!(ph.step, 0.0);
        assert_eq!(ph.mix, 0.0);
        assert_eq!(ph.strength, 0.0);
        assert_eq!(ph.limit, 0.0);
    }
}
