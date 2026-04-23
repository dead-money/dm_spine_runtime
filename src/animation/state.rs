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

//! Single-track animation state — Phase 3's minimum viable
//! `AnimationState`. Phase 4 extends this into full multi-track mixing
//! with an event queue and listener hooks.
//!
//! Shape deliberately compatible with the full version: track entries
//! carry the fields (`mix_time`, `mix_duration`, next track pointer, …)
//! that the current single-track apply doesn't yet read, so Phase 4 only
//! has to turn them on rather than rework the public API.

use std::sync::Arc;

use crate::animation::{Event, MixBlend, MixDirection};
use crate::data::{AnimationId, SkeletonData};
use crate::skeleton::Skeleton;

/// A single playing animation. Phase 3 uses only one of these at a time
/// (see [`AnimationState::set_animation`]); Phase 4 will stack them.
#[derive(Debug, Clone)]
pub struct TrackEntry {
    pub animation: AnimationId,
    /// Seconds since the track started playing. Advances by
    /// `delta * time_scale` each [`AnimationState::update`].
    pub time: f32,
    /// Previous frame's `time`; fed to timeline applies for event
    /// firing / physics-reset gating.
    pub last_time: f32,
    pub loop_: bool,
    /// Overall track weight (0 = no effect, 1 = full). Passed as `alpha`
    /// to every timeline.
    pub alpha: f32,
    /// Multiplier on `delta`. 1.0 = real-time, <1 = slower, >1 = faster.
    pub time_scale: f32,
    /// Blend mode this track contributes with. First track on the stack is
    /// typically `MixBlend::Setup`; stacked tracks default to `Replace`.
    pub blend: MixBlend,
}

impl TrackEntry {
    #[must_use]
    pub fn new(animation: AnimationId, loop_: bool) -> Self {
        Self {
            animation,
            time: 0.0,
            last_time: -1.0, // spine-cpp convention: first apply fires all events
            loop_,
            alpha: 1.0,
            time_scale: 1.0,
            blend: MixBlend::Setup,
        }
    }
}

/// Minimal single-track animation driver.
///
/// ```ignore
/// let mut state = AnimationState::new(Arc::clone(&data));
/// state.set_animation_by_name("walk", true)?;
/// loop {
///     state.update(dt);
///     state.apply(&mut skeleton, &mut events);
///     skeleton.update_world_transform(Physics::None);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AnimationState {
    data: Arc<SkeletonData>,
    track: Option<TrackEntry>,
}

impl AnimationState {
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        Self { data, track: None }
    }

    /// Immutable peek at the current track, if any.
    #[must_use]
    pub fn track(&self) -> Option<&TrackEntry> {
        self.track.as_ref()
    }

    /// Mutable access for tweaking `alpha`, `time_scale`, or `blend`
    /// without restarting the animation.
    pub fn track_mut(&mut self) -> Option<&mut TrackEntry> {
        self.track.as_mut()
    }

    /// Start playing `animation` from time 0. Replaces any existing track.
    ///
    /// # Panics
    ///
    /// The returned reference is obtained via `unwrap` on the just-assigned
    /// `Option`; cannot panic in practice.
    pub fn set_animation(&mut self, animation: AnimationId, loop_: bool) -> &mut TrackEntry {
        self.track = Some(TrackEntry::new(animation, loop_));
        self.track.as_mut().expect("track just assigned")
    }

    /// Name-lookup wrapper around [`Self::set_animation`].
    ///
    /// # Errors
    ///
    /// Returns [`AnimationNotFound`] if no animation with `name` exists
    /// in the skeleton's data.
    pub fn set_animation_by_name(
        &mut self,
        name: &str,
        loop_: bool,
    ) -> Result<&mut TrackEntry, AnimationNotFound> {
        let id = self
            .data
            .animations
            .iter()
            .position(|a| a.name == name)
            .map(|i| AnimationId(i as u16))
            .ok_or_else(|| AnimationNotFound(name.to_string()))?;
        Ok(self.set_animation(id, loop_))
    }

    /// Clear the currently-playing animation, leaving the skeleton at its
    /// last applied pose. The next `apply` is a no-op until `set_animation`
    /// is called again.
    pub fn clear_track(&mut self) {
        self.track = None;
    }

    /// Advance the track's time by `delta * time_scale` seconds.
    pub fn update(&mut self, delta: f32) {
        if let Some(t) = &mut self.track {
            t.last_time = t.time;
            t.time += delta * t.time_scale;
        }
    }

    /// Apply the current track to `skeleton`, pushing any event firings
    /// into `events`. Does nothing when no track is set.
    ///
    /// `events` is **not** cleared — callers drain/iterate it themselves,
    /// so a game loop can accumulate events across multiple tracks once
    /// Phase 4 lands multi-track support.
    pub fn apply(&self, skeleton: &mut Skeleton, events: &mut Vec<Event>) {
        let Some(track) = &self.track else { return };
        let animation = &self.data.animations[track.animation.index()];
        animation.apply(
            skeleton,
            track.last_time,
            track.time,
            track.loop_,
            events,
            track.alpha,
            track.blend,
            MixDirection::In,
        );
    }
}

