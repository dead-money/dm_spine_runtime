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

//! Animation playback: curve sampling, timeline application, and
//! [`AnimationState`].
//!
//! The timeline data lives in [`crate::data::animation`]; this module reads
//! it and writes the resulting values into a
//! [`Skeleton`][crate::skeleton::Skeleton].

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
/// Copies the key's values from [`AnimationEvent`][crate::data::AnimationEvent].
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Index into [`SkeletonData::events`][crate::data::SkeletonData::events].
    pub data: EventId,
    /// Time along the animation (in seconds) when this event fired.
    pub time: f32,
    pub int_value: i32,
    pub float_value: f32,
    pub string_value: Option<std::sync::Arc<str>>,
    pub volume: f32,
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

/// Floats per bezier segment in `CurveFrames::curves`: 9 (x, y) samples.
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
