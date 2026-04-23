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

//! Runtime-mutable constraint instances: IK, Transform, Path, Physics.
//!
//! Each struct mirrors the private fields of the matching class in
//! `spine-cpp/include/spine/{Ik,Transform,Path,Physics}Constraint.h`.

use crate::data::{
    BoneId, IkConstraintData, IkConstraintId, PathConstraintData, PathConstraintId,
    PhysicsConstraintData, PhysicsConstraintId, SlotId, TransformConstraintData,
    TransformConstraintId,
};

// --- IkConstraint ----------------------------------------------------------

/// Runtime-mutable IK constraint. Animations overwrite `mix`, `softness`, and
/// `bend_direction`; the solver reads them to pose `bones` toward
/// `target`.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // domain flags, not API flags
pub struct IkConstraint {
    pub data_index: IkConstraintId,
    pub bones: Vec<BoneId>,
    pub target: BoneId,

    pub mix: f32,
    pub softness: f32,
    pub bend_direction: i8,
    pub compress: bool,
    pub stretch: bool,

    pub active: bool,
}

impl IkConstraint {
    #[must_use]
    pub fn new(data: &IkConstraintData) -> Self {
        Self {
            data_index: data.index,
            bones: data.bones.clone(),
            target: data.target,
            mix: data.mix,
            softness: data.softness,
            bend_direction: data.bend_direction,
            compress: data.compress,
            stretch: data.stretch,
            active: false,
        }
    }

    /// No-op placeholder. Solving happens inside
    /// `Skeleton::update_world_transform`; see `spine::IkConstraint::update`.
    pub fn update(&mut self) {}

    /// Port of `spine::IkConstraint::setToSetupPose`. Resets every
    /// animation-mutable field to `data`'s setup value.
    pub fn set_to_setup_pose(&mut self, data: &IkConstraintData) {
        debug_assert_eq!(data.index, self.data_index);
        self.mix = data.mix;
        self.softness = data.softness;
        self.bend_direction = data.bend_direction;
        self.compress = data.compress;
        self.stretch = data.stretch;
    }
}

// --- TransformConstraint ----------------------------------------------------

/// Runtime-mutable transform constraint. Copies TRS from `target` to `bones`
/// scaled by the per-axis mix values.
#[derive(Debug, Clone)]
pub struct TransformConstraint {
    pub data_index: TransformConstraintId,
    pub bones: Vec<BoneId>,
    pub target: BoneId,

    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub mix_scale_x: f32,
    pub mix_scale_y: f32,
    pub mix_shear_y: f32,

    pub active: bool,
}

impl TransformConstraint {
    #[must_use]
    pub fn new(data: &TransformConstraintData) -> Self {
        Self {
            data_index: data.index,
            bones: data.bones.clone(),
            target: data.target,
            mix_rotate: data.mix_rotate,
            mix_x: data.mix_x,
            mix_y: data.mix_y,
            mix_scale_x: data.mix_scale_x,
            mix_scale_y: data.mix_scale_y,
            mix_shear_y: data.mix_shear_y,
            active: false,
        }
    }

    pub fn update(&mut self) {}

    /// Port of `spine::TransformConstraint::setToSetupPose`.
    pub fn set_to_setup_pose(&mut self, data: &TransformConstraintData) {
        debug_assert_eq!(data.index, self.data_index);
        self.mix_rotate = data.mix_rotate;
        self.mix_x = data.mix_x;
        self.mix_y = data.mix_y;
        self.mix_scale_x = data.mix_scale_x;
        self.mix_scale_y = data.mix_scale_y;
        self.mix_shear_y = data.mix_shear_y;
    }
}

// --- PathConstraint ---------------------------------------------------------

/// Runtime-mutable path constraint. Arranges `bones` along the path attachment
/// held by the `target` slot.
#[derive(Debug, Clone)]
pub struct PathConstraint {
    pub data_index: PathConstraintId,
    pub bones: Vec<BoneId>,
    /// Slot whose current attachment must be a `PathAttachment`.
    pub target: SlotId,

    pub position: f32,
    pub spacing: f32,

    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,