/// Error returned by [`AnimationState::set_animation_by_name`] when the
/// requested name isn't in the skeleton's data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no animation named `{0}`")]
pub struct AnimationNotFound(pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Animation, BoneData, BoneId, CurveFrames, Timeline};

    fn one_bone_with_rotate() -> Arc<SkeletonData> {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        let mut anim = Animation::new("spin", 1.0);
        // Rotate from 0 at t=0 to 90 at t=1 linear.
        anim.timelines.push(Timeline::Rotate {
            bone: BoneId(0),
            curves: CurveFrames {
                frames: vec![0.0, 0.0, 1.0, 90.0],
                // Last frame's curve type must be STEPPED (spine-cpp's
                // CurveTimeline ctor sets `_curves[frameCount - 1] = STEPPED`
                // as a safety against `frames[i + ENTRIES]` reads walking
                // past the last frame).
                curves: vec![
                    crate::animation::CURVE_LINEAR as f32,
                    crate::animation::CURVE_STEPPED as f32,
                ],
            },
        });
        sd.animations.push(anim);
        Arc::new(sd)
    }

    #[test]
    fn update_then_apply_drives_bone_rotation() {
        let data = one_bone_with_rotate();
        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();

        let mut state = AnimationState::new(Arc::clone(&data));
        state.set_animation_by_name("spin", false).unwrap();

        let mut events = Vec::new();
        state.update(0.5);
        state.apply(&mut sk, &mut events);
        assert!((sk.bones[0].rotation - 45.0).abs() < 1e-6);

        state.update(0.5);
        state.apply(&mut sk, &mut events);
        assert!((sk.bones[0].rotation - 90.0).abs() < 1e-6);
    }

    #[test]
    fn looped_animation_wraps_modulo_duration() {
        let data = one_bone_with_rotate();
        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();

        let mut state = AnimationState::new(Arc::clone(&data));
        state.set_animation_by_name("spin", true).unwrap();

        // Advance past the 1s duration → wraps to 0.5s → rotation 45°.
        let mut events = Vec::new();
        state.update(1.5);
        state.apply(&mut sk, &mut events);
        assert!((sk.bones[0].rotation - 45.0).abs() < 1e-6);
    }

    #[test]
    fn set_animation_by_name_missing_errors() {
        let data = one_bone_with_rotate();
        let mut state = AnimationState::new(data);
        let err = state.set_animation_by_name("nope", false).unwrap_err();
        assert_eq!(err, AnimationNotFound("nope".into()));
        assert!(state.track().is_none());
    }

    #[test]
    fn clear_track_stops_applying() {
        let data = one_bone_with_rotate();
        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();

        let mut state = AnimationState::new(Arc::clone(&data));
        state.set_animation_by_name("spin", false).unwrap();
        state.update(0.5);

        let mut events = Vec::new();
        state.clear_track();
        sk.bones[0].rotation = 12.34;
        state.apply(&mut sk, &mut events);
        assert!((sk.bones[0].rotation - 12.34).abs() < 1e-6);
    }
}
