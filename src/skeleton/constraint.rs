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

//! Runtime constraints, one per [`ConstraintData`] in the same order.

use crate::data::{
    ConstraintData, IkConstraintPose, PathConstraintPose, PhysicsConstraintPose, SliderPose,
    TransformConstraintPose,
};
use crate::skeleton::pose::Posed;

#[derive(Debug, Clone)]
pub enum Constraint {
    Ik(IkConstraint),
    Transform(TransformConstraint),
    Path(PathConstraint),
    Physics(PhysicsConstraint),
    Slider(Slider),
}

#[derive(Debug, Clone)]
pub struct IkConstraint {
    pub posed: Posed<IkConstraintPose>,
}

#[derive(Debug, Clone)]
pub struct TransformConstraint {
    pub posed: Posed<TransformConstraintPose>,
}

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

/// Spring simulation state carried between frames.
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
    /// Clears simulation state, as at load or on a physics reset.
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

    /// Moves the simulation by `(x, y)` without adding inertia.
    pub fn translate(&mut self, x: f32, y: f32) {
        self.ux -= x;
        self.uy -= y;
        self.cx -= x;
        self.cy -= y;
    }

    /// Rotates the simulation around `(x, y)` without adding inertia.
    pub fn rotate(&mut self, x: f32, y: f32, degrees: f32) {
        let r = degrees * crate::math::util::DEG_RAD;
        let (cos, sin) = (r.cos(), r.sin());
        let dx = self.cx - x;
        let dy = self.cy - y;
        self.translate(dx * cos - dy * sin - dx, dx * sin + dy * cos - dy);
    }
}

#[derive(Debug, Clone)]
pub struct Slider {
    pub posed: Posed<SliderPose>,
}

impl Constraint {
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
