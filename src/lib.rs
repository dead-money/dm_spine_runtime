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

//! Native Rust port of the [Spine](https://esotericsoftware.com/) 4.3
//! runtime.
//!
//! The crate loads skeletons exported from the Spine editor, poses and
//! animates them, and emits triangle batches for your renderer to draw. It
//! has no GPU or windowing dependencies. Every developer using it needs their
//! own Spine Editor license; see the README.
//!
//! Only Spine 4.3 exports load: binary `.skel` or JSON, each with its
//! `.atlas`.
//!
//! # Where to start
//!
//! - [`atlas::Atlas`] parses a `.atlas` file. Loading the page images is up
//!   to you.
//! - [`load::SkeletonBinary`] or [`load::SkeletonJson`], given an
//!   [`load::AtlasAttachmentLoader`], reads an export into a
//!   [`data::SkeletonData`]. Load it once and share it in an `Arc`.
//! - [`skeleton::Skeleton`] is one posable instance of that data.
//! - [`animation::AnimationState`] plays, queues and crossfades animations
//!   on tracks; [`animation::AnimationStateData`] holds the crossfade times.
//! - [`render::SkeletonRenderer`] turns a posed skeleton into
//!   [`render::RenderCommand`]s. Each command names its texture by atlas page
//!   index ([`render::TextureId`]).
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use spine_runtime::animation::{AnimationState, AnimationStateData};
//! use spine_runtime::atlas::Atlas;
//! use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
//! use spine_runtime::render::SkeletonRenderer;
//! use spine_runtime::skeleton::{Physics, Skeleton};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let atlas = Atlas::parse(&std::fs::read_to_string("spineboy.atlas")?)?;
//! let mut loader = AtlasAttachmentLoader::new(&atlas);
//! let bytes = std::fs::read("spineboy-pro.skel")?;
//! let data = Arc::new(SkeletonBinary::with_loader(&mut loader).read(&bytes)?);
//!
//! let mut skeleton = Skeleton::new(Arc::clone(&data));
//! let mut state = AnimationState::new(Arc::new(AnimationStateData::new(data)));
//! state.set_animation_by_name(0, "walk", true)?;
//!
//! let mut renderer = SkeletonRenderer::new();
//! let mut events = Vec::new();
//! let dt = 1.0 / 60.0;
//! for _frame in 0..60 {
//!     state.update(dt);
//!     events.clear();
//!     state.apply(&mut skeleton, &mut events);
//!     skeleton.update(dt);
//!     skeleton.update_world_transform(Physics::Update);
//!     for command in renderer.render(&skeleton) {
//!         // Upload command.positions, uvs, colors and indices; bind the
//!         // texture for command.texture; set command.blend_mode.
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Coordinates are y-up. Rotations are in degrees and times in seconds.

pub mod animation;
pub mod atlas;
pub mod data;
pub mod load;
pub mod math;
pub mod render;
pub mod skeleton;
