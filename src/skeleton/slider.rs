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

//! Slider constraint (`Slider.cpp`): applies an animation at a time set by
//! its pose or driven by a bone channel.

use crate::data::{ConstraintId, SkeletonData, SliderData, Timeline};
use crate::skeleton::bone;
use crate::skeleton::constraint::Constraint;
use crate::skeleton::transform::from_value;
use crate::skeleton::{Skeleton, UpdateCacheEntry};

impl Skeleton {
    pub(crate) fn sort_slider(&mut self, id: ConstraintId, data: &SliderData, sd: &SkeletonData) {
        if let Some(b) = data.bone
            && !data.local
        {
            self.sort_bone(b);
        }
        self.update_cache.push(UpdateCacheEntry::Constraint(id));
        let Some(animation) = data.animation else {
            return;
        };
        let animation = &sd.animations[animation.index()];
        for &b in &animation.bones {
            self.bones[b.index()].sorted = false;
            self.sort_reset(b);
            self.constrain_bone(b);
        }
        for t in &animation.timelines {
            match t {
                Timeline::Attachment { slot, .. }
                | Timeline::Rgba { slot, .. }
                | Timeline::Rgb { slot, .. }
                | Timeline::Alpha { slot, .. }
                | Timeline::Rgba2 { slot, .. }
                | Timeline::Rgb2 { slot, .. }
                | Timeline::Deform { slot, .. }
                | Timeline::Sequence { slot, .. } => self.constrain_slot(*slot),
                Timeline::DrawOrder { .. } | Timeline::DrawOrderFolder { .. } => {
                    self.draw_order.constrain();
                }
                Timeline::Physics {
                    constraint: None, ..
                }
                | Timeline::PhysicsReset {
                    constraint: None, ..
                } => {
                    for i in 0..self.physics.len() {
                        self.constrain_constraint(self.physics[i]);
                    }
                }
                Timeline::IkConstraint { constraint, .. }
                | Timeline::TransformConstraint { constraint, .. }
                | Timeline::PathConstraintPosition { constraint, .. }
                | Timeline::PathConstraintSpacing { constraint, .. }
                | Timeline::PathConstraintMix { constraint, .. }
                | Timeline::Physics {
                    constraint: Some(constraint),
                    ..
                }
                | Timeline::PhysicsReset {
                    constraint: Some(constraint),
                    ..
                }
                | Timeline::Slider { constraint, .. }
                | Timeline::SliderMix { constraint, .. } => self.constrain_constraint(*constraint),
                _ => {}
            }
        }
    }

    pub(crate) fn update_slider(&mut self, id: ConstraintId, data: &SliderData, sd: &SkeletonData) {
        let Constraint::Slider(c) = &self.constraints[id.index()] else {
            unreachable!()
        };
        let mut p = *c.posed.applied();
        if p.mix == 0.0 {
            return;
        }
        let Some(animation_id) = data.animation else {
            return;
        };
        let animation = &sd.animations[animation_id.index()];
        let f = self.frame();
        if let (Some(b), Some(property)) = (data.bone, data.property) {
            if !self.bones[b.index()].active {
                return;
            }
            if data.local {
                bone::validate_local_transform(&mut self.bones, b.index(), &f);
            }
            let source = *self.bones[b.index()].applied();
            p.time = data.offset
                + (from_value(property.property, &f, &source, data.local, &[0.0; 6])
                    - property.offset)
                    * data.scale;
            if data.looping {
                p.time = animation.duration + p.time % animation.duration;
            } else {
                p.time = p.time.max(0.0);
            }
            if let Constraint::Slider(c) = &mut self.constraints[id.index()] {
                c.posed.applied_mut().time = p.time;
            }
        }
        for &b in &animation.bones {
            bone::modify_local(&mut self.bones, b.index(), &f);
        }
        self.apply_animation(
            animation_id,
            p.time,
            p.time,
            data.looping,
            None,
            p.mix,
            crate::animation::MixFrom::Current,
            data.additive,
            false,
            true,
        );
    }
}
