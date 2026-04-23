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

//! Multi-track `AnimationState` with crossfade, queuing, and listeners.
//!
//! Port target: `spine::AnimationState` in `spine-cpp`. The Rust port uses
//! a slab (`HashMap<EntryId, TrackEntry>`) instead of spine-cpp's object
//! pool so back-pointers (`mixing_from` / `mixing_to` / `next` /
//! `previous`) are typed index handles rather than raw pointers.
//!
//! # Sub-phases
//!
//! Landing this port incrementally:
//! - **4a** (current): `TrackEntry` data shape + entry slab + single-track
//!   `set_animation` / `update` / `apply` that matches Phase 3 semantics.
//! - **4b**: multi-track, queuing via `add_animation`, `clear_track(s)`.
//! - **4c**: mixing (`apply_mixing_from`, `update_mixing_from`).
//! - **4d**: `compute_hold` / `timeline_mode` + specialised rotate /
//!   attachment applies.
//! - **4e**: event queue, per-entry listeners, empty animations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::animation::{AnimationStateData, Event, MixBlend, MixDirection};
use crate::data::{AnimationId, SkeletonData};
use crate::skeleton::Skeleton;

/// Typed handle into the [`AnimationState`] entry slab. Identifies a
/// track entry for the lifetime of its state; dropped when its owning
/// state removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(u32);

/// State for the playback of an animation on one track.
///
/// One-to-one field correspondence with `spine::TrackEntry`'s private
/// members; back-pointers are [`EntryId`]s instead of raw pointers.
///
/// The many boolean flags (`loop_`, `hold_previous`, `reverse`,
/// `shortest_rotation`) match spine-cpp's layout — silencing clippy's
/// `struct_excessive_bools` rather than packing into bitflags keeps
/// direct field access working.
#[derive(Debug, Clone)]
#[non_exhaustive] // fields are still growing across 4b–4e
#[allow(clippy::struct_excessive_bools)]
pub struct TrackEntry {
    pub animation: AnimationId,

    /// Zero-based track number this entry belongs to.
    pub track_index: usize,

    /// Previous queued entry (`_previous` in spine-cpp). Set when the
    /// entry was enqueued behind another animation.
    pub previous: Option<EntryId>,
    /// Next entry queued behind this one.
    pub next: Option<EntryId>,
    /// While mixing, the entry we're crossfading *from*.
    pub mixing_from: Option<EntryId>,
    /// Inverse of [`Self::mixing_from`]: the entry crossfading to this one.
    pub mixing_to: Option<EntryId>,

    pub loop_: bool,
    pub hold_previous: bool,
    pub reverse: bool,
    pub shortest_rotation: bool,

    /// Event timelines for the mixing-out entry don't fire if
    /// `mix_time / mix_duration >= event_threshold`. Default 0.
    pub event_threshold: f32,
    /// Attachment timelines for the mixing-out entry don't fire if
    /// `mix_time / mix_duration >= mix_attachment_threshold`. Default 0.
    pub mix_attachment_threshold: f32,
    /// Attachment timelines on the mixing-in entry don't fire when
    /// `alpha < alpha_attachment_threshold`. Default 0. (Only read by
    /// the specialised attachment apply path in Phase 4d.)
    pub alpha_attachment_threshold: f32,
    /// `DrawOrder` timelines for the mixing-out entry don't fire if
    /// `mix_time / mix_duration >= mix_draw_order_threshold`. Default 0.
    pub mix_draw_order_threshold: f32,

    pub animation_start: f32,
    pub animation_end: f32,
    /// Time of last apply (exclusive lower bound for event firings).
    /// Defaults to `-1` so events on frame 0 fire the first time.
    pub animation_last: f32,
    pub next_animation_last: f32,

    pub delay: f32,
    pub track_time: f32,
    pub track_last: f32,
    pub next_track_last: f32,
    pub track_end: f32,
    pub time_scale: f32,

