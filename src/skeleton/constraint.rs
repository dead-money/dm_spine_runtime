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

//! Runtime constraint state, one [`Constraint`] per [`ConstraintData`] in
//! the same order.
//!
//! Each constraint holds a [`Posed`] of its data's pose type, the values
//! constraint timelines animate. The solvers themselves run
//! inside [`Skeleton::update_world_transform`](crate::skeleton::Skeleton::update_world_transform).

use crate::data::{
    ConstraintData, IkConstraintPose, PathConstraintPose, PhysicsConstraintPose, SliderPose,
    TransformConstraintPose,
};
use crate::skeleton::pose::Posed;

/// A constraint instance. The variant always matches the
/// [`ConstraintData`] at the same index.
#[derive(Debug, Clone)]
pub enum Constraint {
    Ik(IkConstraint),
    Transform(TransformConstraint),
    Path(PathConstraint),
    Physics(PhysicsConstraint),
    Slider(Slider),
}

/// Rotates one or two bones so the chain reaches a target bone.
#[derive(Debug, Clone)]
pub struct IkConstraint {
    pub posed: Posed<IkConstraintPose>,
}

/// Copies transform properties from a source bone to other bones.
#[derive(Debug, Clone)]
pub struct TransformConstraint {
    pub posed: Posed<TransformConstraintPose>,
}

/// Positions, rotates and optionally scales bones along a path attachment.
/// Also carries per-constraint scratch buffers so updates don't allocate.
#[derive(Debug, Clone, Default)]
pub struct PathConstraint {
    pub posed: Posed<PathConstraintPose>,
    pub(crate) spaces: Vec<f32>,
    pub(crate) positions: Vec<f32>,
    pub(crate) world: Vec<f32>,
    pub(crate) curves: Vec<f32>,
    pub(crate) lengths: Vec<f32>,
    pub(crate) segments: [f32; 10],
}

/// Moves a bone with a damped spring simulation. Holds the simulation state
/// carried between frames.
#[derive(Debug, Clone, Default)]
pub struct PhysicsConstraint {
    pub posed: Posed<PhysicsConstraintPose>,
    pub(crate) reset: bool,
    pub(crate) ux: f32,
    pub(crate) uy: f32,
    pub(crate) cx: f32,
    pub(crate) cy: f32,
    pub(crate) tx: f32,
    pub(crate) ty: f32,
    pub(crate) x_offset: f32,
    pub(crate) x_lag: f32,
    pub(crate) x_velocity: f32,
    pub(crate) y_offset: f32,
    pub(crate) y_lag: f32,
    pub(crate) y_velocity: f32,
    pub(crate) rotate_offset: f32,
    pub(crate) rotate_lag: f32,
    pub(crate) rotate_velocity: f32,
    pub(crate) scale_offset: f32,
    pub(crate) scale_lag: f32,
    pub(crate) scale_velocity: f32,
    pub(crate) remaining: f32,
    pub(crate) last_time: f32,
}

impl PhysicsConstraint {
    /// Clears simulation state, as on [`Physics::Reset`] or a physics reset
    /// timeline. The next update restarts from the bone's current pose.
    /// `time` is the [`Skeleton::time`] to step from.
    ///
    /// [`Physics::Reset`]: crate::skeleton::Physics::Reset
    /// [`Skeleton::time`]: crate::skeleton::Skeleton::time
    pub fn reset(&mut self, time: f32) {
        self.remaining = 0.0;
        self.last_time = time;
        self.reset = true;
        self.x_offset = 0.0;
        self.x_lag = 0.0;
        self.x_velocity = 0.0;
        self.y_offset = 0.0;
        self.y_lag = 0.0;
        self.y_velocity = 0.0;
        self.rotate_offset = 0.0;
        self.rotate_lag = 0.0;
        self.rotate_velocity = 0.0;
        self.scale_offset = 0.0;
        self.scale_lag = 0.0;
        self.scale_velocity = 0.0;
    }

