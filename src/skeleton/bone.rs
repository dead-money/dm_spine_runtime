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

//! Runtime-mutable bone state. One `Bone` per [`BoneData`] in the owning
//! `Skeleton`. The runtime `Bone` carries every field `spine-cpp`'s `Bone`
//! mutates during pose/constraint/animation passes, plus parent/child
//! indices built once at `Skeleton` construction.

use crate::data::{BoneData, BoneId, Inherit};

/// Runtime-mutable bone pose state.
///
/// Mirrors the private fields of `spine::Bone` in `spine-cpp/include/spine/Bone.h`
/// field-for-field, with one adjustment: parent/children are stored as
/// [`BoneId`] indices rather than pointers so the skeleton can own a flat
/// `Vec<Bone>`. Every field is public because `Skeleton`, the constraint
/// solvers (Phase 5), and the animation apply path (Phase 3) all need direct
/// read/write access — Rust's `pub` fields play the role of `spine-cpp`'s
/// long list of `friend class` declarations.
#[derive(Debug, Clone)]
pub struct Bone {
    /// Index into [`SkeletonData::bones`][crate::data::SkeletonData::bones].
    /// Read back via `&skeleton.data().bones[bone.data_index.index()]`.
    pub data_index: BoneId,

    /// Parent bone index, or `None` for the root bone. Copied from
    /// [`BoneData::parent`] at construction and never modified afterwards.
    pub parent: Option<BoneId>,

    /// Direct children by `BoneId`. Populated by `Skeleton::new` in one pass
    /// over the data bones. Order matches the data bones order (which is
    /// parents-before-children), so traversal is deterministic.
    pub children: Vec<BoneId>,

    // Local setup-pose-modifiable transform. Timelines write here.
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,

    // "Applied" local transform — the values actually realised on the bone
    // after any constraint pass mutated world state. `Bone::update_world_transform`
    // copies local → applied at the top of each frame; constraints that write
    // to the world matrix directly call `update_applied_transform` to sync
    // these back. See `spine-cpp/src/spine/Bone.cpp` `updateAppliedTransform`.
    pub ax: f32,
    pub ay: f32,
    pub a_rotation: f32,
    pub a_scale_x: f32,
    pub a_scale_y: f32,
    pub a_shear_x: f32,
    pub a_shear_y: f32,

    // World-space affine: [a b; c d] | world_x, world_y.
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub world_x: f32,
    pub world_y: f32,

    /// `true` once this bone has been visited during `Skeleton::update_cache`.
    /// Reset to `false` at the start of each `update_cache` pass.
    pub sorted: bool,

    /// `true` when this bone participates in the current cache. Set by
    /// `update_cache` based on skin requirements.
    pub active: bool,

    /// Runtime-mutable inherit mode. Starts at [`BoneData::inherit`];
    /// animations (the `InheritTimeline`, new in 4.2) may change it per-frame.
    pub inherit: Inherit,
}

impl Bone {
    /// Build a runtime bone initialised to `data`'s setup pose.
    ///
    /// Matches `spine::Bone::Bone(BoneData &, Skeleton &, Bone *)` followed by
    /// `setToSetupPose()` from `spine-cpp`. World-space fields are zeroed —
    /// `Skeleton::update_world_transform` (Phase 2d) is responsible for
    /// populating them before callers inspect `a/b/c/d/world_x/world_y`.
    #[must_use]
    pub fn new(data: &BoneData) -> Self {
        let mut bone = Self {
            data_index: data.index,
            parent: data.parent,
            children: Vec::new(),
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            shear_x: 0.0,
            shear_y: 0.0,
            ax: 0.0,
            ay: 0.0,
            a_rotation: 0.0,
            a_scale_x: 1.0,
            a_scale_y: 1.0,
            a_shear_x: 0.0,
            a_shear_y: 0.0,
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            world_x: 0.0,
            world_y: 0.0,
            sorted: false,
            active: true,
            inherit: data.inherit,
        };
        bone.set_to_setup_pose(data);
        bone
    }

    /// Copy the setup-pose local transform out of `data` onto this bone.
    ///
    /// Matches `spine::Bone::setToSetupPose` — it only resets *local* TRS,
    /// not world-space. Call [`Skeleton::update_world_transform`] afterwards
    /// if the world matrix matters.
    pub fn set_to_setup_pose(&mut self, data: &BoneData) {
        debug_assert_eq!(data.index, self.data_index);
        self.x = data.x;
        self.y = data.y;
        self.rotation = data.rotation;
        self.scale_x = data.scale_x;
        self.scale_y = data.scale_y;
        self.shear_x = data.shear_x;
        self.shear_y = data.shear_y;
        self.inherit = data.inherit;
    }
}