    pub alpha: f32,
    pub mix_time: f32,
    pub mix_duration: f32,
    pub interrupt_alpha: f32,
    pub total_alpha: f32,
    pub mix_blend: MixBlend,

    /// Per-timeline classification for mixing-from (`Subsequent` / `First` /
    /// `HoldSubsequent` / `HoldFirst` / `HoldMix`). Populated by Phase 4d's
    /// `compute_hold`; empty until then.
    pub timeline_mode: Vec<TimelineMode>,
    /// When `timeline_mode[i] == HoldMix`, the entry whose fade-out
    /// alpha contributes to timeline `i`'s hold blend.
    pub timeline_hold_mix: Vec<Option<EntryId>>,
    /// Scratch buffer for the shortest-rotation-path mixing (Phase 4d).
    pub timelines_rotation: Vec<f32>,
}

/// Classifies a timeline's role in a mix transition for
/// [`TrackEntry::timeline_mode`]. Drives which `MixBlend` + `alpha`
/// combination `apply_mixing_from` uses per timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineMode {
    /// Timeline's property is keyed by a lower track already — blend into
    /// the accumulated state as the mix-out ramps down.
    Subsequent,
    /// Timeline's property isn't keyed anywhere else — use `Setup` blend
    /// with the mix-out alpha so we're fading toward setup pose.
    First,
    /// Property is keyed by a later animation in the chain that has no
    /// mix; hold this entry's value at full `alphaHold` even as the
    /// named-mix ramp ramps down.
    HoldSubsequent,
    /// Like `HoldSubsequent` but blending toward setup instead.
    HoldFirst,
    /// Property handoff to a mixing-from entry that still has finite
    /// `mix_duration` — ramp down by its mix progress. Pairs with
    /// [`TrackEntry::timeline_hold_mix`] for the specific entry to poll.
    HoldMix,
}

impl TrackEntry {
    /// Fresh entry initialised to spine-cpp's `newTrackEntry` defaults.
    /// `mix_duration` is set by the caller (needs [`AnimationStateData`]).
    fn new(track_index: usize, animation: AnimationId, animation_end: f32, loop_: bool) -> Self {
        Self {
            animation,
            track_index,
            previous: None,
            next: None,
            mixing_from: None,
            mixing_to: None,
            loop_,
            hold_previous: false,
            reverse: false,
            shortest_rotation: false,
            event_threshold: 0.0,
            mix_attachment_threshold: 0.0,
            alpha_attachment_threshold: 0.0,
            mix_draw_order_threshold: 0.0,
            animation_start: 0.0,
            animation_end,
            animation_last: -1.0,
            next_animation_last: -1.0,
            delay: 0.0,
            track_time: 0.0,
            track_last: -1.0,
            next_track_last: -1.0,
            track_end: f32::MAX, // loop default; caller may tighten
            time_scale: 1.0,
            alpha: 1.0,
            mix_time: 0.0,
            mix_duration: 0.0,
            interrupt_alpha: 1.0,
            total_alpha: 0.0,
            mix_blend: MixBlend::Replace,
            timeline_mode: Vec::new(),
            timeline_hold_mix: Vec::new(),
            timelines_rotation: Vec::new(),
        }
    }

    /// `trackTime` mapped onto the animation's `[animation_start, animation_end]`
    /// range, accounting for looping. Ports `spine::TrackEntry::getAnimationTime`.
    #[must_use]
    pub fn animation_time(&self) -> f32 {
        if self.loop_ {
            let duration = self.animation_end - self.animation_start;
            if duration == 0.0 {
                return self.animation_start;
            }
            (self.track_time % duration) + self.animation_start
        } else {
            (self.track_time + self.animation_start).min(self.animation_end)
        }
    }

