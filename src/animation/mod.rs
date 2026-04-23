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

//! Timeline evaluation — curve sampling + `Timeline::apply` logic.
//!
//! Data shapes (the [`Timeline`][crate::data::Timeline] enum, [`CurveFrames`],
//! etc.) live in [`crate::data::animation`]. This module is the runtime side
//! — the code that reads those shapes and pushes values into a
//! [`Skeleton`][crate::skeleton::Skeleton].

pub mod apply;
pub mod curve;
pub mod property;
pub mod state;
pub mod state_data;

pub use curve::{bezier_value, compute_bezier_samples, curve_value1, curve_value2, search};
pub use property::{Property, PropertyId, animation_has_timeline, property_ids};
pub use state::{
    AnimationNotFound, AnimationState, EMPTY_ANIMATION_ID, EntryId, EventType, StateEvent,
    TimelineMode, TrackEntry,
};
pub use state_data::{AnimationStateData, MixAnimationNotFound};

use crate::data::EventId;

/// Runtime event firing — one per animation frame that tripped since the
/// previous `apply` call. Carries a copy of the frame's int/float/string
/// values so downstream consumers can read them without chasing back to
/// [`AnimationEvent`][crate::data::AnimationEvent].
///
/// Pushed into the `events` out-param by [`Timeline::Event`][crate::data::Timeline::Event]
/// during `Animation::apply`. [`AnimationState`] also drains these into its
/// lifecycle event queue.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Index into [`SkeletonData::events`][crate::data::SkeletonData::events].
    pub data: EventId,
    /// Time along the animation (in seconds) when this event fired.
    pub time: f32,
    pub int_value: i32,
    pub float_value: f32,
    pub string_value: Option<String>,
    pub volume: f32,
    pub balance: f32,
}

/// How a timeline blends with the existing pose when applied.
///
/// Ported from `spine-cpp/include/spine/MixBlend.h`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MixBlend {
    /// Overwrite the target property with the timeline's value (setup
    /// pose + timeline, scaled by `alpha`). Used on the first frame an
    /// animation starts driving a property.
    #[default]
    Setup,
    /// Same as `Setup` on the first pose of a track, but subsequent poses
    /// read from the current skeleton state. Used to seed the "first"
    /// value a track contributes so later tracks can blend against it.
    First,
    /// Blend timeline value into current pose, replacing whatever was
    /// there before. Non-additive.
    Replace,
    /// Add timeline value onto current pose. Useful for layering e.g. a
    /// breathing animation on top of a walk cycle.
    Add,
}

/// Direction a timeline is mixing in a crossfade.
///
/// `In` = alpha is ramping toward 1 (this animation taking over); `Out` =
/// alpha is ramping toward 0 (this animation handing off to setup or the
/// next track).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MixDirection {
    #[default]
    In,
    Out,
}

/// Number of floats per bezier segment stored in `CurveFrames::curves`.
///
/// Ports `spine-cpp`'s `CurveTimeline::BEZIER_SIZE`: 9 samples × 2 floats
/// (x, y) each. The exact shape is `_curves = [type_frame_0, …,
/// type_frame_(N-1), bezier_sample_0_x, bezier_sample_0_y, …]`; the per-frame
/// type code either encodes `LINEAR` / `STEPPED` directly or an absolute
/// offset into the bezier-sample tail (see [`curve::curve_value1`]).
pub const BEZIER_SIZE: usize = 18;

/// Linear interpolation curve-type code (stored in `CurveFrames::curves[frame]`).
pub const CURVE_LINEAR: i32 = 0;
/// Stepped (no interpolation) curve-type code.
pub const CURVE_STEPPED: i32 = 1;
/// Bezier base code. Actual stored value is `BEZIER + offset_into_curves`.
pub const CURVE_BEZIER: i32 = 2;