    // Scratch buffers used by the solver. Kept on the instance so repeated
    // frames don't re-allocate.
    pub positions: Vec<f32>,
    pub world: Vec<f32>,
    pub lengths: Vec<f32>,
    pub segments: Vec<f32>,

    pub active: bool,
}

impl PathConstraint {
    #[must_use]
    pub fn new(data: &PathConstraintData) -> Self {
        Self {
            data_index: data.index,
            bones: data.bones.clone(),
            target: data.target,
            position: data.position,
            spacing: data.spacing,
            mix_rotate: data.mix_rotate,
            mix_x: data.mix_x,
            mix_y: data.mix_y,
            positions: Vec::new(),
            world: Vec::new(),
            lengths: Vec::new(),
            segments: Vec::new(),
            active: false,
        }
    }

    pub fn update(&mut self) {}

    /// Port of `spine::PathConstraint::setToSetupPose`.
    pub fn set_to_setup_pose(&mut self, data: &PathConstraintData) {
        debug_assert_eq!(data.index, self.data_index);
        self.position = data.position;
        self.spacing = data.spacing;
        self.mix_rotate = data.mix_rotate;
        self.mix_x = data.mix_x;
        self.mix_y = data.mix_y;
    }
}

// --- PhysicsConstraint ------------------------------------------------------

/// Runtime-mutable physics constraint (new in Spine 4.2). Simulates damped
/// inertia on one bone.
#[derive(Debug, Clone)]
pub struct PhysicsConstraint {
    pub data_index: PhysicsConstraintId,
    pub bone: BoneId,

    // Per-parameter working values; seeded from data at construction, then
    // modified by `PhysicsConstraintTimeline` (Phase 3).
    pub inertia: f32,
    pub strength: f32,
    pub damping: f32,
    pub mass_inverse: f32,
    pub wind: f32,
    pub gravity: f32,
    pub mix: f32,

    /// Set by constraint API mutators (`translate`, `rotate`) to request a
    /// simulation reset on the next `update`. Read+cleared by the solver.
    pub reset: bool,

    // Previous-frame simulation state — the spring integrator reads and
    // writes these every step. Zeroed on setup.
    pub ux: f32,
    pub uy: f32,
    pub cx: f32,
    pub cy: f32,
    pub tx: f32,
    pub ty: f32,
    pub x_offset: f32,
    pub x_velocity: f32,
    pub y_offset: f32,
    pub y_velocity: f32,
    pub rotate_offset: f32,
    pub rotate_velocity: f32,
    pub scale_offset: f32,
    pub scale_velocity: f32,

    pub active: bool,

    /// Seconds of simulation pending from fractional timesteps.
    pub remaining: f32,
    /// `Skeleton::time` value at the previous `update`. Used to compute delta.
    pub last_time: f32,
}

impl PhysicsConstraint {
    #[must_use]
    pub fn new(data: &PhysicsConstraintData) -> Self {
        Self {
            data_index: data.index,
            bone: data.bone,
            inertia: data.inertia,
            strength: data.strength,
            damping: data.damping,
            mass_inverse: data.mass_inverse,
            wind: data.wind,
            gravity: data.gravity,
            mix: data.mix,
            reset: true,
            ux: 0.0,
            uy: 0.0,
            cx: 0.0,
            cy: 0.0,
            tx: 0.0,
            ty: 0.0,
            x_offset: 0.0,
            x_velocity: 0.0,
            y_offset: 0.0,
            y_velocity: 0.0,
            rotate_offset: 0.0,
            rotate_velocity: 0.0,
            scale_offset: 0.0,
            scale_velocity: 0.0,
            active: false,
            remaining: 0.0,
            last_time: 0.0,
        }
    }

    pub fn update(&mut self) {}

    /// Port of `spine::PhysicsConstraint::setToSetupPose`. Resets the
    /// per-parameter working values; velocity state is left alone (the
    /// solver will consume and zero it on next update).
    pub fn set_to_setup_pose(&mut self, data: &PhysicsConstraintData) {
        debug_assert_eq!(data.index, self.data_index);
        self.inertia = data.inertia;
        self.strength = data.strength;
        self.damping = data.damping;
        self.mass_inverse = data.mass_inverse;
        self.wind = data.wind;
        self.gravity = data.gravity;
        self.mix = data.mix;
    }
}