    /// One full loop's worth of animation time (for loop completion
    /// counting). Ports `spine::TrackEntry::getTrackComplete`.
    #[must_use]
    pub fn track_complete(&self) -> f32 {
        let duration = self.animation_end - self.animation_start;
        if duration != 0.0 {
            if self.loop_ {
                // Completion time of the next loop iteration.
                return duration * (1.0 + (self.track_time / duration).floor());
            }
            if self.track_time < duration {
                return duration;
            }
        }
        self.track_time
    }

    /// `true` once the entry has been applied at least once (matches
    /// spine-cpp's `wasApplied() = _nextTrackLast != -1`).
    #[must_use]
    #[allow(clippy::float_cmp)] // sentinel comparison, not numeric equality
    pub fn was_applied(&self) -> bool {
        self.next_track_last != -1.0
    }
}

/// Multi-track animation driver.
///
/// Phase 4a's shape: slab-backed entries, single `set_animation` call
/// that replaces whatever was on the track. Crossfade (4c) and queueing
/// (4b) layer on top.
#[derive(Debug)]
pub struct AnimationState {
    data: Arc<AnimationStateData>,
    /// All live track entries keyed by handle. Disposal removes entries
    /// when a track is cleared or mixing completes.
    entries: HashMap<EntryId, TrackEntry>,
    /// Per-track current entry. `tracks.len()` grows to cover the highest
    /// requested index. Tracks can be `None` (empty) when cleared.
    tracks: Vec<Option<EntryId>>,
    next_id: u32,
    /// Global time scaling — multiplies every track's `delta` in `update`.
    pub time_scale: f32,
    /// `true` when the track topology changed since the last `apply`;
    /// triggers `compute_hold` to re-classify every timeline (Phase 4d).
    animations_changed: bool,
}

impl AnimationState {
    /// Build an empty state bound to `data`'s skeleton + mix table.
    #[must_use]
    pub fn new(data: Arc<AnimationStateData>) -> Self {
        Self {
            data,
            entries: HashMap::new(),
            tracks: Vec::new(),
            next_id: 0,
            time_scale: 1.0,
            animations_changed: false,
        }
    }

    /// Peek at a track's current entry, or `None` when the track is empty.
    #[must_use]
    pub fn current(&self, track_index: usize) -> Option<&TrackEntry> {
        self.tracks
            .get(track_index)
            .and_then(|id| id.as_ref().and_then(|id| self.entries.get(id)))
    }

    /// Mutably access a track's current entry (for tweaking alpha,
    /// `time_scale`, etc. without restarting).
    pub fn current_mut(&mut self, track_index: usize) -> Option<&mut TrackEntry> {
        let id = (*self.tracks.get(track_index)?)?;
        self.entries.get_mut(&id)
    }

    /// Resolve a handle to its entry. Returns `None` after the entry has
    /// been disposed.
    #[must_use]
    pub fn entry(&self, id: EntryId) -> Option<&TrackEntry> {
        self.entries.get(&id)
    }

    /// Mutably resolve a handle. Use sparingly — most tweaks should go
    /// through [`Self::current_mut`] at the track level.
    pub fn entry_mut(&mut self, id: EntryId) -> Option<&mut TrackEntry> {
        self.entries.get_mut(&id)
    }

    /// Number of tracks (sparse — includes cleared slots).
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Immutable handle to the underlying mix table.
    #[must_use]
    pub fn data(&self) -> &Arc<AnimationStateData> {
        &self.data
    }

    /// Skeleton data this state reads animations from.
    #[must_use]
    pub fn skeleton_data(&self) -> &Arc<SkeletonData> {
        self.data.data()
    }

    // ----- Entry-slab plumbing --------------------------------------------

