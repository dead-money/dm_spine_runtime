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

//! Crossfade duration lookup for the [`AnimationState`][crate::animation::AnimationState].
//!
//! Ports `spine::AnimationStateData` — a (from, to) → mix-duration table with
//! a default fallback. Typical setup at load time: write per-transition
//! overrides for natural-looking animation swaps ("walk → idle ≈ 0.2s",
//! "walk → jump ≈ 0.1s"), falling back to [`Self::default_mix`] otherwise.

use std::collections::HashMap;
use std::sync::Arc;

use crate::data::{AnimationId, SkeletonData};

/// Stores mix (crossfade) durations to be applied when
/// [`AnimationState`][crate::animation::AnimationState] animations change.
#[derive(Debug, Clone)]
pub struct AnimationStateData {
    data: Arc<SkeletonData>,
    default_mix: f32,
    /// spine-cpp keys by `(Animation*, Animation*)` identity; the Rust port
    /// uses the (from, to) `AnimationId` pair. Semantics are identical.
    mixes: HashMap<(AnimationId, AnimationId), f32>,
}

impl AnimationStateData {
    /// Create an empty mix table for the given skeleton. All transitions
    /// use the default mix duration (initially 0) until overridden.
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        Self {
            data,
            default_mix: 0.0,
            mixes: HashMap::new(),
        }
    }

    /// Skeleton the mix table belongs to.
    #[must_use]
    pub fn data(&self) -> &Arc<SkeletonData> {
        &self.data
    }

    /// Fallback mix duration when no explicit override exists for a
    /// `(from, to)` pair. spine-cpp default: 0 seconds.
    #[must_use]
    pub fn default_mix(&self) -> f32 {
        self.default_mix
    }

    pub fn set_default_mix(&mut self, value: f32) {
        self.default_mix = value;
    }

    /// Override the mix duration for `from → to`.
    pub fn set_mix(&mut self, from: AnimationId, to: AnimationId, duration: f32) {
        self.mixes.insert((from, to), duration);
    }

    /// Name-lookup wrapper around [`Self::set_mix`].
    ///
    /// # Errors
    ///
    /// Returns [`MixAnimationNotFound`] if either name doesn't match an
    /// animation in the owning skeleton data.
    pub fn set_mix_by_name(
        &mut self,
        from_name: &str,
        to_name: &str,
        duration: f32,
    ) -> Result<(), MixAnimationNotFound> {
        let from = self.find(from_name)?;
        let to = self.find(to_name)?;
        self.set_mix(from, to, duration);
        Ok(())
    }

    /// Look up the mix duration for a `from → to` transition, falling back
    /// to [`Self::default_mix`].
    #[must_use]
    pub fn mix(&self, from: AnimationId, to: AnimationId) -> f32 {
        *self.mixes.get(&(from, to)).unwrap_or(&self.default_mix)
    }

    /// Remove every override and reset the default mix to 0 (matches
    /// `spine::AnimationStateData::clear`).
    pub fn clear(&mut self) {
        self.mixes.clear();
        self.default_mix = 0.0;
    }

    fn find(&self, name: &str) -> Result<AnimationId, MixAnimationNotFound> {
        self.data
            .animations
            .iter()
            .position(|a| a.name == name)
            .map(|i| AnimationId(i as u16))
            .ok_or_else(|| MixAnimationNotFound(name.to_string()))
    }
}

/// Error returned by [`AnimationStateData::set_mix_by_name`] when the
/// named animation isn't in the owning skeleton data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no animation named `{0}`")]
pub struct MixAnimationNotFound(pub String);

#[cfg(test)]
#[allow(clippy::float_cmp)] // small-int comparisons against literal defaults
mod tests {
    use super::*;
    use crate::data::Animation;

    fn with_animations(names: &[&str]) -> Arc<SkeletonData> {
        let mut sd = SkeletonData::default();
        for name in names {
            sd.animations.push(Animation::new(*name, 1.0));
        }
        Arc::new(sd)
    }

    #[test]
    fn default_mix_is_zero() {
        let sd = with_animations(&["a", "b"]);
        let data = AnimationStateData::new(sd);
        assert_eq!(data.default_mix(), 0.0);
        assert_eq!(data.mix(AnimationId(0), AnimationId(1)), 0.0);
    }

    #[test]
    fn default_mix_overridable() {
        let sd = with_animations(&["a", "b"]);
        let mut data = AnimationStateData::new(sd);
        data.set_default_mix(0.25);
        assert_eq!(data.mix(AnimationId(0), AnimationId(1)), 0.25);
    }

    #[test]
    fn per_pair_override_beats_default() {
        let sd = with_animations(&["walk", "idle", "run"]);
        let mut data = AnimationStateData::new(sd);
        data.set_default_mix(0.2);
        data.set_mix_by_name("walk", "idle", 0.5).unwrap();
        // walk → idle uses the override, run → idle uses the default.
        assert_eq!(data.mix(AnimationId(0), AnimationId(1)), 0.5);
        assert_eq!(data.mix(AnimationId(2), AnimationId(1)), 0.2);
    }

    #[test]
    fn set_mix_by_name_errors_on_missing() {
        let sd = with_animations(&["a"]);
        let mut data = AnimationStateData::new(sd);
        assert_eq!(
            data.set_mix_by_name("a", "nope", 0.1).unwrap_err(),
            MixAnimationNotFound("nope".into())
        );
    }

    #[test]
    fn clear_resets_default_and_overrides() {
        let sd = with_animations(&["a", "b"]);
        let mut data = AnimationStateData::new(sd);
        data.set_default_mix(0.3);
        data.set_mix(AnimationId(0), AnimationId(1), 0.9);
        data.clear();
        assert_eq!(data.default_mix(), 0.0);
        assert_eq!(data.mix(AnimationId(0), AnimationId(1)), 0.0);
    }
}
