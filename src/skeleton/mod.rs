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

//! Per-instance skeleton state: bones, slots, constraints and their poses.
//!
//! A [`Skeleton`] is one posed instance of a shared
//! [`SkeletonData`](crate::data::SkeletonData). It owns a [`Bone`] per bone,
//! a [`Slot`] per slot and a [`Constraint`] per constraint, indexed by the
//! same typed ids as the data, plus a [`DrawOrder`]. Animations write local
//! poses; [`Skeleton::update_world_transform`] then runs bones and
//! constraints in update order to produce world transforms, which
//! [`SkeletonRenderer`](crate::render::SkeletonRenderer) and
//! [`SkeletonBounds`] read.
//!
//! Each bone, slot and constraint keeps two poses in a [`Posed`]: `pose`,
//! which animations and application code write, and `constrained`, which
//! constraints write. Read the result through `applied()`.
//!
//! ```no_run
//! use std::sync::Arc;
//! use spine_runtime::data::SkeletonData;
//! use spine_runtime::skeleton::{Physics, Skeleton};
//!
//! fn hand_position(data: Arc<SkeletonData>, dt: f32) -> Option<(f32, f32)> {
//!     let mut skeleton = Skeleton::new(data);
//!     skeleton.x = 100.0;
//!     skeleton.update(dt);
//!     skeleton.update_world_transform(Physics::Update);
//!     let hand = skeleton.find_bone("front-hand")?;
//!     let pose = skeleton.bones[hand.index()].applied();
//!     Some((pose.world_x, pose.world_y))
//! }
//! ```

pub mod bone;
pub mod bounds;
pub mod constraint;
pub mod ik;
pub mod path;
pub mod physics;
pub mod pose;
pub mod region_geometry;
#[allow(clippy::module_inception)]
pub mod skeleton;
pub mod slider;
pub mod slot;
pub mod transform;
pub mod update_cache;
pub mod vertex;

pub use bone::{Bone, BonePose};
pub use bounds::{BoundsPolygon, SkeletonBounds};
pub use constraint::{
    Constraint, IkConstraint, PathConstraint, PhysicsConstraint, Slider, TransformConstraint,
};
pub use pose::{Pose, Posed};
pub use region_geometry::RegionGeometry;
pub use skeleton::{Skeleton, SkinNotFound};
pub use slot::{DrawOrder, Slot, SlotPose};
pub use update_cache::{ResetEntry, UpdateCacheEntry};

/// How physics constraints behave during one
/// [`Skeleton::update_world_transform`] call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Physics {
    /// Physics are neither simulated nor applied.
    #[default]
    None,
    /// Simulation state is cleared and restarts from the current pose.
    Reset,
    /// The simulation advances to [`Skeleton::time`] and its result is applied.
    Update,
    /// The last simulated result is applied without advancing.
    Pose,
}
