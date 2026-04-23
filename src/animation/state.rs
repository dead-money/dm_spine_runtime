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

    /// Replace the track's current animation, discarding any queued
    /// entries.
    ///
    /// When an existing entry is current:
    /// - If it was never applied (`next_track_last == -1`), it's dropped
    ///   without marking an interrupt — matches spine-cpp's "don't mix
    ///   from an entry that was never applied" rule.
    /// - Otherwise the entry is chained as `mixing_from` on the new entry
    ///   so [`apply`][Self::apply] crossfades between them (Phase 4c).
    ///
    /// Returns an [`EntryId`] the caller can keep to mutate per-entry
    /// settings via [`Self::entry_mut`].
    pub fn set_animation(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        loop_: bool,
    ) -> EntryId {
        self.expand_to_index(track_index);

        let mut interrupt = true;
        // "Last" for mix_duration lookup. Starts as the current entry,
        // then drops into its mixing_from when the current was never
        // applied so the mix comes from the most-recently-applied source.
        let mut last_for_mix: Option<EntryId> = self.tracks[track_index];

        if let Some(current_id) = self.tracks[track_index] {
            let never_applied = self
                .entries
                .get(&current_id)
                .is_some_and(|e| !e.was_applied());
            if never_applied {
                let mixing_from = self.entries.get(&current_id).and_then(|e| e.mixing_from);
                self.tracks[track_index] = mixing_from;
                // queue->interrupt(current) + queue->end(current) land in 4e.
                self.clear_next(current_id);
                self.dispose_entry_chain(current_id);
                last_for_mix = mixing_from;
                interrupt = false;
            } else {
                self.clear_next(current_id);
            }
        }

        let entry_id = self.new_track_entry(track_index, animation, loop_, last_for_mix);
        self.set_current(track_index, entry_id, interrupt);
        entry_id
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

    /// Queue an animation to play `delay` seconds after the current (or
    /// last-queued) animation on `track_index`. If `delay <= 0`, the
    /// start time is computed relative to the previous entry's
    /// completion time minus the new entry's mix duration.
    ///
    /// Ports `spine::AnimationState::addAnimation`.
    pub fn add_animation(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        loop_: bool,
        delay: f32,
    ) -> EntryId {
        self.expand_to_index(track_index);

        // Walk to the tail of the queue for this track.
        let mut last: Option<EntryId> = self.tracks[track_index];
        while let Some(id) = last {
            match self.entries.get(&id).and_then(|e| e.next) {
                Some(next_id) => last = Some(next_id),
                None => break,
            }
        }

        let entry_id = self.new_track_entry(track_index, animation, loop_, last);

        let mut delay = delay;
        if let Some(last_id) = last {
            // Queue behind `last`.
            if let Some(last_entry) = self.entries.get_mut(&last_id) {
                last_entry.next = Some(entry_id);
            }
            if let Some(new_entry) = self.entries.get_mut(&entry_id) {
                new_entry.previous = Some(last_id);
            }
            if delay <= 0.0 {
                let complete = self
                    .entries
                    .get(&last_id)
                    .map_or(0.0, TrackEntry::track_complete);
                let mix = self.entries.get(&entry_id).map_or(0.0, |e| e.mix_duration);
                delay = (delay + complete - mix).max(0.0);
            }
        } else {
            // Empty track: promote directly.
            self.set_current(track_index, entry_id, true);
            if delay < 0.0 {
                delay = 0.0;
            }
        }

        if let Some(new_entry) = self.entries.get_mut(&entry_id) {
            new_entry.delay = delay;
        }
        entry_id
    }

    /// Name-lookup wrapper around [`Self::add_animation`].
    ///
    /// # Errors
    ///
    /// Returns [`AnimationNotFound`] if no animation with `name` exists.
    pub fn add_animation_by_name(
        &mut self,
        track_index: usize,
        name: &str,
        loop_: bool,
        delay: f32,
    ) -> Result<EntryId, AnimationNotFound> {
        let id = self
            .skeleton_data()
            .animations
            .iter()
            .position(|a| a.name == name)
            .map(|i| AnimationId(i as u16))
            .ok_or_else(|| AnimationNotFound(name.to_string()))?;
        Ok(self.add_animation(track_index, id, loop_, delay))
    }

    /// Remove the entry on `track_index`, leaving the skeleton at its
    /// current pose. Matches `spine::AnimationState::clearTrack` minus
    /// the event queue (Phase 4e).
    pub fn clear_track(&mut self, track_index: usize) {
        if track_index >= self.tracks.len() {
            return;
        }
        let Some(current_id) = self.tracks[track_index].take() else {
            return;
        };

        // Walk down the mixing_from chain and dispose each entry.
        self.clear_next(current_id);
        let mut entry_id = Some(current_id);
        while let Some(id) = entry_id {
            let from_id = self.entries.get(&id).and_then(|e| e.mixing_from);
            // Break the mixing_from/mixing_to link before disposing.
            if let Some(e) = self.entries.get_mut(&id) {
                e.mixing_from = None;
                e.mixing_to = None;
            }
            entry_id = from_id;
        }
        // Now dispose the whole chain.
        self.dispose_entry_chain(current_id);
        self.animations_changed = true;
    }

    /// Remove every track's entries.
    pub fn clear_tracks(&mut self) {
        for i in 0..self.tracks.len() {
            self.clear_track(i);
        }
        self.tracks.clear();
    }

    // ----- set/queue helpers ---------------------------------------------

    /// Hook `current` into `_tracks[index]`, chaining any previous entry
    /// as `mixing_from` for a crossfade. Matches `spine::AnimationState::setCurrent`.
    fn set_current(&mut self, index: usize, current: EntryId, interrupt: bool) {
        let from = self.tracks[index];
        self.tracks[index] = Some(current);

        // Always clear `previous` on the new current.
        if let Some(e) = self.entries.get_mut(&current) {
            e.previous = None;
        }

        if let Some(from_id) = from {
            if interrupt {
                // queue->interrupt(from) lands in Phase 4e.
            }

            // Link mixing_from ↔ mixing_to.
            let from_mix_time;
            let from_mix_duration;
            let from_mixing_from;
            {
                let Some(from_entry) = self.entries.get_mut(&from_id) else {
                    self.animations_changed = true;
                    return;
                };
                from_entry.timelines_rotation.clear();
                from_mix_time = from_entry.mix_time;
                from_mix_duration = from_entry.mix_duration;
                from_mixing_from = from_entry.mixing_from;
            }
            if let Some(e) = self.entries.get_mut(&current) {
                e.mixing_from = Some(from_id);
                e.mix_time = 0.0;
                // Store interrupted mix percentage per spine-cpp.
                if from_mixing_from.is_some() && from_mix_duration > 0.0 {
                    e.interrupt_alpha *= (from_mix_time / from_mix_duration).min(1.0);
                }
            }
            if let Some(from_entry) = self.entries.get_mut(&from_id) {
                from_entry.mixing_to = Some(current);
            }
        }
        self.animations_changed = true;
    }

    /// Build a new `TrackEntry` and register it in the slab, returning
    /// its handle. Matches `spine::AnimationState::newTrackEntry`.
    fn new_track_entry(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        loop_: bool,
        last: Option<EntryId>,
    ) -> EntryId {
        let duration = self.skeleton_data().animations[animation.index()].duration;
        let mut entry = TrackEntry::new(track_index, animation, duration, loop_);
        // mix_duration defaults to 0 when there's no predecessor, else
        // pulled from the AnimationStateData's (last_anim → new_anim) lookup.
        if let Some(last_id) = last
            && let Some(last_entry) = self.entries.get(&last_id)
        {
            entry.mix_duration = self.data.mix(last_entry.animation, animation);
        }
        self.alloc_entry(entry)
    }

    /// Dispose every entry queued behind `entry_id` (via `next` pointers).
    /// Matches `spine::AnimationState::clearNext`.
    fn clear_next(&mut self, entry_id: EntryId) {
        // Walk the `next` chain, disposing each.
        let mut next = self.entries.get(&entry_id).and_then(|e| e.next);
        while let Some(next_id) = next {
            let after = self.entries.get(&next_id).and_then(|e| e.next);
            // queue->dispose(next) lands in Phase 4e.
            self.free_entry(next_id);
            next = after;
        }
        if let Some(e) = self.entries.get_mut(&entry_id) {
            e.next = None;
        }
    }

    /// Dispose an entry and its entire `mixing_from` chain. Used on
    /// `clear_track` and on the "never-applied" replacement path in
    /// `set_animation`.
    fn dispose_entry_chain(&mut self, entry_id: EntryId) {
        let mut cur = Some(entry_id);
        while let Some(id) = cur {
            let from = self.entries.get(&id).and_then(|e| e.mixing_from);
            self.free_entry(id);
            cur = from;
        }
    }

    // ----- Update + apply -------------------------------------------------

    /// Advance every active track by `delta` seconds.
    ///
    /// Ports `spine::AnimationState::update`. Handles:
    /// - Per-entry delay countdown.
    /// - Promotion of queued `next` entries when their delay elapses
    ///   (chains `mix_time` advances on any `mixing_from` on the new current).
    /// - Track clearing when `track_end` is reached.
    /// - Mixing-from decay via `update_mixing_from` (Phase 4c fills the body).
    pub fn update(&mut self, delta: f32) {
        let delta = delta * self.time_scale;
        for i in 0..self.tracks.len() {
            let Some(current_id) = self.tracks[i] else {
                continue;
            };

            // Snapshot before any mutation: spine-cpp's
            // `animation_last = next_animation_last; track_last = next_track_last;`
            let time_scale;
            let next_id;
            let track_last;
            let track_end;
            let mixing_from;
            {
                let Some(entry) = self.entries.get_mut(&current_id) else {
                    continue;
                };
                entry.animation_last = entry.next_animation_last;
                entry.track_last = entry.next_track_last;
                time_scale = entry.time_scale;
                next_id = entry.next;
                track_last = entry.track_last;
                track_end = entry.track_end;
                mixing_from = entry.mixing_from;
            }

            let mut current_delta = delta * time_scale;
            {
                let Some(entry) = self.entries.get_mut(&current_id) else {
                    continue;
                };
                if entry.delay > 0.0 {
                    entry.delay -= current_delta;
                    if entry.delay > 0.0 {
                        continue;
                    }
                    current_delta = -entry.delay;
                    entry.delay = 0.0;
                }
            }

            // Queued-animation promotion.
            if let Some(next_id) = next_id {
                let next_delay = self.entries.get(&next_id).map_or(0.0, |n| n.delay);
                let next_time = track_last - next_delay;
                if next_time >= 0.0 {
                    if let Some(next_entry) = self.entries.get_mut(&next_id) {
                        next_entry.delay = 0.0;
                        let step = if time_scale == 0.0 {
                            0.0
                        } else {
                            (next_time / time_scale + delta) * next_entry.time_scale
                        };
                        next_entry.track_time += step;
                    }
                    if let Some(entry) = self.entries.get_mut(&current_id) {
                        entry.track_time += current_delta;
                    }
                    self.set_current(i, next_id, true);

                    // Bump mix_time on the promoted entry and its mixing_from chain.
                    let mut walk = Some(next_id);
                    while let Some(w_id) = walk {
                        let from = {
                            let Some(w) = self.entries.get_mut(&w_id) else {
                                break;
                            };
                            w.mix_time += delta;
                            w.mixing_from
                        };
                        walk = from;
                    }
                    continue;
                }
            } else if track_last >= track_end && mixing_from.is_none() {
                // No next + track_end reached + no mixing_from → clear the track.
                self.tracks[i] = None;
                // queue->end(currentP) lands in Phase 4e.
                self.clear_next(current_id);
                self.dispose_entry_chain(current_id);
                self.animations_changed = true;
                continue;
            }

            // Decay any mixing_from chain.
            if mixing_from.is_some() {
                let finished = self.update_mixing_from(current_id, delta);
                if finished {
                    let chain_root = {
                        let Some(e) = self.entries.get_mut(&current_id) else {
                            continue;
                        };
                        e.mixing_from.take()
                    };
                    if let Some(from_id) = chain_root {
                        if let Some(f) = self.entries.get_mut(&from_id) {
                            f.mixing_to = None;
                        }
                        self.dispose_entry_chain(from_id);
                    }
                }
            }

            if let Some(entry) = self.entries.get_mut(&current_id) {
                entry.track_time += current_delta;
            }
        }
    }

    /// Advance a mixing-from chain's mix times. Returns `true` when the
    /// entire chain has completed its crossfade and the caller can drop
    /// the chain.
    ///
    /// Ports `spine::AnimationState::updateMixingFrom`. Walks the chain
    /// bottom-up recursively, copies `next_animation_last` +
    /// `next_track_last` into their counterparts, and either closes the
    /// mix (when the outer entry's `mix_time` has reached `mix_duration`)
    /// or bumps `from.track_time` + `to.mix_time`.
    #[allow(clippy::float_cmp)] // sentinel comparison (to_next_track_last != -1)
    fn update_mixing_from(&mut self, to: EntryId, delta: f32) -> bool {
        let Some(from) = self.entries.get(&to).and_then(|e| e.mixing_from) else {
            return true;
        };

        let finished = self.update_mixing_from(from, delta);

        // Copy spine-cpp's "animation_last = next_animation_last; track_last
        // = next_track_last;" step on the `from` entry.
        if let Some(from_entry) = self.entries.get_mut(&from) {
            from_entry.animation_last = from_entry.next_animation_last;
            from_entry.track_last = from_entry.next_track_last;
        }

        let (to_next_track_last, to_mix_time, to_mix_duration) =
            self.entries.get(&to).map_or((-1.0, 0.0, 0.0), |e| {
                (e.next_track_last, e.mix_time, e.mix_duration)
            });

        if to_next_track_last != -1.0 && to_mix_time >= to_mix_duration {
            let (from_total_alpha, from_interrupt_alpha, from_mixing_from) =
                self.entries.get(&from).map_or((0.0, 1.0, None), |e| {
                    (e.total_alpha, e.interrupt_alpha, e.mixing_from)
                });
            if from_total_alpha == 0.0 || to_mix_duration == 0.0 {
                if let Some(to_entry) = self.entries.get_mut(&to) {
                    to_entry.mixing_from = from_mixing_from;
                    to_entry.interrupt_alpha = from_interrupt_alpha;
                }
                if let Some(mf) = from_mixing_from
                    && let Some(mf_entry) = self.entries.get_mut(&mf)
                {
                    mf_entry.mixing_to = Some(to);
                }
                // queue->end(from) lands in Phase 4e — entry is disposed by
                // the caller (update or set_animation) via dispose_entry_chain.
            }
            return finished;
        }

        // Advance mix timers.
        if let Some(from_entry) = self.entries.get_mut(&from) {
            from_entry.track_time += delta * from_entry.time_scale;
        }
        if let Some(to_entry) = self.entries.get_mut(&to) {
            to_entry.mix_time += delta;
        }

        false
    }

    /// Apply the mixing-from chain for `to`, returning the alpha
    /// multiplier that should scale the incoming animation.
    ///
    /// Ports `spine::AnimationState::applyMixingFrom`. Phase 4c uses the
    /// simple path: every timeline on `from` applies with `MixBlend::Setup`
    /// (or `Add` when the top blend is Add) scaled by
    /// `alpha_hold * (1 - mix)`. Phase 4d replaces this with per-timeline
    /// `timeline_mode` specialisation (HoldSubsequent/HoldFirst/HoldMix)
    /// for cleaner blends against lower tracks.
    #[allow(clippy::float_cmp)] // tag comparisons (mix_duration == 0, blend == Setup/First)
    fn apply_mixing_from(
        &mut self,
        to: EntryId,
        skeleton: &mut Skeleton,
        mut blend: MixBlend,
        events: &mut Vec<Event>,
    ) -> f32 {
        let Some(from) = self.entries.get(&to).and_then(|e| e.mixing_from) else {
            return 1.0;
        };

        // Recurse down the chain first.
        if self
            .entries
            .get(&from)
            .is_some_and(|e| e.mixing_from.is_some())
        {
            self.apply_mixing_from(from, skeleton, blend, events);
        }

        let (mix_time, mix_duration) = self
            .entries
            .get(&to)
            .map_or((0.0, 0.0), |e| (e.mix_time, e.mix_duration));
        let mix = if mix_duration == 0.0 {
            if blend == MixBlend::First {
                blend = MixBlend::Setup;
            }
            1.0
        } else {
            let raw = mix_time / mix_duration;
            let m = raw.min(1.0);
            if blend != MixBlend::First {
                blend = self
                    .entries
                    .get(&from)
                    .map_or(MixBlend::Replace, |e| e.mix_blend);
            }
            m
        };

        let (
            from_alpha,
            from_event_threshold,
            from_reverse,
            from_animation_last,
            from_animation_time,
            from_animation_id,
            from_duration,
        ) = {
            let Some(from_entry) = self.entries.get(&from) else {
                return mix;
            };
            let anim = &self.skeleton_data().animations[from_entry.animation.index()];
            (
                from_entry.alpha,
                from_entry.event_threshold,
                from_entry.reverse,
                from_entry.animation_last,
                from_entry.animation_time(),
                from_entry.animation,
                anim.duration,
            )
        };
        let to_interrupt_alpha = self.entries.get(&to).map_or(1.0, |e| e.interrupt_alpha);
        let alpha_hold = from_alpha * to_interrupt_alpha;
        let alpha_mix = alpha_hold * (1.0 - mix);

        // total_alpha is accumulated by Phase 4d's HoldMix branch; the
        // simple path just resets it here.
        if let Some(from_entry) = self.entries.get_mut(&from) {
            from_entry.total_alpha = 0.0;
        }

        let apply_time = if from_reverse {
            from_duration - from_animation_time
        } else {
            from_animation_time
        };
        let capture_events = !from_reverse && mix < from_event_threshold;
        let mut scratch: Vec<Event> = Vec::new();
        let ev_ref: &mut Vec<Event> = if capture_events {
            &mut *events
        } else {
            &mut scratch
        };

        let (direction, effective_blend) = if blend == MixBlend::Add {
            (MixDirection::Out, blend)
        } else {
            (MixDirection::Out, MixBlend::Setup)
        };

        let timelines = self.skeleton_data().animations[from_animation_id.index()]
            .timelines
            .clone();
        for tl in &timelines {
            tl.apply(
                skeleton,
                from_animation_last,
                apply_time,
                ev_ref,
                alpha_mix,
                effective_blend,
                direction,
            );
        }

        if let Some(from_entry) = self.entries.get_mut(&from) {
            from_entry.next_animation_last = from_animation_time;
            from_entry.next_track_last = from_entry.track_time;
        }

        mix
    }

    /// Apply every track's current animation to `skeleton`, crossfading
    /// from any `mixing_from` chain as needed, and pushing event firings
    /// into `events`.
    ///
    /// Ports `spine::AnimationState::apply`. Phase 4c form: simple
    /// mixing path — every timeline on the outgoing entry applies with
    /// `MixBlend::Setup` and `alpha * (1 - mix_progress)`. Phase 4d
    /// introduces the per-timeline `timeline_mode` specialisation
    /// (HoldSubsequent/HoldFirst/HoldMix) for cleaner crossfades when
    /// lower tracks also key the same property.
    pub fn apply(&mut self, skeleton: &mut Skeleton, events: &mut Vec<Event>) {
        // Phase 4d hook: re-classify timelines when the track topology
        // changed. Stubbed for 4c — simple apply path doesn't use
        // timeline_mode.
        self.animations_changed = false;

        for i in 0..self.tracks.len() {
            let Some(current_id) = self.tracks[i] else {
                continue;
            };
            let (delay, alpha, mix_blend, mixing_from) = {
                let Some(entry) = self.entries.get(&current_id) else {
                    continue;
                };
                (entry.delay, entry.alpha, entry.mix_blend, entry.mixing_from)
            };
            if delay > 0.0 {
                continue;
            }

            let mut blend = if i == 0 { MixBlend::First } else { mix_blend };

            // Apply the mixing-from chain first; returns the alpha
            // multiplier to scale the incoming timeline.
            let mut effective_alpha = alpha;
            if mixing_from.is_some() {
                effective_alpha *= self.apply_mixing_from(current_id, skeleton, blend, events);
            } else {
                // spine-cpp: track_time >= track_end && next is None →
                // fade toward setup on the last apply before the track
                // is cleared.
                let (track_time, track_end, has_next) = self
                    .entries
                    .get(&current_id)
                    .map_or((0.0, 0.0, false), |e| {
                        (e.track_time, e.track_end, e.next.is_some())
                    });
                if track_time >= track_end && !has_next {
                    effective_alpha = 0.0;
                }
            }

            let (animation_last, animation_time, reverse, animation_id) = {
                let Some(entry) = self.entries.get(&current_id) else {
                    continue;
                };
                (
                    entry.animation_last,
                    entry.animation_time(),
                    entry.reverse,
                    entry.animation,
                )
            };
            let duration = self.skeleton_data().animations[animation_id.index()].duration;
            let apply_time = if reverse {
                duration - animation_time
            } else {
                animation_time
            };

            // On track 0 at full alpha, switch to First blend so setup
            // pose acts as the baseline. Add stays Add.
            if i == 0 && ((effective_alpha - 1.0).abs() < f32::EPSILON || blend == MixBlend::Add) {
                blend = MixBlend::First;
            }

            let mut scratch: Vec<Event> = Vec::new();
            // Clone timelines so the apply loop doesn't hold a borrow
            // through `self.skeleton_data()`.
            let timelines = self.skeleton_data().animations[animation_id.index()]
                .timelines
                .clone();
            let ev_ref: &mut Vec<Event> = if reverse { &mut scratch } else { &mut *events };

            for tl in &timelines {
                tl.apply(
                    skeleton,
                    animation_last,
                    apply_time,
                    ev_ref,
                    effective_alpha,
                    blend,
                    MixDirection::In,
                );
            }

            if let Some(entry) = self.entries.get_mut(&current_id) {
                entry.next_animation_last = animation_time;
                entry.next_track_last = entry.track_time;
            }
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

    fn two_anim_skeleton() -> Arc<SkeletonData> {
        let mut sd = SkeletonData::default();
        sd.bones.push(BoneData::new(BoneId(0), "root", None));
        let mut a = Animation::new("a", 1.0);
        a.timelines.push(Timeline::Rotate {
            bone: BoneId(0),
            curves: CurveFrames {
                frames: vec![0.0, 10.0, 1.0, 20.0],
                curves: vec![
                    crate::animation::CURVE_LINEAR as f32,
                    crate::animation::CURVE_STEPPED as f32,
                ],
            },
        });
        sd.animations.push(a);
        let mut b = Animation::new("b", 1.0);
        b.timelines.push(Timeline::Rotate {
            bone: BoneId(0),
            curves: CurveFrames {
                frames: vec![0.0, 30.0, 1.0, 40.0],
                curves: vec![
                    crate::animation::CURVE_LINEAR as f32,
                    crate::animation::CURVE_STEPPED as f32,
                ],
            },
        });
        sd.animations.push(b);
        Arc::new(sd)
    }

    #[test]
    fn set_animation_replaces_previous_with_mixing_from() {
        let sd = two_anim_skeleton();
        let mut state_data = AnimationStateData::new(Arc::clone(&sd));
        state_data.set_default_mix(0.2);
        let state_data = Arc::new(state_data);
        let mut state = AnimationState::new(state_data);

        let a_id = state.set_animation(0, AnimationId(0), false);
        // Apply once so entry `a` is marked applied — otherwise set_animation's
        // "never applied" branch drops it without chaining mixing_from.
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let mut events = Vec::new();
        state.apply(&mut sk, &mut events);
        assert!(state.entry(a_id).unwrap().was_applied());

        let b_id = state.set_animation(0, AnimationId(1), false);
        let b = state.entry(b_id).unwrap();
        assert_eq!(b.animation, AnimationId(1));
        assert_eq!(b.mixing_from, Some(a_id));
        assert!((b.mix_duration - 0.2).abs() < 1e-6);
    }

    #[test]
    fn set_animation_on_never_applied_entry_drops_without_mixing() {
        let sd = two_anim_skeleton();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);

        let a_id = state.set_animation(0, AnimationId(0), false);
        // Replace immediately — `a` was never applied.
        let b_id = state.set_animation(0, AnimationId(1), false);
        assert!(state.entry(a_id).is_none(), "a should be disposed");
        assert_eq!(state.entry(b_id).unwrap().mixing_from, None);
    }

    #[test]
    fn add_animation_queues_behind_current() {
        let sd = two_anim_skeleton();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);

        let a_id = state.set_animation(0, AnimationId(0), false);
        let b_id = state.add_animation(0, AnimationId(1), false, 0.5);

        assert_eq!(state.entry(a_id).unwrap().next, Some(b_id));
        assert_eq!(state.entry(b_id).unwrap().previous, Some(a_id));
        assert!((state.entry(b_id).unwrap().delay - 0.5).abs() < 1e-6);
        // `a` is still the current track entry; `b` is queued.
        assert_eq!(state.current(0).unwrap().animation, AnimationId(0));
    }

    #[test]
    fn update_promotes_queued_animation_after_delay() {
        let sd = two_anim_skeleton();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);

        state.set_animation(0, AnimationId(0), false);
        // Apply once so `a` is marked applied; the promotion path reads
        // `track_last` which is set by the `apply` snapshot loop.
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let mut events = Vec::new();
        state.update(0.1);
        state.apply(&mut sk, &mut events);

        let b_id = state.add_animation(0, AnimationId(1), false, 0.2);
        // Advance past the delay (track_time is 0.1, next_delay 0.2 → need
        // track_last >= 0.2). Apply once more to make the snapshot take effect.
        state.update(0.25);
        state.apply(&mut sk, &mut events);
        state.update(0.1);

        assert_eq!(
            state.current(0).map(|e| e.animation),
            Some(AnimationId(1)),
            "queued animation should have been promoted"
        );
        assert_eq!(state.current(0).map(|e| e.track_index), Some(0));
        let _ = b_id;
    }

    /// Crossfade from animation A (rotation 10 → 20 over 1s) to B
    /// (rotation 30 → 40 over 1s) with a `mix_duration` of 0.4s. Halfway
    /// through the crossfade (`mix_time = 0.2s`, mix = 0.5), the bone's
    /// rotation should sit between each animation's sampled values: the
    /// outgoing `from` applies with `alpha * (1 - mix)`, the incoming
    /// `to` overwrites with `First` blend at full alpha, so the final
    /// rotation tracks the incoming animation's value with a residual
    /// pull from the fading-out one.
    #[test]
    fn crossfade_ramps_apply_through_mix_duration() {
        let sd = two_anim_skeleton();
        let mut state_data = AnimationStateData::new(Arc::clone(&sd));
        state_data.set_default_mix(0.4);
        let state_data = Arc::new(state_data);
        let mut state = AnimationState::new(state_data);

        // Start `a`, apply once, update past a little, switch to `b`.
        state.set_animation(0, AnimationId(0), false);
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let mut events = Vec::new();
        state.update(0.5);
        state.apply(&mut sk, &mut events);
        // Halfway through `a` (t=0.5) → rotation 15.
        assert!((sk.bones[0].rotation - 15.0).abs() < 1e-4);

        state.set_animation(0, AnimationId(1), false);
        assert!(state.current(0).unwrap().mixing_from.is_some());
        assert!((state.current(0).unwrap().mix_duration - 0.4).abs() < 1e-6);

        // Advance halfway into the mix. First update delta advances both
        // to.track_time and mix_time.
        state.update(0.2);
        state.apply(&mut sk, &mut events);

        // At mix = 0.5, `from` is still live but at half alpha; `b` is at
        // track_time = 0.2 (rot = 32). With Setup blend from applies
        // `setup + value * alpha` then to overwrites with First blend.
        // Final rotation dominated by `b`'s ~32 — not a perfect linear blend
        // here, but must be well between the two sampled values.
        let r = sk.bones[0].rotation;
        assert!(
            r > 20.0 && r < 45.0,
            "expected mid-crossfade rotation, got {r}"
        );
    }

    #[test]
    fn clear_tracks_drops_everything() {
        let sd = two_anim_skeleton();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);

        state.set_animation(0, AnimationId(0), false);
        state.add_animation(0, AnimationId(1), false, 1.0);
        state.set_animation(1, AnimationId(1), true);
        state.clear_tracks();
        assert!(state.current(0).is_none());
        assert!(state.current(1).is_none());
        assert_eq!(state.track_count(), 0);
    }
}