    /// Moves the simulation by `(x, y)` world units without adding inertia,
    /// for teleporting a skeleton.
    pub fn translate(&mut self, x: f32, y: f32) {
        self.ux -= x;
        self.uy -= y;
        self.cx -= x;
        self.cy -= y;
    }

    /// Rotates the simulation by `degrees` around world point `(x, y)`
    /// without adding inertia.
    pub fn rotate(&mut self, x: f32, y: f32, degrees: f32) {
        let r = degrees * crate::math::util::DEG_RAD;
        let (cos, sin) = (r.cos(), r.sin());
        let dx = self.cx - x;
        let dy = self.cy - y;
        self.translate(dx * cos - dy * sin - dx, dx * sin + dy * cos - dy);
    }
}

/// Applies an animation to the skeleton at a time set by the slider's pose
/// or driven by a bone.
#[derive(Debug, Clone)]
pub struct Slider {
    pub posed: Posed<SliderPose>,
}

impl Constraint {
    /// A constraint in its data's setup pose.
    #[must_use]
    pub fn new(data: &ConstraintData) -> Self {
        match data {
            ConstraintData::Ik(d) => Self::Ik(IkConstraint {
                posed: Posed::new(d.setup, d.setup),
            }),
            ConstraintData::Transform(d) => Self::Transform(TransformConstraint {
                posed: Posed::new(d.setup, d.setup),
            }),
            ConstraintData::Path(d) => Self::Path(PathConstraint {
                posed: Posed::new(d.setup, d.setup),
                ..PathConstraint::default()
            }),
            ConstraintData::Physics(d) => Self::Physics(PhysicsConstraint {
                posed: Posed::new(d.setup, d.setup),
                reset: true,
                ..PhysicsConstraint::default()
            }),
            ConstraintData::Slider(d) => Self::Slider(Slider {
                posed: Posed::new(d.setup, d.setup),
            }),
        }
    }

    /// Resets the unconstrained pose to `data`'s setup pose. Physics
    /// simulation state is left alone.
    ///
    /// # Panics
    /// If `data` is a different constraint kind.
    pub fn setup_pose(&mut self, data: &ConstraintData) {
        match (self, data) {
            (Self::Ik(c), ConstraintData::Ik(d)) => c.posed.pose = d.setup,
            (Self::Transform(c), ConstraintData::Transform(d)) => c.posed.pose = d.setup,
            (Self::Path(c), ConstraintData::Path(d)) => c.posed.pose = d.setup,
            (Self::Physics(c), ConstraintData::Physics(d)) => c.posed.pose = d.setup,
            (Self::Slider(c), ConstraintData::Slider(d)) => c.posed.pose = d.setup,
            _ => unreachable!("constraint kind matches its data"),
        }
    }

    pub(crate) fn constrain(&mut self) -> bool {
        macro_rules! go {
            ($c:expr) => {{
                if $c.posed.is_constrained() {
                    false
                } else {
                    $c.posed.constrain();
                    true
                }
            }};
        }
        match self {
            Self::Ik(c) => go!(c),
            Self::Transform(c) => go!(c),
            Self::Path(c) => go!(c),
            Self::Physics(c) => go!(c),
            Self::Slider(c) => go!(c),
        }
    }

    pub(crate) fn unconstrain(&mut self) {
        match self {
            Self::Ik(c) => c.posed.unconstrain(),
            Self::Transform(c) => c.posed.unconstrain(),
            Self::Path(c) => c.posed.unconstrain(),
            Self::Physics(c) => c.posed.unconstrain(),
            Self::Slider(c) => c.posed.unconstrain(),
        }
    }

    pub(crate) fn reset_constrained(&mut self) {
        match self {
            Self::Ik(c) => c.posed.reset_constrained(),
            Self::Transform(c) => c.posed.reset_constrained(),
            Self::Path(c) => c.posed.reset_constrained(),
            Self::Physics(c) => c.posed.reset_constrained(),
            Self::Slider(c) => c.posed.reset_constrained(),
        }
    }
}
