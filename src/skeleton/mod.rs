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

//! Runtime-mutable skeleton pose.
//!
//! Counterpart to [`crate::data`]: where `data` is the immutable,
//! `Arc`-shared setup-pose asset, this module owns the per-instance mutable
//! state every `Skeleton` needs to animate and be rendered.

pub mod bone;
pub mod constraint;
pub mod ik;
pub mod path;
// 1:1 port parity with `spine-cpp/src/spine/Skeleton.cpp`. The inner module
// name matches the file it came from; the inception is intentional.
#[allow(clippy::module_inception)]
pub mod skeleton;
pub mod slot;
pub mod transform;
pub mod update_cache;

pub use bone::Bone;
pub use constraint::{IkConstraint, PathConstraint, PhysicsConstraint, TransformConstraint};
pub use skeleton::{Skeleton, SkinNotFound};
pub use slot::Slot;
pub use update_cache::UpdateCacheEntry;

/// Controls how physics constraints behave on this `update_world_transform`
/// pass. Ported verbatim from `spine-cpp/include/spine/Physics.h`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Physics {
    /// Physics are not updated or applied.
    #[default]
    None,
    /// Physics are reset to the current pose.
    Reset,
    /// Physics are updated and the pose from physics is applied.
    Update,
    /// Physics are not updated but the pose from physics is applied.
    Pose,
}
