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

/// How a bone inherits its parent's world transform.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Inherit {
    /// Rotation, scale, shear and reflection.
    #[default]
    Normal,
    OnlyTranslation,
    NoRotationOrReflection,
    NoScale,
    NoScaleOrReflection,
}

impl Inherit {
    /// Decodes the wire value shared by the binary and JSON formats; `None`
    /// if out of range.
    #[must_use]
    pub fn from_index(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Normal,
            1 => Self::OnlyTranslation,
            2 => Self::NoRotationOrReflection,
            3 => Self::NoScale,
            4 => Self::NoScaleOrReflection,
            _ => return None,
        })
    }
}

/// A bone's local transform. Used for the setup pose, the unconstrained
/// pose animations write, and the constrained pose constraints write.
///
/// Translation is in the parent's space, in skeleton units. Rotation and
/// shear are in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneLocal {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,
    pub inherit: Inherit,
}

impl Default for BoneLocal {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            shear_x: 0.0,
            shear_y: 0.0,
            inherit: Inherit::Normal,
        }
    }
}

/// Setup-pose bone, owned by [`SkeletonData`](crate::data::SkeletonData).
#[derive(Debug, Clone, PartialEq)]
pub struct BoneData {
    /// Position in [`SkeletonData::bones`](crate::data::SkeletonData::bones).
    pub index: BoneId,
    pub name: String,
    /// `None` only for the root bone.
    pub parent: Option<BoneId>,
    /// Skeleton units. Used by IK, path and physics constraints.
    pub length: f32,
    pub setup: BoneLocal,
    /// Active only while a skin listing this bone is applied.
    pub skin_required: bool,

    /// Nonessential: editor display color.
    pub color: Color,
    /// Nonessential: editor icon name and display.
    pub icon: String,
    pub icon_size: f32,
    pub icon_rotation: f32,
    /// Nonessential: shown in the editor.
    pub visible: bool,
}

impl BoneData {
    /// A bone at the origin with identity transform.
    #[must_use]
    pub fn new(index: BoneId, name: impl Into<String>, parent: Option<BoneId>) -> Self {
        Self {
            index,
            name: name.into(),
            parent,
            length: 0.0,
            setup: BoneLocal::default(),
            skin_required: false,
            color: Color::new(0.61, 0.61, 0.61, 1.0),
            icon: String::new(),
            icon_size: 1.0,
            icon_rotation: 0.0,
            visible: true,
        }
    }
}
