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

//! Setup-pose bone data.

use crate::data::BoneId;
use crate::math::Color;

/// Controls how a bone inherits its parent's world transform.
///
/// Ported from `spine-cpp/include/spine/Inherit.h`. Prior to Spine 4.2 this
/// was named `TransformMode` with `SP_TRANSFORMMODE_*` variants.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Inherit {
    /// Default: inherit translation, rotation, scale, shear.
    #[default]
    Normal,
    /// Inherit translation only.
    OnlyTranslation,
    /// Inherit translation and scale; ignore parent rotation / reflection.
    NoRotationOrReflection,
    /// Inherit translation and rotation; ignore parent scale.
    NoScale,
    /// Inherit translation and rotation; ignore parent scale and reflection.
    NoScaleOrReflection,
}

/// Immutable setup-pose bone, owned by [`SkeletonData`].
///
/// The `Skeleton` runtime instance holds mutable `Bone` snapshots
/// initialised from this data.
///
/// [`SkeletonData`]: crate::data::SkeletonData
#[derive(Debug, Clone, PartialEq)]
pub struct BoneData {
    /// Index of this bone in [`SkeletonData::bones`][crate::data::SkeletonData::bones].
    pub index: BoneId,
    pub name: String,
    /// Parent bone index, or `None` for the root bone.
    pub parent: Option<BoneId>,

    pub length: f32,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,
    pub inherit: Inherit,

    /// When true, this bone is only active while a skin that lists it is
    /// applied to the skeleton.
    pub skin_required: bool,

    // Non-essential fields (only present when the export included
    // non-essential data; set to sensible defaults otherwise).
    pub color: Color,
    pub icon: String,
    pub visible: bool,
}

impl BoneData {
    /// Construct a bone with setup-pose defaults matching `spine-cpp`'s
    /// `BoneData` constructor: identity rotation, unit scale, no shear.
    #[must_use]
    pub fn new(index: BoneId, name: impl Into<String>, parent: Option<BoneId>) -> Self {
        Self {
            index,
            name: name.into(),
            parent,
            length: 0.0,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            shear_x: 0.0,
            shear_y: 0.0,
            inherit: Inherit::Normal,
            skin_required: false,
            color: Color::WHITE,
            icon: String::new(),
            visible: true,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // All comparisons in this module are against literal setup-pose defaults.
mod tests {
    use super::*;

    #[test]
    fn new_bone_has_setup_pose_defaults() {
        let b = BoneData::new(BoneId(0), "root", None);
        assert_eq!(b.scale_x, 1.0);
        assert_eq!(b.scale_y, 1.0);
        assert_eq!(b.rotation, 0.0);
        assert_eq!(b.inherit, Inherit::Normal);
        assert_eq!(b.parent, None);
        assert_eq!(b.color, Color::WHITE);
        assert!(b.visible);
    }
}
