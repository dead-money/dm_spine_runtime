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

//! Animation playback: plays animations on a [`Skeleton`], with tracks,
//! queuing, crossfades and events.
//!
//! Start from [`AnimationState`]. It reads crossfade durations from an
//! [`AnimationStateData`] and poses a skeleton from its tracks. Each frame,
//! call [`AnimationState::update`] with the elapsed time, then
//! [`AnimationState::apply`], then compute world transforms with
//! [`Skeleton::update_world_transform`]. Keyframe [`Event`]s come back
//! through `apply`'s out-parameter; lifecycle and keyframe [`StateEvent`]s
//! come from [`AnimationState::drain_events`].
//!
//! To pose one animation directly without tracks or mixing, use
//! [`Skeleton::apply_animation`]. The [`curve`] functions sample raw curve
//! timeline data.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use spine_runtime::animation::{AnimationState, AnimationStateData, EventType};
//! use spine_runtime::data::SkeletonData;
//! use spine_runtime::skeleton::{Physics, Skeleton};
//!
//! fn play(data: Arc<SkeletonData>) -> Result<(), Box<dyn std::error::Error>> {
//!     let mut mixes = AnimationStateData::new(Arc::clone(&data));
//!     mixes.set_default_mix(0.2);
//!     mixes.set_mix_by_name("walk", "run", 0.4)?;
//!
//!     let mut skeleton = Skeleton::new(data);
//!     let mut state = AnimationState::new(Arc::new(mixes));
//!     state.set_animation_by_name(0, "walk", true)?;
//!     state.add_animation_by_name(0, "run", true, 2.0)?;
//!
//!     let mut events = Vec::new();
//!     let dt = 1.0 / 60.0;
//!     for _frame in 0..120 {
//!         state.update(dt);
//!         events.clear();
//!         state.apply(&mut skeleton, &mut events);
//!         skeleton.update(dt);
//!         skeleton.update_world_transform(Physics::Update);
//!         for e in state.drain_events() {
//!             if e.kind == EventType::Complete {
//!                 println!("track {} completed a loop", e.track_index);
//!             }
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! The timeline data lives in [`crate::data::animation`].
//!
//! [`Skeleton`]: crate::skeleton::Skeleton
//! [`Skeleton::update_world_transform`]: crate::skeleton::Skeleton::update_world_transform
//! [`Skeleton::apply_animation`]: crate::skeleton::Skeleton::apply_animation

pub mod apply;
pub mod curve;
pub mod state;
pub mod state_data;

pub use crate::data::animation::{Property, PropertyId};
pub use curve::{bezier_value, compute_bezier_samples, curve_value1, curve_value2, search};
pub use state::{
    AnimationNotFound, AnimationState, EMPTY_ANIMATION_ID, EntryId, EventType, Interpolation,
    StateEvent, TrackEntry,
};
pub use state_data::{AnimationStateData, MixAnimationNotFound};

use crate::data::EventId;

/// A keyframe event that fired between the last and current apply time.
/// Its values are copied from the key's
/// [`AnimationEvent`][crate::data::AnimationEvent].
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Index into [`SkeletonData::events`][crate::data::SkeletonData::events].
    pub data: EventId,
    /// Time along the animation (in seconds) when this event fired.
    pub time: f32,
    pub int_value: i32,
    pub float_value: f32,
    pub string_value: Option<std::sync::Arc<str>>,
    /// Playback volume for an event with an
    /// [`audio_path`][crate::data::EventData::audio_path].
    pub volume: f32,
    /// Stereo balance for an audio event, -1 (left) to 1 (right).
    pub balance: f32,
}

/// What a timeline mixes from before and between its keys.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MixFrom {
    /// The current pose, which a lower track or the app may have written.
    #[default]
    Current,
    /// The setup pose, ignoring the current pose.
    Setup,
    /// The current pose, returning to setup before the first key. Used for
    /// the first entry on a track to key a property.
    First,
}

/// Floats per bezier segment in
/// [`CurveFrames::curves`][crate::data::CurveFrames::curves]: 9 `(x, y)`
/// samples.
///
/// `curves` holds one curve-type code per frame, then the bezier samples.
/// A frame's code is [`CURVE_LINEAR`], [`CURVE_STEPPED`], or
/// [`CURVE_BEZIER`] plus the index of its first channel's samples in
/// `curves`; later channels follow at `BEZIER_SIZE` strides.
pub const BEZIER_SIZE: usize = 18;

/// Linear interpolation curve-type code.
pub const CURVE_LINEAR: i32 = 0;
/// Stepped (no interpolation) curve-type code.
pub const CURVE_STEPPED: i32 = 1;
/// Bezier curve-type code. The stored value is this plus the sample index
/// in `CurveFrames::curves`.
pub const CURVE_BEZIER: i32 = 2;
