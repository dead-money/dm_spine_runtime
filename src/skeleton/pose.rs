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

//! Unconstrained and constrained poses. Animations and application code
//! write `pose`. An object a constraint affects is marked constrained by
//! `Skeleton::update_cache`; its `constrained` pose is reset from `pose`
//! each frame and is the applied pose.

/// A pose type that can be reset from another instance.
pub trait Pose {
    /// Copies the parts of `other` that a constrained pose inherits each frame.
    fn set_from(&mut self, other: &Self);
}

#[derive(Debug, Clone, Default)]
pub struct Posed<P> {
    pub pose: P,
    pub constrained: P,
    is_constrained: bool,
}

impl<P: Pose> Posed<P> {
    pub fn new(pose: P, constrained: P) -> Self {
        Self {
            pose,
            constrained,
            is_constrained: false,
        }
    }

    /// The pose to read: constrained if a constraint writes this object.
    #[inline]
    pub fn applied(&self) -> &P {
        if self.is_constrained {
            &self.constrained
        } else {
            &self.pose
        }
    }

    #[inline]
    pub fn applied_mut(&mut self) -> &mut P {
        if self.is_constrained {
            &mut self.constrained
        } else {
            &mut self.pose
        }
    }

    /// The applied pose if `applied`, else `pose`. Timelines applied by a
    /// slider pass `applied = true`.
    #[inline]
    pub fn select_mut(&mut self, applied: bool) -> &mut P {
        if applied {
            self.applied_mut()
        } else {
            &mut self.pose
        }
    }

    #[inline]
    pub fn select(&self, applied: bool) -> &P {
        if applied { self.applied() } else { &self.pose }
    }

    #[inline]
    pub fn is_constrained(&self) -> bool {
        self.is_constrained
    }

    pub(crate) fn constrain(&mut self) {
        self.is_constrained = true;
    }

    pub(crate) fn unconstrain(&mut self) {
        self.is_constrained = false;
    }

    pub(crate) fn reset_constrained(&mut self) {
        self.constrained.set_from(&self.pose);
    }
}

macro_rules! copy_pose {
    ($($t:ty),*) => {
        $(impl Pose for $t {
            #[inline]
            fn set_from(&mut self, other: &Self) {
                *self = *other;
            }
        })*
    };
}

copy_pose!(
    crate::data::IkConstraintPose,
    crate::data::TransformConstraintPose,
    crate::data::PathConstraintPose,
    crate::data::PhysicsConstraintPose,
    crate::data::SliderPose
);
