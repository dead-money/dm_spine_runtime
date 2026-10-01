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

//! Mix durations for [`AnimationState`][crate::animation::AnimationState]
//! transitions.

use std::collections::HashMap;
use std::sync::Arc;

use crate::data::{AnimationId, SkeletonData};

/// Crossfade durations, in seconds, per `(from, to)` animation pair, with a
/// default for pairs that have none.
#[derive(Debug, Clone)]
pub struct AnimationStateData {
    data: Arc<SkeletonData>,
    default_mix: f32,
    mixes: HashMap<(AnimationId, AnimationId), f32>,
}

impl AnimationStateData {
    /// An empty table whose default mix is 0.
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        Self {
            data,
            default_mix: 0.0,
            mixes: HashMap::new(),
        }
    }

    /// Skeleton data the animation ids refer to.
    #[must_use]
    pub fn data(&self) -> &Arc<SkeletonData> {
        &self.data
    }

    /// Mix duration for pairs without an override. Starts at 0.
    #[must_use]
    pub fn default_mix(&self) -> f32 {
        self.default_mix
    }

    /// Sets the mix duration, in seconds, for pairs without an override.
    pub fn set_default_mix(&mut self, value: f32) {
        self.default_mix = value;
    }

    /// Sets the mix duration for `from` to `to`.
    pub fn set_mix(&mut self, from: AnimationId, to: AnimationId, duration: f32) {
        self.mixes.insert((from, to), duration);
    }

    /// [`Self::set_mix`] by animation name.
    ///
    /// # Errors
    ///
    /// [`MixAnimationNotFound`] if either name isn't in the skeleton data.
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

    /// Mix duration for `from` to `to`, or [`Self::default_mix`].
    #[must_use]
    pub fn mix(&self, from: AnimationId, to: AnimationId) -> f32 {
        *self.mixes.get(&(from, to)).unwrap_or(&self.default_mix)
    }

    /// Removes every override and resets the default mix to 0.
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

/// [`AnimationStateData::set_mix_by_name`] was given an unknown animation
/// name.
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
