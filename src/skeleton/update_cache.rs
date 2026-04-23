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

//! Unified update order for `Skeleton::update_world_transform`.
//!
//! `spine-cpp` stores update-cache entries as a `Vector<Updatable *>` where
//! `Updatable` is a polymorphic base class. Our tagged-enum equivalent keeps
//! dispatch cache-friendly and matches the "no `Box<dyn ...>` in hot paths"
//! invariant from `CLAUDE.md`.

use crate::data::{
    BoneId, IkConstraintId, PathConstraintId, PhysicsConstraintId, TransformConstraintId,
};

/// One entry in the per-skeleton update order.
///
/// Built by `Skeleton::update_cache` (Phase 2c) and consumed by
/// `Skeleton::update_world_transform` (Phase 2d). Ordering reflects the
/// constraint dependency graph: a bone appears after every constraint it
/// depends on and before every constraint that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCacheEntry {
    Bone(BoneId),
    IkConstraint(IkConstraintId),
    TransformConstraint(TransformConstraintId),
    PathConstraint(PathConstraintId),
    PhysicsConstraint(PhysicsConstraintId),
}