    fn alloc_entry(&mut self, entry: TrackEntry) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id += 1;
        self.entries.insert(id, entry);
        id
    }

    /// Drop an entry from the slab. Safe to call on an already-gone id.
    fn free_entry(&mut self, id: EntryId) {
        self.entries.remove(&id);
    }

    /// Expand `tracks` so `track_index` is a valid slot.
    fn expand_to_index(&mut self, track_index: usize) {
        while self.tracks.len() <= track_index {
            self.tracks.push(None);
        }
    }

    // ----- Public API: set/clear ------------------------------------------

    /// Replace the track's current animation, discarding any queued entries.
    ///
    /// Starts the animation from `track_time = 0` with the default
    /// properties `newTrackEntry` assigns. Returns an [`EntryId`] the
    /// caller can use to tweak per-entry settings via [`Self::entry_mut`].
    ///
    /// No crossfade yet — Phase 4c wires `mixing_from` here.
    pub fn set_animation(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        loop_: bool,
    ) -> EntryId {
        self.expand_to_index(track_index);

        // Phase 4b/4c will clean up any existing entry (dispose queued,
        // potentially chain mixing_from). For 4a, just replace.
        if let Some(old_id) = self.tracks[track_index].take() {
            self.free_entry(old_id);
        }

        let duration = self.skeleton_data().animations[animation.index()].duration;
        let entry = TrackEntry::new(track_index, animation, duration, loop_);
        let id = self.alloc_entry(entry);
        self.tracks[track_index] = Some(id);
        self.animations_changed = true;
        id
    }

    /// Name-lookup wrapper around [`Self::set_animation`].
    ///
    /// # Errors
    ///
    /// Returns [`AnimationNotFound`] if no animation with `name` exists.
    pub fn set_animation_by_name(
        &mut self,
        track_index: usize,
        name: &str,
        loop_: bool,
    ) -> Result<EntryId, AnimationNotFound> {
        let id = self
            .skeleton_data()
            .animations
            .iter()
            .position(|a| a.name == name)
            .map(|i| AnimationId(i as u16))
            .ok_or_else(|| AnimationNotFound(name.to_string()))?;
        Ok(self.set_animation(track_index, id, loop_))
    }

    /// Remove the entry on `track_index`, leaving the skeleton at its
    /// current pose. Matches `spine::AnimationState::clearTrack` minus
    /// the event queue (Phase 4e).
    pub fn clear_track(&mut self, track_index: usize) {
        if track_index >= self.tracks.len() {
            return;
        }
        if let Some(id) = self.tracks[track_index].take() {
            self.free_entry(id);
            self.animations_changed = true;
        }
    }

    /// Remove every track's entries.
    pub fn clear_tracks(&mut self) {
        for i in 0..self.tracks.len() {
            self.clear_track(i);
        }
        self.tracks.clear();
    }

    // ----- Update + apply (Phase 4a subset) -------------------------------

    /// Advance every active track by `delta` seconds.
    ///
    /// Phase 4a: single-track simple path (delay decrement + `track_time`
    /// bump). Phase 4b adds queued-animation promotion and `mixing_from`
    /// decay.
    pub fn update(&mut self, delta: f32) {
        let delta = delta * self.time_scale;
        for i in 0..self.tracks.len() {
            let Some(id) = self.tracks[i] else { continue };
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };

            entry.animation_last = entry.next_animation_last;
            entry.track_last = entry.next_track_last;

            let mut current_delta = delta * entry.time_scale;
            if entry.delay > 0.0 {
                entry.delay -= current_delta;
                if entry.delay > 0.0 {
                    continue;
                }
                current_delta = -entry.delay;
                entry.delay = 0.0;
            }

            // Phase 4b will insert the queued-next promotion + clear-on-end
            // logic here. For 4a, just bump track_time.
            entry.track_time += current_delta;
        }
    }

    /// Apply every track's current animation to `skeleton`, pushing event
    /// firings into `events`.
    ///
    /// Phase 4a: no mixing, no event queue. Each track's animation runs
    /// top-to-bottom with `MixBlend::First` on track 0 (so Setup-pose
    /// defaults kick in) and `entry.mix_blend` on higher tracks. Without
    /// `applyMixingFrom`, overlapping property writes are "last wins".
    pub fn apply(&self, skeleton: &mut Skeleton, events: &mut Vec<Event>) {
        for i in 0..self.tracks.len() {
            let Some(id) = self.tracks[i] else { continue };
            let Some(entry) = self.entries.get(&id) else {
                continue;
            };
            if entry.delay > 0.0 {
                continue;
            }

            let blend = if i == 0 {
                MixBlend::First
            } else {
                entry.mix_blend
            };

            let animation = &self.skeleton_data().animations[entry.animation.index()];
            let animation_last = entry.animation_last;
            let animation_time = entry.animation_time();

            for tl in &animation.timelines {
                tl.apply(
                    skeleton,
                    animation_last,
                    animation_time,
                    events,
                    entry.alpha,
                    blend,
                    MixDirection::In,
                );
            }

            // next_{animation,track}_last are normally written back inside
            // apply() in spine-cpp; with `&self` here we defer that to
            // `advance_last_applied` (called from `apply_and_advance`). Phase
            // 4b revisits this with `&mut self` once the queue machinery
            // needs it.
        }
    }

    /// Convenience: apply then record the last-applied times so events
    /// don't refire. Until Phase 4e's event queue lands, callers who want
    /// correct looping event firings should use this pair.
    pub fn apply_and_advance(&mut self, skeleton: &mut Skeleton, events: &mut Vec<Event>) {
        // Snapshot per-entry state before the (`&self`) apply, then write
        // back nextAnimationLast/nextTrackLast afterwards.
        self.apply(skeleton, events);
        for id_slot in &self.tracks {
            let Some(id) = id_slot else { continue };
            let Some(entry) = self.entries.get_mut(id) else {
                continue;
            };
            entry.next_animation_last = entry.animation_time();
            entry.next_track_last = entry.track_time;
        }
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

    fn one_bone_rotate() -> Arc<SkeletonData> {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        let mut anim = Animation::new("spin", 1.0);
        anim.timelines.push(Timeline::Rotate {
            bone: BoneId(0),
            curves: CurveFrames {
                frames: vec![0.0, 0.0, 1.0, 90.0],
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
    fn set_animation_creates_entry() {
        let sd = one_bone_rotate();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);
        let id = state.set_animation(0, AnimationId(0), false);
        assert_eq!(state.current(0).unwrap().animation, AnimationId(0));
        assert_eq!(state.entry(id).unwrap().track_index, 0);
    }

    #[test]
    fn clear_track_drops_entry() {
        let sd = one_bone_rotate();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);
        let id = state.set_animation(0, AnimationId(0), false);
        state.clear_track(0);
        assert!(state.current(0).is_none());
        assert!(state.entry(id).is_none());
    }

    #[test]
    fn update_then_apply_drives_bone_rotation() {
        let sd = one_bone_rotate();
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);
        state.set_animation(0, AnimationId(0), false);

        let mut events = Vec::new();
        state.update(0.5);
        state.apply(&mut sk, &mut events);
        assert!((sk.bones[0].rotation - 45.0).abs() < 1e-6);
    }

    #[test]
    fn set_animation_by_name_missing_errors() {
        let sd = one_bone_rotate();
        let state_data = Arc::new(AnimationStateData::new(sd));
        let mut state = AnimationState::new(state_data);
        let err = state.set_animation_by_name(0, "nope", false).unwrap_err();
        assert_eq!(err, AnimationNotFound("nope".into()));
    }

    #[test]
    fn multi_track_slots_expand() {
        let sd = one_bone_rotate();
        let state_data = Arc::new(AnimationStateData::new(sd));
        let mut state = AnimationState::new(state_data);
        state.set_animation(0, AnimationId(0), false);
        state.set_animation(2, AnimationId(0), true);
        assert_eq!(state.track_count(), 3);
        assert!(state.current(0).is_some());
        assert!(state.current(1).is_none());
        assert!(state.current(2).is_some());
    }
}
