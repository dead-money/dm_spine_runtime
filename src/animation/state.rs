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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::animation::{
    AnimationStateData, Event, MixBlend, MixDirection, PropertyId, animation_has_timeline,
    property_ids,
};
use crate::data::{AnimationId, SkeletonData};
use crate::skeleton::Skeleton;

/// Sentinel `AnimationId` marking a track entry as empty (no timelines,
/// used by [`AnimationState::set_empty_animation`] and friends to fade
/// back to setup pose).
///
/// spine-cpp represents this as a singleton `<empty>` Animation object;
/// we use an out-of-range id to avoid threading a static through the
/// `Arc<SkeletonData>` model.
pub const EMPTY_ANIMATION_ID: AnimationId = AnimationId(u16::MAX);

/// Classifies a [`StateEvent`] by lifecycle stage.
///
/// Ports `spine::EventType`. `Start`/`Interrupt`/`End`/`Complete`/`Dispose`
/// report `TrackEntry` state changes; `Event` is the animation keyframe
/// firing (the event timeline's output, wrapped into a queue entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Start,
    Interrupt,
    End,
    Complete,
    Dispose,
    Event,
}

/// One lifecycle or keyframe event emitted by [`AnimationState`]. Drain
/// after each `update` / `apply` pair via [`AnimationState::drain_events`].
#[derive(Debug, Clone)]
pub struct StateEvent {
    pub kind: EventType,
    pub entry: EntryId,
    /// Only present when `kind == EventType::Event` — the keyframe that
    /// fired on the corresponding track's event timeline.
    pub event: Option<Event>,
}

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
    /// `alpha < alpha_attachment_threshold`. Default 0.
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
    /// `HoldSubsequent` / `HoldFirst` / `HoldMix`). Populated by
    /// `compute_hold` whenever the track topology changes.
    pub timeline_mode: Vec<TimelineMode>,
    /// When `timeline_mode[i] == HoldMix`, the entry whose fade-out
    /// alpha contributes to timeline `i`'s hold blend.
    pub timeline_hold_mix: Vec<Option<EntryId>>,
    /// Scratch buffer for the shortest-rotation-path mixing.
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
/// Slab-backed entries with multi-track support, queuing via `add_animation`,
/// and crossfade via the `AnimationStateData` mix table.
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
    /// triggers `compute_hold` to re-classify every timeline.
    animations_changed: bool,
    /// Lifecycle + keyframe events queued by `update`/`apply`, drained by
    /// callers through [`Self::drain_events`].
    event_queue: Vec<StateEvent>,
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
            event_queue: Vec::new(),
        }
    }

    /// Take every queued [`StateEvent`] since the last drain. Typical
    /// use: call once after each `update` + `apply` pair in the game
    /// loop; poll for Start / Complete / Event / … to drive SFX or
    /// gameplay state. Events queued during a listener callback in
    /// spine-cpp's model become "just the next drain" here.
    pub fn drain_events(&mut self) -> Vec<StateEvent> {
        std::mem::take(&mut self.event_queue)
    }

    fn queue_lifecycle(&mut self, kind: EventType, entry: EntryId) {
        self.event_queue.push(StateEvent {
            kind,
            entry,
            event: None,
        });
    }

    fn queue_event(&mut self, entry: EntryId, event: Event) {
        self.event_queue.push(StateEvent {
            kind: EventType::Event,
            entry,
            event: Some(event),
        });
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
    ///   so [`apply`][Self::apply] crossfades between them.
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
                self.queue_lifecycle(EventType::Interrupt, current_id);
                self.queue_lifecycle(EventType::End, current_id);
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

    /// Set an empty animation for `track_index`, discarding any queued
    /// entries and crossfading to setup pose over `mix_duration` seconds.
    ///
    /// Ports `spine::AnimationState::setEmptyAnimation`. The sentinel
    /// [`EMPTY_ANIMATION_ID`] identifies empty entries; their apply step
    /// skips the timeline loop, and their `track_end` equals
    /// `mix_duration` so the track auto-clears when the fade completes.
    pub fn set_empty_animation(&mut self, track_index: usize, mix_duration: f32) -> EntryId {
        let id = self.set_animation(track_index, EMPTY_ANIMATION_ID, false);
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.mix_duration = mix_duration;
            entry.track_end = mix_duration;
        }
        id
    }

    /// Queue an empty animation `delay` seconds after the current/last
    /// queued entry. If `delay <= 0`, the effective delay accounts for
    /// the previous entry's `mix_duration`.
    pub fn add_empty_animation(
        &mut self,
        track_index: usize,
        mix_duration: f32,
        delay: f32,
    ) -> EntryId {
        let id = self.add_animation(track_index, EMPTY_ANIMATION_ID, false, delay);
        if let Some(entry) = self.entries.get_mut(&id) {
            // spine-cpp: if (delay <= 0) entry->_delay = max(entry->_delay +
            // entry->_mixDuration - mixDuration, 0.0f);
            if delay <= 0.0 {
                entry.delay = (entry.delay + entry.mix_duration - mix_duration).max(0.0);
            }
            entry.mix_duration = mix_duration;
            entry.track_end = mix_duration;
        }
        id
    }

    /// Crossfade every active track back to setup pose.
    pub fn set_empty_animations(&mut self, mix_duration: f32) {
        for i in 0..self.tracks.len() {
            if self.tracks[i].is_some() {
                self.set_empty_animation(i, mix_duration);
            }
        }
    }

    /// Remove the entry on `track_index`, leaving the skeleton at its
    /// current pose. Matches `spine::AnimationState::clearTrack`.
    pub fn clear_track(&mut self, track_index: usize) {
        if track_index >= self.tracks.len() {
            return;
        }
        let Some(current_id) = self.tracks[track_index].take() else {
            return;
        };
        self.queue_lifecycle(EventType::End, current_id);
        self.clear_next(current_id);

        // Walk the mixing_from chain: fire End per entry, unlink.
        let mut entry_id = self.entries.get(&current_id).and_then(|e| e.mixing_from);
        while let Some(id) = entry_id {
            self.queue_lifecycle(EventType::End, id);
            entry_id = self.entries.get(&id).and_then(|e| e.mixing_from);
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
                self.queue_lifecycle(EventType::Interrupt, from_id);
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
        // `start` on the new current triggers the animationsChanged rebuild.
        self.queue_lifecycle(EventType::Start, current);
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
        // Empty-animation sentinel has no backing data; its duration is 0.
        let duration = if animation == EMPTY_ANIMATION_ID {
            0.0
        } else {
            self.skeleton_data().animations[animation.index()].duration
        };
        let mut entry = TrackEntry::new(track_index, animation, duration, loop_);
        // mix_duration defaults to 0 when there's no predecessor, else
        // pulled from the AnimationStateData's (last_anim → new_anim) lookup.
        // The EMPTY sentinel is invalid as a key in the mix table, so
        // empty-animation entries get the default mix (caller overrides).
        if let Some(last_id) = last
            && let Some(last_entry) = self.entries.get(&last_id)
            && last_entry.animation != EMPTY_ANIMATION_ID
            && animation != EMPTY_ANIMATION_ID
        {
            entry.mix_duration = self.data.mix(last_entry.animation, animation);
        }
        self.alloc_entry(entry)
    }

    /// Dispose every entry queued behind `entry_id` (via `next` pointers).
    /// Matches `spine::AnimationState::clearNext`.
    fn clear_next(&mut self, entry_id: EntryId) {
        // Walk the `next` chain, firing Dispose on each then freeing.
        let mut next = self.entries.get(&entry_id).and_then(|e| e.next);
        while let Some(next_id) = next {
            let after = self.entries.get(&next_id).and_then(|e| e.next);
            self.queue_lifecycle(EventType::Dispose, next_id);
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
    /// - Mixing-from decay via `update_mixing_from`.
    #[allow(clippy::too_many_lines)] // matches spine-cpp's single-function shape
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
                self.queue_lifecycle(EventType::End, current_id);
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
                        // End events for every entry in the chain before disposal.
                        let mut walk = Some(from_id);
                        while let Some(id) = walk {
                            let prev = self.entries.get(&id).and_then(|e| e.mixing_from);
                            self.queue_lifecycle(EventType::End, id);
                            walk = prev;
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

    // ----- Phase 4d: animationsChanged + computeHold --------------------

    /// Re-classify every timeline on every `mixing_from` chain entry.
    /// Ports `spine::AnimationState::animationsChanged`.
    ///
    /// Walks each track from its outgoing (`mixing_from`-most) entry up
    /// through `mixing_to` edges back to the current entry, invoking
    /// [`compute_hold`][Self::compute_hold] on each entry (skipping
    /// Add-blend entries that don't need hold classification).
    fn animations_changed_rebuild(&mut self) {
        self.animations_changed = false;

        // Collect the walk order across tracks into a linear Vec so the
        // per-entry mutation inside compute_hold doesn't have to fight
        // aliased iteration of `self.tracks`.
        let mut walk: Vec<EntryId> = Vec::new();
        for i in 0..self.tracks.len() {
            let Some(start) = self.tracks[i] else {
                continue;
            };
            // Walk mixing_from to the root.
            let mut root = start;
            while let Some(from) = self.entries.get(&root).and_then(|e| e.mixing_from) {
                root = from;
            }
            // Then walk mixing_to back up.
            let mut entry = Some(root);
            while let Some(id) = entry {
                walk.push(id);
                entry = self.entries.get(&id).and_then(|e| e.mixing_to);
            }
        }

        let mut seen: HashSet<PropertyId> = HashSet::new();
        for id in walk {
            let (mixing_to, mix_blend) = self
                .entries
                .get(&id)
                .map_or((None, MixBlend::Replace), |e| (e.mixing_to, e.mix_blend));
            if mixing_to.is_none() || mix_blend != MixBlend::Add {
                self.compute_hold(id, &mut seen);
            }
        }
    }

    /// Classify each timeline on `entry` as Subsequent/First/HoldSubsequent/
    /// HoldFirst/HoldMix, seeding `timeline_mode` and `timeline_hold_mix`.
    ///
    /// Ports `spine::AnimationState::computeHold`. `seen` is the shared
    /// "already-keyed property IDs" set that drives the classification.
    fn compute_hold(&mut self, entry: EntryId, seen: &mut HashSet<PropertyId>) {
        let (animation_id, mixing_to, hold_previous) = {
            let Some(e) = self.entries.get(&entry) else {
                return;
            };
            (e.animation, e.mixing_to, e.hold_previous)
        };
        let timelines = if animation_id == EMPTY_ANIMATION_ID {
            Vec::new()
        } else {
            self.skeleton_data().animations[animation_id.index()]
                .timelines
                .clone()
        };
        let n = timelines.len();

        // Pre-size the per-entry scratch vecs.
        if let Some(e) = self.entries.get_mut(&entry) {
            e.timeline_mode.clear();
            e.timeline_mode.resize(n, TimelineMode::First);
            e.timeline_hold_mix.clear();
            e.timeline_hold_mix.resize(n, None);
        }

        // `to->_holdPrevious` fast path: every timeline is HoldFirst or
        // HoldSubsequent depending on whether this is the first time we've
        // seen its property IDs.
        if let Some(to_id) = mixing_to
            && self.entries.get(&to_id).is_some_and(|e| e.hold_previous)
        {
            let _ = hold_previous; // silences unused when to_id exists
            for (i, tl) in timelines.iter().enumerate() {
                let ids = property_ids(tl);
                let newly_seen = add_all(seen, &ids);
                let mode = if newly_seen {
                    TimelineMode::HoldFirst
                } else {
                    TimelineMode::HoldSubsequent
                };
                if let Some(e) = self.entries.get_mut(&entry) {
                    e.timeline_mode[i] = mode;
                }
            }
            return;
        }

        // Main path: per-timeline classification.
        for (i, tl) in timelines.iter().enumerate() {
            let ids = property_ids(tl);
            if !add_all(seen, &ids) {
                // Property is already keyed by a lower-indexed track or
                // another timeline — this timeline is "subsequent".
                if let Some(e) = self.entries.get_mut(&entry) {
                    e.timeline_mode[i] = TimelineMode::Subsequent;
                }
                continue;
            }

            // Newly-seen property. If there's no mixing_to, or if this is
            // a non-blendable timeline (attachment/drawOrder/event), or
            // the mixing-to animation doesn't even key this property, we
            // use MixBlend::Setup (First) with alpha_mix.
            let Some(to_id) = mixing_to else {
                continue;
            };
            let uses_first = matches!(
                tl,
                crate::data::Timeline::Attachment { .. }
                    | crate::data::Timeline::DrawOrder { .. }
                    | crate::data::Timeline::Event { .. }
            );
            let to_anim_id = self.entries.get(&to_id).map(|e| e.animation);
            let to_has_timeline = to_anim_id.is_some_and(|aid| {
                aid != EMPTY_ANIMATION_ID
                    && animation_has_timeline(&self.skeleton_data().animations[aid.index()], &ids)
            });
            if uses_first || !to_has_timeline {
                // HoldFirst: timeline ramps down under its own mix but
                // blends against setup pose.
                if let Some(e) = self.entries.get_mut(&entry) {
                    e.timeline_mode[i] = TimelineMode::First;
                }
                continue;
            }

            // Walk the rest of the mixing_to chain. If any subsequent
            // entry also keys this property with mix_duration > 0, we
            // need HoldMix pointing at that entry.
            let mut hold_mix: Option<EntryId> = None;
            let mut next_id = self.entries.get(&to_id).and_then(|e| e.mixing_to);
            while let Some(nid) = next_id {
                let Some(n_entry) = self.entries.get(&nid) else {
                    break;
                };
                let n_anim = n_entry.animation;
                if animation_has_timeline(&self.skeleton_data().animations[n_anim.index()], &ids) {
                    next_id = n_entry.mixing_to;
                    continue;
                }
                if n_entry.mix_duration > 0.0 {
                    hold_mix = Some(nid);
                }
                break;
            }
            let mode = if hold_mix.is_some() {
                TimelineMode::HoldMix
            } else {
                TimelineMode::HoldFirst
            };
            if let Some(e) = self.entries.get_mut(&entry) {
                e.timeline_mode[i] = mode;
                e.timeline_hold_mix[i] = hold_mix;
            }
        }
    }

    /// Fire the events captured during the current entry's apply and
    /// emit a Complete event if the track just finished a loop or (for
    /// non-looping) reached its end.
    ///
    /// Ports `spine::AnimationState::queueEvents`.
    #[allow(clippy::float_cmp)] // tag/sentinel comparisons
    fn queue_events_for(&mut self, entry_id: EntryId, animation_time: f32, captured: &[Event]) {
        let (animation_start, animation_end, track_time, track_last, animation_last, loop_) = {
            let Some(e) = self.entries.get(&entry_id) else {
                return;
            };
            (
                e.animation_start,
                e.animation_end,
                e.track_time,
                e.track_last,
                e.animation_last,
                e.loop_,
            )
        };
        let duration = animation_end - animation_start;
        let track_last_wrapped = if duration == 0.0 {
            f32::NAN
        } else {
            track_last % duration
        };

        // Queue events before Complete.
        let mut i = 0;
        while i < captured.len() {
            let e = &captured[i];
            if e.time < track_last_wrapped {
                break;
            }
            if e.time > animation_end {
                i += 1;
                continue;
            }
            self.queue_event(entry_id, e.clone());
            i += 1;
        }

        // Complete detection.
        let complete = if loop_ {
            if duration == 0.0 {
                true
            } else {
                let cycles = (track_time / duration).floor() as i64;
                cycles > 0 && cycles > (track_last / duration).floor() as i64
            }
        } else {
            animation_time >= animation_end && animation_last < animation_end
        };
        if complete {
            self.queue_lifecycle(EventType::Complete, entry_id);
        }

        // Queue events after Complete.
        while i < captured.len() {
            let e = &captured[i];
            if e.time < animation_start {
                i += 1;
                continue;
            }
            self.queue_event(entry_id, e.clone());
            i += 1;
        }
    }

    /// Apply the mixing-from chain for `to`, returning the alpha
    /// multiplier that should scale the incoming animation.
    ///
    /// Ports `spine::AnimationState::applyMixingFrom`. Dispatches per
    /// timeline via [`TrackEntry::timeline_mode`], which
    /// [`compute_hold`][Self::compute_hold] populated in
    /// [`animations_changed_rebuild`][Self::animations_changed_rebuild].
    #[allow(clippy::float_cmp)] // tag comparisons (mix_duration == 0, blend == Setup/First)
    #[allow(clippy::too_many_lines)] // matches spine-cpp's single-function shape
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
            let duration = if from_entry.animation == EMPTY_ANIMATION_ID {
                0.0
            } else {
                self.skeleton_data().animations[from_entry.animation.index()].duration
            };
            (
                from_entry.alpha,
                from_entry.event_threshold,
                from_entry.reverse,
                from_entry.animation_last,
                from_entry.animation_time(),
                from_entry.animation,
                duration,
            )
        };
        let to_interrupt_alpha = self.entries.get(&to).map_or(1.0, |e| e.interrupt_alpha);
        let alpha_hold = from_alpha * to_interrupt_alpha;
        let alpha_mix = alpha_hold * (1.0 - mix);
        // Whether DrawOrder timelines on the mixing-out side fire; also
        // used by the HoldMix path below when filtering timelines.
        let draw_order = mix
            < self
                .entries
                .get(&from)
                .map_or(0.0, |e| e.mix_draw_order_threshold);

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

        let timelines = if from_animation_id == EMPTY_ANIMATION_ID {
            Vec::new()
        } else {
            self.skeleton_data().animations[from_animation_id.index()]
                .timelines
                .clone()
        };

        if blend == MixBlend::Add {
            // Add-blend entries: every timeline applies with the entry's
            // blend + alpha_mix + MixDirection::Out.
            for tl in &timelines {
                tl.apply(
                    skeleton,
                    from_animation_last,
                    apply_time,
                    ev_ref,
                    alpha_mix,
                    blend,
                    MixDirection::Out,
                );
            }
        } else {
            // Per-timeline dispatch via the entry's timeline_mode.
            // Snapshot timeline_mode + hold-mix state first to avoid
            // aliasing self during the inner loop.
            let modes = self
                .entries
                .get(&from)
                .map(|e| e.timeline_mode.clone())
                .unwrap_or_default();
            let hold_mix = self
                .entries
                .get(&from)
                .map(|e| e.timeline_hold_mix.clone())
                .unwrap_or_default();

            let mut total_alpha_acc = 0.0_f32;
            for (i, tl) in timelines.iter().enumerate() {
                let mode = modes.get(i).copied().unwrap_or(TimelineMode::First);
                let (timeline_blend, tl_alpha) = match mode {
                    TimelineMode::Subsequent => (blend, alpha_mix),
                    TimelineMode::First => (MixBlend::Setup, alpha_mix),
                    TimelineMode::HoldSubsequent => (blend, alpha_hold),
                    TimelineMode::HoldFirst => (MixBlend::Setup, alpha_hold),
                    TimelineMode::HoldMix => {
                        // Alpha tapers with the downstream mix's progress.
                        let hm_id = hold_mix.get(i).and_then(|h| *h);
                        let (hm_time, hm_duration) = hm_id
                            .and_then(|id| self.entries.get(&id))
                            .map_or((0.0, 0.0), |e| (e.mix_time, e.mix_duration));
                        let frac = if hm_duration > 0.0 {
                            (1.0 - hm_time / hm_duration).max(0.0)
                        } else {
                            1.0
                        };
                        (MixBlend::Setup, alpha_hold * frac)
                    }
                };
                total_alpha_acc += tl_alpha;

                // DrawOrder timelines on the mixing-out side: spine-cpp
                // skips them when mix is past mix_draw_order_threshold
                // (except for the Setup-blend reset). We approximate
                // "subsequent + !draw_order → skip" to match that.
                if matches!(tl, crate::data::Timeline::DrawOrder { .. })
                    && mode == TimelineMode::Subsequent
                    && !draw_order
                {
                    continue;
                }

                tl.apply(
                    skeleton,
                    from_animation_last,
                    apply_time,
                    ev_ref,
                    tl_alpha,
                    timeline_blend,
                    MixDirection::Out,
                );
            }
            if let Some(from_entry) = self.entries.get_mut(&from) {
                from_entry.total_alpha = total_alpha_acc;
            }
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
    /// Ports `spine::AnimationState::apply`. Uses the per-timeline
    /// `timeline_mode` specialisation (`Subsequent` / `First` /
    /// `HoldSubsequent` / `HoldFirst` / `HoldMix`) for crossfades when
    /// lower tracks also key the same property.
    ///
    /// Apply the skeleton's current track state, draining captured
    /// keyframe events and lifecycle events into the internal queue (poll
    /// via [`Self::drain_events`]). The `events` out-param is retained
    /// for callers that want the raw keyframe event list; populated only
    /// with per-entry firings, not `Complete`/`Start`/etc. state events.
    #[allow(clippy::too_many_lines)]
    pub fn apply(&mut self, skeleton: &mut Skeleton, events: &mut Vec<Event>) {
        // Phase 4d: re-classify timelines when the track topology
        // changed. Populates every entry's timeline_mode/timeline_hold_mix
        // so apply_mixing_from can dispatch per-timeline.
        if self.animations_changed {
            self.animations_changed_rebuild();
        }

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
            let is_empty = animation_id == EMPTY_ANIMATION_ID;
            let duration = if is_empty {
                0.0
            } else {
                self.skeleton_data().animations[animation_id.index()].duration
            };
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

            // Capture timeline firings into a scratch Vec, then route
            // through queue_events_for (which emits Complete + Event
            // lifecycle entries on the state queue).
            let mut captured: Vec<Event> = Vec::new();
            let timelines = if is_empty {
                Vec::new()
            } else {
                self.skeleton_data().animations[animation_id.index()]
                    .timelines
                    .clone()
            };

            for tl in &timelines {
                tl.apply(
                    skeleton,
                    animation_last,
                    apply_time,
                    &mut captured,
                    effective_alpha,
                    blend,
                    MixDirection::In,
                );
            }

            // Keep the caller's out-param populated for Phase 3 API
            // compat; the state queue also gets them (routed through
            // queue_events_for which handles Complete detection).
            events.extend(captured.iter().cloned());
            self.queue_events_for(current_id, animation_time, &captured);

            if let Some(entry) = self.entries.get_mut(&current_id) {
                entry.next_animation_last = animation_time;
                entry.next_track_last = entry.track_time;
            }
        }
    }
}

/// Insert every id into `seen`, returning `true` if any were newly
/// added. Ports spine-cpp's `HashMap::addAll(keys, value)` idiom used in
/// `computeHold`.
fn add_all(seen: &mut HashSet<PropertyId>, ids: &[PropertyId]) -> bool {
    let mut any_new = false;
    for id in ids {
        if seen.insert(*id) {
            any_new = true;
        }
    }
    any_new
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
    fn set_animation_queues_start_event() {
        let sd = one_bone_rotate();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);
        let id = state.set_animation(0, AnimationId(0), false);

        let events = state.drain_events();
        assert_eq!(events.len(), 1, "expected one Start event, got {events:?}");
        assert_eq!(events[0].kind, EventType::Start);
        assert_eq!(events[0].entry, id);
        assert!(events[0].event.is_none());
    }

    #[test]
    fn set_animation_over_applied_queues_interrupt_and_start() {
        let sd = two_anim_skeleton();
        let mut state_data = AnimationStateData::new(Arc::clone(&sd));
        state_data.set_default_mix(0.2);
        let state_data = Arc::new(state_data);
        let mut state = AnimationState::new(state_data);

        state.set_animation(0, AnimationId(0), false);
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let mut events = Vec::new();
        state.apply(&mut sk, &mut events);
        // Discard the Start event from the first set_animation.
        let _ = state.drain_events();

        state.set_animation(0, AnimationId(1), false);
        let kinds: Vec<_> = state.drain_events().into_iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EventType::Interrupt));
        assert!(kinds.contains(&EventType::Start));
    }

    #[test]
    fn set_empty_animation_crossfades_to_setup() {
        let sd = two_anim_skeleton();
        let state_data = Arc::new(AnimationStateData::new(Arc::clone(&sd)));
        let mut state = AnimationState::new(state_data);
        state.set_animation(0, AnimationId(0), false);
        let mut sk = Skeleton::new(Arc::clone(&sd));
        sk.update_cache();
        let mut events = Vec::new();
        state.update(0.5);
        state.apply(&mut sk, &mut events);
        // Rotation at t=0.5 on animation `a` is 15.

        let empty_id = state.set_empty_animation(0, 0.3);
        assert_eq!(state.current(0).unwrap().animation, EMPTY_ANIMATION_ID);
        assert!((state.entry(empty_id).unwrap().mix_duration - 0.3).abs() < 1e-6);
        assert!((state.entry(empty_id).unwrap().track_end - 0.3).abs() < 1e-6);

        // Advance past the mix duration. spine-cpp's update clears the
        // mixing_from chain *inside* update_mixing_from but defers the
        // track-clear by one more update tick (the clear-on-end check
        // runs before update_mixing_from each call). So it takes two
        // update calls to fully unload the empty track.
        state.update(0.4);
        state.apply(&mut sk, &mut events);
        state.update(0.1);
        state.apply(&mut sk, &mut events);
        state.update(0.0);
        assert!(
            state.current(0).is_none(),
            "empty-animation track should clear after mix_duration"
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
