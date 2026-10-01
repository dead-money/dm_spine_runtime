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

//! `AnimationState` (4.3): plays animations on tracks with crossfades,
//! queuing and events. Track entries live in a slab addressed by
//! generation-checked [`EntryId`]s.
//!
//! Keyframe events are pushed to the `events` out-parameter of
//! [`AnimationState::apply`]; lifecycle events (start, interrupt, end,
//! complete, dispose) and keyframe events tagged with their entry are
//! collected for [`AnimationState::drain_events`].

#![allow(clippy::float_cmp)]

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

use crate::animation::apply::{apply_timeline, set_attachment_by_key};
use crate::animation::curve::{curve_value1, search, sign};
use crate::animation::{AnimationStateData, Event, MixFrom};
use crate::data::animation::PropertyId;
use crate::data::{Animation, AnimationId, SkeletonData, Timeline};
use crate::skeleton::Skeleton;

/// Plays nothing; mixing to it fades a track back to the setup pose.
pub const EMPTY_ANIMATION_ID: AnimationId = AnimationId(u16::MAX);

// Timeline modes. The low bits are a `MixFrom`; `HOLD` keeps a mixing-out
// timeline at full strength because the entry mixing in keys it too.
const CURRENT: u8 = 0;
const SETUP: u8 = 1;
const FIRST: u8 = 2;
const MODE: u8 = 3;
const HOLD: u8 = 4;

// `Slot::attachment_state` offsets from `unkeyed_state`.
const ATTACH_SETUP: i32 = 1;
const ATTACH_RETAIN: i32 = 2;

fn mix_from(mode: u8) -> MixFrom {
    match mode & MODE {
        SETUP => MixFrom::Setup,
        FIRST => MixFrom::First,
        _ => MixFrom::Current,
    }
}

/// Easing applied to a track entry's mix percentage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    #[default]
    Linear,
    Smooth,
    SlowFast,
    FastSlow,
    Circle,
}

impl Interpolation {
    #[must_use]
    pub fn apply(self, a: f32) -> f32 {
        match self {
            Self::Linear => a,
            Self::Smooth => a * a * (3.0 - 2.0 * a),
            Self::SlowFast => a * a,
            Self::FastSlow => -((a - 1.0) * (a - 1.0)) + 1.0,
            Self::Circle => {
                if a <= 0.5 {
                    let a = a * 2.0;
                    (1.0 - (1.0 - a * a).sqrt()) / 2.0
                } else {
                    let a = (a - 1.0) * 2.0;
                    f32::midpoint((1.0 - a * a).sqrt(), 1.0)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Start,
    Interrupt,
    End,
    Complete,
    Dispose,
    Event,
}

/// A lifecycle or keyframe event. `track_index` and `animation` are copied
/// so they stay readable after the entry is disposed.
#[derive(Debug, Clone)]
pub struct StateEvent {
    pub kind: EventType,
    pub entry: EntryId,
    pub track_index: usize,
    pub animation: AnimationId,
    /// Set for [`EventType::Event`].
    pub event: Option<Event>,
}

/// Handle to a track entry. Stale once the entry is disposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId {
    index: u32,
    generation: u32,
}

/// Playback of one animation on a track (`TrackEntry`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TrackEntry {
    pub animation: AnimationId,
    pub track_index: usize,
    pub previous: Option<EntryId>,
    pub next: Option<EntryId>,
    pub mixing_from: Option<EntryId>,
    pub mixing_to: Option<EntryId>,
    pub looping: bool,
    /// Adds this entry's timeline values to lower tracks instead of mixing.
    pub additive: bool,
    pub reverse: bool,
    pub shortest_rotation: bool,
    keep_hold: bool,
    pub event_threshold: f32,
    pub mix_attachment_threshold: f32,
    pub alpha_attachment_threshold: f32,
    pub mix_draw_order_threshold: f32,
    pub animation_start: f32,
    pub animation_end: f32,
    pub animation_last: f32,
    next_animation_last: f32,
    pub delay: f32,
    pub track_time: f32,
    track_last: f32,
    next_track_last: f32,
    pub track_end: f32,
    pub time_scale: f32,
    pub alpha: f32,
    pub mix_time: f32,
    pub mix_duration: f32,
    pub mix_interpolation: Interpolation,
    total_alpha: f32,
    timeline_mode: Vec<u8>,
    timeline_hold_mix: Vec<Option<EntryId>>,
    timelines_rotation: Vec<f32>,
}

impl TrackEntry {
    fn new(
        track_index: usize,
        animation: AnimationId,
        duration: f32,
        looping: bool,
        mix_duration: f32,
    ) -> Self {
        Self {
            animation,
            track_index,
            previous: None,
            next: None,
            mixing_from: None,
            mixing_to: None,
            looping,
            additive: false,
            reverse: false,
            shortest_rotation: false,
            keep_hold: false,
            event_threshold: 0.0,
            mix_attachment_threshold: 0.0,
            alpha_attachment_threshold: 0.0,
            mix_draw_order_threshold: 0.0,
            animation_start: 0.0,
            animation_end: duration,
            animation_last: -1.0,
            next_animation_last: -1.0,
            delay: 0.0,
            track_time: 0.0,
            track_last: -1.0,
            next_track_last: -1.0,
            track_end: f32::MAX,
            time_scale: 1.0,
            alpha: 1.0,
            mix_time: 0.0,
            mix_duration,
            mix_interpolation: Interpolation::Linear,
            total_alpha: 0.0,
            timeline_mode: Vec::new(),
            timeline_hold_mix: Vec::new(),
            timelines_rotation: Vec::new(),
        }
    }

    /// Time within the animation, wrapped when looping.
    #[must_use]
    pub fn animation_time(&self) -> f32 {
        if !self.looping {
            return (self.track_time + self.animation_start).min(self.animation_end);
        }
        let duration = self.animation_end - self.animation_start;
        if duration == 0.0 {
            return self.animation_start;
        }
        self.track_time % duration + self.animation_start
    }

    /// Also resets the next frame's `animation_last`.
    pub fn set_animation_last(&mut self, value: f32) {
        self.animation_last = value;
        self.next_animation_last = value;
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.track_time >= self.animation_end - self.animation_start
    }

    /// Track time when the current loop or the animation completes.
    #[must_use]
    pub fn track_complete(&self) -> f32 {
        let duration = self.animation_end - self.animation_start;
        if duration != 0.0 {
            if self.looping {
                return duration * (1.0 + (self.track_time / duration).trunc());
            }
            if self.track_time < duration {
                return duration;
            }
        }
        self.track_time
    }

    #[must_use]
    pub fn was_applied(&self) -> bool {
        self.next_track_last != -1.0
    }

    #[must_use]
    pub fn is_empty_animation(&self) -> bool {
        self.animation == EMPTY_ANIMATION_ID
    }

    /// Mix percentage after easing, 0..=1.
    #[must_use]
    pub fn mix(&self) -> f32 {
        if self.mix_duration == 0.0 {
            return 1.0;
        }
        let mix = self.mix_time / self.mix_duration;
        if mix >= 1.0 {
            return 1.0;
        }
        if self.mix_interpolation == Interpolation::Linear {
            return mix;
        }
        self.mix_interpolation.apply(mix).clamp(0.0, 1.0)
    }

    /// Discards shortest-rotation history, eg after changing rotation direction.
    pub fn reset_rotation_directions(&mut self) {
        self.timelines_rotation.clear();
    }
}

/// Multiplicative hasher for integer property ids.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_i64(&mut self, v: i64) {
        self.0 = (v as u64 ^ (v as u64 >> 29)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type IdMap<V> = HashMap<PropertyId, V, BuildHasherDefault<IdHasher>>;

struct Slot {
    generation: u32,
    entry: Option<TrackEntry>,
    spare: EntryBuffers,
}

/// A disposed entry's emptied buffers, handed to the next entry in its slot.
#[derive(Default)]
struct EntryBuffers {
    timeline_mode: Vec<u8>,
    timeline_hold_mix: Vec<Option<EntryId>>,
    timelines_rotation: Vec<f32>,
}

struct Queued {
    kind: EventType,
    entry: EntryId,
    event: Option<Event>,
}

/// Plays animations on tracks and poses a skeleton from them.
///
/// For a skeleton that isn't drawn, [`Self::update`] and [`Self::apply`]
/// alone keep its tracks and events going; skip the world transform and
/// rendering until it's visible again.
pub struct AnimationState {
    data: Arc<AnimationStateData>,
    skeleton_data: Arc<SkeletonData>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    tracks: Vec<Option<EntryId>>,
    /// Scales every track's delta.
    pub time_scale: f32,
    animations_changed: bool,
    unkeyed_state: i32,
    property_owners: IdMap<EntryId>,
    events: Vec<Event>,
    queue: Vec<Queued>,
    drain_disabled: bool,
    drained: Vec<StateEvent>,
    pose_rotation: Vec<f32>,
    pose_modes: Vec<(Vec<u8>, Vec<Option<EntryId>>)>,
}

impl AnimationState {
    #[must_use]
    pub fn new(data: Arc<AnimationStateData>) -> Self {
        Self {
            skeleton_data: Arc::clone(data.data()),
            data,
            slots: Vec::new(),
            free: Vec::new(),
            tracks: Vec::new(),
            time_scale: 1.0,
            animations_changed: false,
            unkeyed_state: 0,
            property_owners: IdMap::default(),
            events: Vec::new(),
            queue: Vec::new(),
            drain_disabled: false,
            drained: Vec::new(),
            pose_rotation: Vec::new(),
            pose_modes: Vec::new(),
        }
    }

    #[must_use]
    pub fn data(&self) -> &Arc<AnimationStateData> {
        &self.data
    }

    /// The entry playing on a track.
    #[must_use]
    pub fn track(&self, track_index: usize) -> Option<EntryId> {
        self.tracks.get(track_index).copied().flatten()
    }

    #[must_use]
    pub fn tracks(&self) -> &[Option<EntryId>] {
        &self.tracks
    }

    #[must_use]
    pub fn entry(&self, id: EntryId) -> Option<&TrackEntry> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation == id.generation {
            slot.entry.as_ref()
        } else {
            None
        }
    }

    pub fn entry_mut(&mut self, id: EntryId) -> Option<&mut TrackEntry> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation == id.generation {
            slot.entry.as_mut()
        } else {
            None
        }
    }

    /// Sets the entry's mix duration and, when `delay <= 0`, its delay
    /// relative to when the previous entry completes.
    pub fn set_mix_duration(&mut self, id: EntryId, mix_duration: f32, mut delay: f32) {
        let previous_complete = self.e(id).previous.map(|p| self.e(p).track_complete());
        if delay <= 0.0 {
            delay = previous_complete.map_or(0.0, |c| (delay + c - mix_duration).max(0.0));
        }
        let e = self.e_mut(id);
        e.mix_duration = mix_duration;
        e.delay = delay;
    }

    /// Lifecycle and keyframe events since the last call.
    pub fn drain_events(&mut self) -> Vec<StateEvent> {
        std::mem::take(&mut self.drained)
    }

    /// As [`Self::drain_events`], reusing `out`'s allocation.
    pub fn drain_events_into(&mut self, out: &mut Vec<StateEvent>) {
        out.append(&mut self.drained);
    }

    #[inline]
    fn e(&self, id: EntryId) -> &TrackEntry {
        self.slots[id.index as usize]
            .entry
            .as_ref()
            .expect("live track entry")
    }

    #[inline]
    fn e_mut(&mut self, id: EntryId) -> &mut TrackEntry {
        self.slots[id.index as usize]
            .entry
            .as_mut()
            .expect("live track entry")
    }

    fn alloc(&mut self, mut entry: TrackEntry) -> EntryId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            let spare = std::mem::take(&mut slot.spare);
            entry.timeline_mode = spare.timeline_mode;
            entry.timeline_hold_mix = spare.timeline_hold_mix;
            entry.timelines_rotation = spare.timelines_rotation;
            slot.entry = Some(entry);
            EntryId {
                index,
                generation: slot.generation,
            }
        } else {
            self.slots.push(Slot {
                generation: 0,
                entry: Some(entry),
                spare: EntryBuffers::default(),
            });
            EntryId {
                index: (self.slots.len() - 1) as u32,
                generation: 0,
            }
        }
    }

    fn dispose(&mut self, id: EntryId) {
        let slot = &mut self.slots[id.index as usize];
        if slot.generation != id.generation {
            return;
        }
        if let Some(entry) = slot.entry.take() {
            slot.spare = EntryBuffers {
                timeline_mode: entry.timeline_mode,
                timeline_hold_mix: entry.timeline_hold_mix,
                timelines_rotation: entry.timelines_rotation,
            };
            slot.spare.timeline_mode.clear();
            slot.spare.timeline_hold_mix.clear();
            slot.spare.timelines_rotation.clear();
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(id.index);
        }
    }

    fn enqueue(&mut self, kind: EventType, entry: EntryId) {
        self.queue.push(Queued {
            kind,
            entry,
            event: None,
        });
        if matches!(kind, EventType::Start | EventType::End) {
            self.animations_changed = true;
        }
    }

    /// Hands queued events to [`Self::drain_events`] and disposes ended
    /// entries.
    fn drain(&mut self) {
        if self.drain_disabled {
            return;
        }
        let queue = std::mem::take(&mut self.queue);
        for q in &queue {
            let (track_index, animation) = {
                let e = self.e(q.entry);
                (e.track_index, e.animation)
            };
            let event = |kind| StateEvent {
                kind,
                entry: q.entry,
                track_index,
                animation,
                event: None,
            };
            match q.kind {
                EventType::End => {
                    self.drained.push(event(EventType::End));
                    self.drained.push(event(EventType::Dispose));
                }
                EventType::Event => self.drained.push(StateEvent {
                    event: q.event.clone(),
                    ..event(EventType::Event)
                }),
                kind => self.drained.push(event(kind)),
            }
        }
        for q in &queue {
            if matches!(q.kind, EventType::End | EventType::Dispose) {
                self.dispose(q.entry);
            }
        }
        let mut queue = queue;
        queue.clear();
        if self.queue.is_empty() {
            self.queue = queue;
        }
    }

    /// Advances every track by `delta` seconds.
    pub fn update(&mut self, delta: f32) {
        let delta = delta * self.time_scale;
        for i in 0..self.tracks.len() {
            let Some(cur) = self.tracks[i] else { continue };
            let current = self.e_mut(cur);
            current.animation_last = current.next_animation_last;
            current.track_last = current.next_track_last;
            let mut current_delta = delta * current.time_scale;
            if current.delay > 0.0 {
                current.delay -= current_delta;
                if current.delay > 0.0 {
                    continue;
                }
                current_delta = -current.delay;
                current.delay = 0.0;
            }

            let (next, track_last, time_scale) =
                (current.next, current.track_last, current.time_scale);
            if let Some(next) = next {
                let next_time = track_last - self.e(next).delay;
                if next_time >= 0.0 {
                    let n = self.e_mut(next);
                    n.delay = 0.0;
                    n.track_time += if time_scale == 0.0 {
                        0.0
                    } else {
                        (next_time / time_scale + delta) * n.time_scale
                    };
                    self.e_mut(cur).track_time += current_delta;
                    self.set_track(i, next, true);
                    let mut e = next;
                    while let Some(from) = self.e(e).mixing_from {
                        self.e_mut(e).mix_time += delta;
                        e = from;
                    }
                    continue;
                }
            } else {
                let current = self.e(cur);
                if current.track_last >= current.track_end && current.mixing_from.is_none() {
                    self.tracks[i] = None;
                    self.enqueue(EventType::End, cur);
                    self.clear_next(cur);
                    continue;
                }
            }
            if self.e(cur).mixing_from.is_some() && self.update_mixing_from(cur, delta) {
                // Every mixing-from entry finished.
                let mut from = self.e_mut(cur).mixing_from.take();
                if let Some(f) = from {
                    self.e_mut(f).mixing_to = None;
                }
                while let Some(f) = from {
                    self.enqueue(EventType::End, f);
                    from = self.e(f).mixing_from;
                }
            }
            self.e_mut(cur).track_time += current_delta;
        }
        self.drain();
    }

    fn update_mixing_from(&mut self, to: EntryId, delta: f32) -> bool {
        let Some(from) = self.e(to).mixing_from else {
            return true;
        };
        let finished = self.update_mixing_from(from, delta);
        {
            let f = self.e_mut(from);
            f.animation_last = f.next_animation_last;
            f.track_last = f.next_track_last;
        }
        let t = self.e(to);
        if t.next_track_last != -1.0 && t.mix_time >= t.mix_duration {
            let f = self.e(from);
            let (total_alpha, from_from) = (f.total_alpha, f.mixing_from);
            if total_alpha == 0.0 || t.mix_duration == 0.0 {
                self.e_mut(to).mixing_from = from_from;
                if let Some(ff) = from_from {
                    self.e_mut(ff).mixing_to = Some(to);
                }
                if total_alpha == 0.0 {
                    let mut next = to;
                    while let Some(n) = self.e(next).mixing_to {
                        self.e_mut(next).keep_hold = true;
                        next = n;
                    }
                }
                self.enqueue(EventType::End, from);
            }
            return finished;
        }
        let f = self.e_mut(from);
        f.track_time += delta * f.time_scale;
        self.e_mut(to).mix_time += delta;
        false
    }

    /// Poses the skeleton from every track. Keyframe events that fire are
    /// pushed to `events`. Returns whether any track was applied.
    pub fn apply(&mut self, skeleton: &mut Skeleton, events: &mut Vec<Event>) -> bool {
        if self.animations_changed {
            self.animations_changed();
        }
        let applied = self.apply_tracks(skeleton, Some(events));
        self.drain();
        applied
    }

    /// Poses the skeleton as [`Self::apply`] would at the current track
    /// times, without firing events or touching anything a later
    /// [`Self::update`] or [`Self::apply`] reads.
    pub fn pose(&mut self, skeleton: &mut Skeleton) -> bool {
        let pending = self.animations_changed;
        if pending {
            self.stash_modes(false);
            self.animations_changed();
        }
        let applied = self.apply_tracks(skeleton, None);
        if pending {
            self.stash_modes(true);
            self.animations_changed = true;
        }
        applied
    }

    /// Copies the applied entries' timeline modes aside (or back), so a pose
    /// can compute pending modes and leave the next apply's `keep_hold` input
    /// untouched.
    fn stash_modes(&mut self, restore: bool) {
        let mut k = 0;
        for i in 0..self.tracks.len() {
            let mut next = self.tracks[i];
            while let Some(id) = next {
                if k == self.pose_modes.len() {
                    self.pose_modes.push((Vec::new(), Vec::new()));
                }
                let entry = self.slots[id.index as usize]
                    .entry
                    .as_mut()
                    .expect("live track entry");
                let (modes, hold_mix) = &mut self.pose_modes[k];
                if restore {
                    entry.timeline_mode.clone_from(modes);
                    entry.timeline_hold_mix.clone_from(hold_mix);
                } else {
                    modes.clone_from(&entry.timeline_mode);
                    hold_mix.clone_from(&entry.timeline_hold_mix);
                }
                next = entry.mixing_from;
                k += 1;
            }
        }
    }

    /// `events` is `None` when posing: no events, and entries keep their
    /// last-applied times, rotation history and total alpha.
    fn apply_tracks(
        &mut self,
        skeleton: &mut Skeleton,
        mut events: Option<&mut Vec<Event>>,
    ) -> bool {
        let record = events.is_some();
        let sd = Arc::clone(skeleton.data());
        debug_assert!(
            Arc::ptr_eq(&sd, &self.skeleton_data),
            "skeleton uses other data"
        );
        let mut applied = false;
        for i in 0..self.tracks.len() {
            let Some(cur) = self.tracks[i] else { continue };
            if self.e(cur).delay > 0.0 {
                continue;
            }
            applied = true;

            let mut alpha = self.e(cur).alpha;
            if self.e(cur).mixing_from.is_some() {
                alpha *= self.apply_mixing_from(cur, skeleton, &sd, record);
            } else {
                let c = self.e(cur);
                if c.track_time >= c.track_end && c.next.is_none() {
                    alpha = 0.0;
                }
            }

            let c = self.e(cur);
            let animation_last = c.animation_last;
            let animation_time = c.animation_time();
            let reverse = c.reverse;
            let anim = animation(&sd, c.animation);
            let apply_time = if reverse {
                anim.duration - animation_time
            } else {
                animation_time
            };
            let mut event_buf = std::mem::take(&mut self.events);

            if i == 0 && alpha == 1.0 {
                for t in &anim.timelines {
                    if let Timeline::Attachment { .. } = t {
                        self.apply_attachment_timeline(
                            t,
                            skeleton,
                            &sd,
                            apply_time,
                            MixFrom::Setup,
                            true,
                        );
                    } else {
                        let mut ev = (record && !reverse).then_some(&mut event_buf);
                        apply_timeline(
                            skeleton,
                            &sd,
                            t,
                            animation_last,
                            apply_time,
                            &mut ev,
                            alpha,
                            MixFrom::Setup,
                            false,
                            false,
                            false,
                        );
                    }
                }
            } else {
                let c = self.e(cur);
                let retain = alpha >= c.alpha_attachment_threshold;
                let add = c.additive;
                let shortest = add || c.shortest_rotation;
                let n = anim.timelines.len();
                let first_frame = !shortest && c.timelines_rotation.len() != n << 1;
                let mut rotation = self.take_rotation(cur, record);
                if first_frame {
                    rotation.clear();
                    rotation.resize(n << 1, 0.0);
                }
                let modes = std::mem::take(&mut self.e_mut(cur).timeline_mode);
                for (ii, t) in anim.timelines.iter().enumerate() {
                    let from = mix_from(modes[ii]);
                    if !shortest && matches!(t, Timeline::Rotate { .. }) {
                        apply_rotate_timeline(
                            t,
                            skeleton,
                            &sd,
                            apply_time,
                            alpha,
                            from,
                            &mut rotation,
                            ii << 1,
                            first_frame,
                        );
                    } else if let Timeline::Attachment { .. } = t {
                        self.apply_attachment_timeline(t, skeleton, &sd, apply_time, from, retain);
                    } else {
                        let mut ev = (record && !reverse).then_some(&mut event_buf);
                        apply_timeline(
                            skeleton,
                            &sd,
                            t,
                            animation_last,
                            apply_time,
                            &mut ev,
                            alpha,
                            from,
                            add,
                            false,
                            false,
                        );
                    }
                }
                self.e_mut(cur).timeline_mode = modes;
                self.put_rotation(cur, rotation, record);
            }
            self.events = event_buf;
            let Some(events) = events.as_deref_mut() else {
                continue;
            };
            if reverse {
                self.events_reverse(anim, animation_last, animation_time);
            }
            self.queue_events(cur, animation_time, Some(events));
            self.events.clear();
            let c = self.e_mut(cur);
            c.next_animation_last = animation_time;
            c.next_track_last = c.track_time;
        }

        // Restore setup attachments the timelines mixed out without keying.
        let setup_state = self.unkeyed_state + ATTACH_SETUP;
        for s in 0..skeleton.slots.len() {
            if skeleton.slots[s].attachment_state == setup_state {
                let key = sd.slots[s].attachment_key;
                set_attachment_by_key(skeleton, crate::data::SlotId(s as u16), key, false);
            }
        }
        self.unkeyed_state += 2;
        applied
    }

    fn take_rotation(&mut self, entry: EntryId, record: bool) -> Vec<f32> {
        if record {
            return std::mem::take(&mut self.e_mut(entry).timelines_rotation);
        }
        let mut rotation = std::mem::take(&mut self.pose_rotation);
        rotation.clone_from(&self.e(entry).timelines_rotation);
        rotation
    }

    fn put_rotation(&mut self, entry: EntryId, rotation: Vec<f32>, record: bool) {
        if record {
            self.e_mut(entry).timelines_rotation = rotation;
        } else {
            self.pose_rotation = rotation;
        }
    }

    fn apply_mixing_from(
        &mut self,
        to: EntryId,
        skeleton: &mut Skeleton,
        sd: &Arc<SkeletonData>,
        record: bool,
    ) -> f32 {
        let from = self.e(to).mixing_from.expect("mixing from");
        let from_mix = if self.e(from).mixing_from.is_some() {
            self.apply_mixing_from(from, skeleton, sd, record)
        } else {
            1.0
        };
        let t = self.e(to);
        let mix = t.mix();
        let (to_alpha, to_mix_duration) = (t.alpha, t.mix_duration);
        let f = self.e(from);
        let a = f.alpha * from_mix;
        let keep = 1.0 - mix * to_alpha;
        let alpha_mix = a * (1.0 - mix);
        let alpha_hold = if keep > 0.0 { alpha_mix / keep } else { a };
        let retain_attachments = mix < f.mix_attachment_threshold;
        let draw_order = mix < f.mix_draw_order_threshold;
        let add = f.additive;
        let shortest = add || f.shortest_rotation;
        let alpha_attachment_threshold = f.alpha_attachment_threshold;
        let anim = animation(sd, f.animation);
        let n = anim.timelines.len();
        let first_frame = !shortest && f.timelines_rotation.len() != n << 1;
        let animation_last = f.animation_last;
        let animation_time = f.animation_time();
        let reverse = f.reverse;
        let apply_time = if reverse {
            anim.duration - animation_time
        } else {
            animation_time
        };
        let use_events = record && !reverse && mix < f.event_threshold;

        let mut rotation = self.take_rotation(from, record);
        if first_frame {
            rotation.clear();
            rotation.resize(n << 1, 0.0);
        }
        let modes = std::mem::take(&mut self.e_mut(from).timeline_mode);
        let hold_mix = std::mem::take(&mut self.e_mut(from).timeline_hold_mix);
        let mut event_buf = std::mem::take(&mut self.events);
        let mut total_alpha = 0.0;
        for (i, t) in anim.timelines.iter().enumerate() {
            let mode = modes[i];
            let from_mode = mix_from(mode);
            let alpha = if mode & HOLD != 0 {
                match hold_mix[i] {
                    None => alpha_hold,
                    Some(h) => alpha_hold * (1.0 - self.e(h).mix()),
                }
            } else {
                if !draw_order
                    && matches!(t, Timeline::DrawOrder { .. })
                    && from_mode == MixFrom::Current
                {
                    continue;
                }
                alpha_mix
            };
            total_alpha += alpha;
            if !shortest && matches!(t, Timeline::Rotate { .. }) {
                apply_rotate_timeline(
                    t,
                    skeleton,
                    sd,
                    apply_time,
                    alpha,
                    from_mode,
                    &mut rotation,
                    i << 1,
                    first_frame,
                );
            } else if let Timeline::Attachment { .. } = t {
                self.apply_attachment_timeline(
                    t,
                    skeleton,
                    sd,
                    apply_time,
                    from_mode,
                    retain_attachments && alpha >= alpha_attachment_threshold,
                );
            } else {
                let out = !draw_order
                    || !matches!(t, Timeline::DrawOrder { .. })
                    || from_mode == MixFrom::Current;
                let mut ev = use_events.then_some(&mut event_buf);
                apply_timeline(
                    skeleton,
                    sd,
                    t,
                    animation_last,
                    apply_time,
                    &mut ev,
                    alpha,
                    from_mode,
                    add,
                    out,
                    false,
                );
            }
        }
        self.events = event_buf;
        {
            let f = self.e_mut(from);
            f.timeline_mode = modes;
            f.timeline_hold_mix = hold_mix;
        }
        self.put_rotation(from, rotation, record);
        if !record {
            return mix;
        }
        self.e_mut(from).total_alpha = total_alpha;
        if reverse && mix < self.e(from).event_threshold {
            self.events_reverse(anim, animation_last, animation_time);
        }
        if to_mix_duration > 0.0 {
            self.queue_events(from, animation_time, None);
        }
        self.events.clear();
        let f = self.e_mut(from);
        f.next_animation_last = animation_time;
        f.next_track_last = f.track_time;
        mix
    }

    /// `applyAttachmentTimeline`: tracks which slots the animation keyed so
    /// unkeyed ones can return to setup.
    fn apply_attachment_timeline(
        &mut self,
        t: &Timeline,
        skeleton: &mut Skeleton,
        sd: &SkeletonData,
        time: f32,
        from: MixFrom,
        retain: bool,
    ) {
        let Timeline::Attachment {
            slot, frames, keys, ..
        } = t
        else {
            return;
        };
        let s = slot.index();
        if !skeleton.bones[skeleton.slots[s].bone.index()].active {
            return;
        }
        if !retain && skeleton.slots[s].attachment_state == self.unkeyed_state + ATTACH_RETAIN {
            return;
        }
        let mut setup = time < frames[0];
        let mut key = None;
        if !setup {
            key = keys[search(frames, time, 1)];
            setup = !retain && key.is_none();
        }
        if setup {
            if from == MixFrom::Current {
                return;
            }
            key = sd.slots[s].attachment_key;
        }
        set_attachment_by_key(skeleton, *slot, key, false);
        if retain {
            skeleton.slots[s].attachment_state = self.unkeyed_state + ATTACH_RETAIN;
        } else if !setup {
            skeleton.slots[s].attachment_state = self.unkeyed_state + ATTACH_SETUP;
        }
    }

    fn queue_events(
        &mut self,
        entry: EntryId,
        animation_time: f32,
        mut out: Option<&mut Vec<Event>>,
    ) {
        let e = self.e(entry);
        let (start, end) = (e.animation_start, e.animation_end);
        let duration = end - start;
        let reverse = e.reverse;
        let mut split = if duration == 0.0 {
            0.0
        } else {
            e.track_last % duration
        };
        if reverse {
            split = duration - split;
        }
        let complete = if e.looping {
            duration == 0.0 || {
                let cycles = (e.track_time / duration) as i32;
                cycles > 0 && cycles > (e.track_last / duration) as i32
            }
        } else {
            animation_time >= end && e.animation_last < end
        };

        let events = std::mem::take(&mut self.events);
        let mut i = 0;
        while i < events.len() {
            let ev = &events[i];
            if (ev.time < split) != reverse {
                break;
            }
            if ev.time >= start && ev.time <= end {
                self.queue_keyframe(entry, ev, out.as_deref_mut());
            }
            i += 1;
        }
        if complete {
            self.enqueue(EventType::Complete, entry);
        }
        for ev in &events[i..] {
            if ev.time >= start && ev.time <= end {
                self.queue_keyframe(entry, ev, out.as_deref_mut());
            }
        }
        self.events = events;
    }

    fn queue_keyframe(&mut self, entry: EntryId, event: &Event, out: Option<&mut Vec<Event>>) {
        if let Some(out) = out {
            out.push(event.clone());
        }
        self.queue.push(Queued {
            kind: EventType::Event,
            entry,
            event: Some(event.clone()),
        });
    }

    /// Events crossed while playing in reverse.
    fn events_reverse(&mut self, anim: &Animation, animation_last: f32, animation_time: f32) {
        let duration = anim.duration;
        let from = duration - animation_last;
        let to = duration - animation_time;
        for t in &anim.timelines {
            let Timeline::Event { frames, events } = t else {
                continue;
            };
            let push = |this: &mut Self, i: usize| {
                let k = &events[i];
                this.events.push(Event {
                    data: k.event,
                    time: k.time,
                    int_value: k.int_value,
                    float_value: k.float_value,
                    string_value: k.string_value.clone(),
                    volume: k.volume,
                    balance: k.balance,
                });
            };
            if from >= to {
                for (i, &f) in frames.iter().enumerate() {
                    if f < to {
                        continue;
                    }
                    if f >= from {
                        break;
                    }
                    push(self, i);
                }
            } else {
                for (i, &f) in frames.iter().enumerate() {
                    if f >= from {
                        break;
                    }
                    push(self, i);
                }
                let first = frames.iter().position(|&f| f >= to).unwrap_or(frames.len());
                for i in first..frames.len() {
                    push(self, i);
                }
            }
        }
    }

    pub fn clear_tracks(&mut self) {
        let old = self.drain_disabled;
        self.drain_disabled = true;
        for i in 0..self.tracks.len() {
            self.clear_track(i);
        }
        self.tracks.clear();
        self.drain_disabled = old;
        self.drain();
    }

    pub fn clear_track(&mut self, track_index: usize) {
        let Some(current) = self.track(track_index) else {
            return;
        };
        self.enqueue(EventType::End, current);
        self.clear_next(current);
        let mut entry = current;
        while let Some(from) = self.e(entry).mixing_from {
            self.enqueue(EventType::End, from);
            let e = self.e_mut(entry);
            e.mixing_from = None;
            e.mixing_to = None;
            entry = from;
        }
        let index = self.e(current).track_index;
        self.tracks[index] = None;
        self.drain();
    }

    fn set_track(&mut self, index: usize, current: EntryId, interrupt: bool) {
        let from = self.expand_to_index(index);
        self.tracks[index] = Some(current);
        self.e_mut(current).previous = None;
        if let Some(from) = from {
            self.e_mut(from).next = None;
            if interrupt {
                self.enqueue(EventType::Interrupt, from);
            }
            let c = self.e_mut(current);
            c.mixing_from = Some(from);
            c.mix_time = 0.0;
            let f = self.e_mut(from);
            f.mixing_to = Some(current);
            f.timelines_rotation.clear();
        }
        self.enqueue(EventType::Start, current);
    }

    fn expand_to_index(&mut self, index: usize) -> Option<EntryId> {
        if index < self.tracks.len() {
            return self.tracks[index];
        }
        self.tracks.resize(index + 1, None);
        None
    }

    fn new_track_entry(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        looping: bool,
        last: Option<EntryId>,
    ) -> EntryId {
        let mix_duration = last.map_or(0.0, |l| self.data.mix(self.e(l).animation, animation));
        let duration = animation_of(&self.skeleton_data, animation).duration;
        self.alloc(TrackEntry::new(
            track_index,
            animation,
            duration,
            looping,
            mix_duration,
        ))
    }

    fn clear_next(&mut self, entry: EntryId) {
        let mut next = self.e_mut(entry).next.take();
        while let Some(n) = next {
            self.enqueue(EventType::Dispose, n);
            next = self.e(n).next;
        }
    }

    /// Plays `animation` on a track now, mixing from what was playing.
    pub fn set_animation(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        looping: bool,
    ) -> EntryId {
        let mut interrupt = true;
        let mut current = self.expand_to_index(track_index);
        if let Some(c) = current {
            let e = self.e(c);
            if e.next_track_last == -1.0 && e.animation == animation {
                // Don't mix from an entry that was never applied.
                let mixing_from = e.mixing_from;
                self.tracks[track_index] = mixing_from;
                self.enqueue(EventType::Interrupt, c);
                self.enqueue(EventType::End, c);
                self.clear_next(c);
                current = mixing_from;
                interrupt = false;
            } else {
                self.clear_next(c);
            }
        }
        let entry = self.new_track_entry(track_index, animation, looping, current);
        self.set_track(track_index, entry, interrupt);
        self.drain();
        entry
    }

    /// # Errors
    /// [`AnimationNotFound`] if the data has no animation with that name.
    pub fn set_animation_by_name(
        &mut self,
        track_index: usize,
        name: &str,
        looping: bool,
    ) -> Result<EntryId, AnimationNotFound> {
        let id = self.find_animation(name)?;
        Ok(self.set_animation(track_index, id, looping))
    }

    /// Queues `animation` after the track's last entry. `delay <= 0` is
    /// relative to when the previous entry completes, minus the mix.
    pub fn add_animation(
        &mut self,
        track_index: usize,
        animation: AnimationId,
        looping: bool,
        mut delay: f32,
    ) -> EntryId {
        let mut last = self.expand_to_index(track_index);
        if let Some(mut l) = last {
            while let Some(n) = self.e(l).next {
                l = n;
            }
            last = Some(l);
        }
        let entry = self.new_track_entry(track_index, animation, looping, last);
        match last {
            None => {
                self.set_track(track_index, entry, true);
                self.drain();
                if delay < 0.0 {
                    delay = 0.0;
                }
            }
            Some(l) => {
                self.e_mut(l).next = Some(entry);
                self.e_mut(entry).previous = Some(l);
                if delay <= 0.0 {
                    let complete = self.e(l).track_complete();
                    delay = (delay + complete - self.e(entry).mix_duration).max(0.0);
                }
            }
        }
        self.e_mut(entry).delay = delay;
        entry
    }

    /// # Errors
    /// [`AnimationNotFound`] if the data has no animation with that name.
    pub fn add_animation_by_name(
        &mut self,
        track_index: usize,
        name: &str,
        looping: bool,
        delay: f32,
    ) -> Result<EntryId, AnimationNotFound> {
        let id = self.find_animation(name)?;
        Ok(self.add_animation(track_index, id, looping, delay))
    }

    /// Mixes the track back to the setup pose over `mix_duration`.
    pub fn set_empty_animation(&mut self, track_index: usize, mix_duration: f32) -> EntryId {
        let entry = self.set_animation(track_index, EMPTY_ANIMATION_ID, false);
        let e = self.e_mut(entry);
        e.mix_duration = mix_duration;
        e.track_end = mix_duration;
        entry
    }

    pub fn add_empty_animation(
        &mut self,
        track_index: usize,
        mix_duration: f32,
        delay: f32,
    ) -> EntryId {
        let entry = self.add_animation(track_index, EMPTY_ANIMATION_ID, false, delay);
        let e = self.e_mut(entry);
        if delay <= 0.0 {
            e.delay = (e.delay + e.mix_duration - mix_duration).max(0.0);
        }
        e.mix_duration = mix_duration;
        e.track_end = mix_duration;
        entry
    }

    pub fn set_empty_animations(&mut self, mix_duration: f32) {
        let old = self.drain_disabled;
        self.drain_disabled = true;
        for i in 0..self.tracks.len() {
            if self.tracks[i].is_some() {
                self.set_empty_animation(i, mix_duration);
            }
        }
        self.drain_disabled = old;
        self.drain();
    }

    fn find_animation(&self, name: &str) -> Result<AnimationId, AnimationNotFound> {
        self.skeleton_data
            .animations
            .iter()
            .position(|a| a.name == name)
            .map(|i| AnimationId(i as u16))
            .ok_or_else(|| AnimationNotFound(name.to_string()))
    }

    /// Rebuilds timeline modes after the set of entries changed.
    fn animations_changed(&mut self) {
        self.animations_changed = false;
        for i in 0..self.tracks.len() {
            let Some(track) = self.tracks[i] else {
                continue;
            };
            let mut entry = track;
            while let Some(from) = self.e(entry).mixing_from {
                entry = from;
            }
            loop {
                self.compute_hold(entry, track);
                match self.e(entry).mixing_to {
                    Some(to) => entry = to,
                    None => break,
                }
            }
        }
        self.property_owners.clear();
    }

    fn compute_hold(&mut self, entry: EntryId, track: EntryId) {
        let sd = Arc::clone(&self.skeleton_data);
        let e = self.e(entry);
        let anim = animation(&sd, e.animation);
        let n = anim.timelines.len();
        let add = e.additive;
        let keep_hold = e.keep_hold;
        let to = e.mixing_to;
        let mut modes = std::mem::take(&mut self.e_mut(entry).timeline_mode);
        let mut hold_mix = std::mem::take(&mut self.e_mut(entry).timeline_hold_mix);
        modes.resize(n, 0);
        hold_mix.clear();
        hold_mix.resize(n, None);
        for (i, t) in anim.timelines.iter().enumerate() {
            let ids = anim.timeline_property_ids(i);
            let from = self.mix_from_for(track, t, ids);
            let additive = t.is_additive();
            if add && additive {
                modes[i] = from;
                continue;
            }
            let mut mode;
            match to {
                Some(to_id)
                    if !t.is_instant()
                        && (!additive || !self.e(to_id).additive)
                        && animation(&sd, self.e(to_id).animation).has_timeline(ids) =>
                {
                    mode = from | HOLD;
                    let mut next = self.e(to_id).mixing_to;
                    while let Some(nx) = next {
                        let ne = self.e(nx);
                        if (ne.additive && additive)
                            || !animation(&sd, ne.animation).has_timeline(ids)
                        {
                            if ne.mix_duration > 0.0 {
                                hold_mix[i] = Some(nx);
                            }
                            break;
                        }
                        next = ne.mixing_to;
                    }
                }
                _ => mode = from,
            }
            if keep_hold {
                mode = (mode & !HOLD) | (modes[i] & HOLD);
            }
            modes[i] = mode;
        }
        let e = self.e_mut(entry);
        e.timeline_mode = modes;
        e.timeline_hold_mix = hold_mix;
    }

    /// `from`: which pose a timeline mixes from, by which track first keyed
    /// its properties.
    fn mix_from_for(&mut self, track: EntryId, t: &Timeline, ids: &[PropertyId]) -> u8 {
        let mut mode = SETUP;
        let mut owners = std::mem::take(&mut self.property_owners);
        for (i, &id) in ids.iter().enumerate() {
            match owners.get(&id) {
                None => {
                    owners.insert(id, track);
                }
                Some(&owner) => {
                    if owner != track {
                        for &rest in &ids[i + 1..] {
                            owners.entry(rest).or_insert(track);
                        }
                        self.property_owners = owners;
                        return CURRENT;
                    }
                    mode = FIRST;
                }
            }
        }
        if let Timeline::DrawOrderFolder { .. } = t
            && let Some(&first) = owners.get(&crate::data::animation::property_id(
                crate::data::animation::Property::DrawOrder,
                0,
            ))
        {
            mode = if first == track { FIRST } else { CURRENT };
        }
        self.property_owners = owners;
        mode
    }
}

static EMPTY_ANIMATION: std::sync::LazyLock<Animation> =
    std::sync::LazyLock::new(|| Animation::new("<empty>", 0.0));

fn animation_of(sd: &SkeletonData, id: AnimationId) -> &Animation {
    animation(sd, id)
}

fn animation(sd: &SkeletonData, id: AnimationId) -> &Animation {
    if id == EMPTY_ANIMATION_ID {
        &EMPTY_ANIMATION
    } else {
        &sd.animations[id.index()]
    }
}

/// `applyRotateTimeline`: mixes rotation the shortest way on the first
/// frame, then keeps that direction.
fn apply_rotate_timeline(
    t: &Timeline,
    skeleton: &mut Skeleton,
    sd: &SkeletonData,
    time: f32,
    alpha: f32,
    from: MixFrom,
    rotation: &mut [f32],
    i: usize,
    first_frame: bool,
) {
    if first_frame {
        rotation[i] = 0.0;
    }
    if alpha == 1.0 {
        apply_timeline(
            skeleton, sd, t, 0.0, time, &mut None, 1.0, from, false, false, false,
        );
        return;
    }
    let Timeline::Rotate { bone, curves } = t else {
        return;
    };
    let b = &mut skeleton.bones[bone.index()];
    if !b.active {
        return;
    }
    let setup = sd.bones[bone.index()].setup.rotation;
    let pose = &mut b.posed.pose;
    let (r1, r2);
    if time < curves.frames[0] {
        match from {
            MixFrom::Setup => {
                pose.rotation = setup;
                return;
            }
            MixFrom::Current => return,
            MixFrom::First => {}
        }
        r1 = pose.rotation;
        r2 = setup;
    } else {
        r1 = if from == MixFrom::Setup {
            setup
        } else {
            pose.rotation
        };
        r2 = setup + curve_value1(&curves.frames, &curves.curves, time);
    }
    let mut diff = r2 - r1;
    diff -= (diff / 360.0 - 0.5).ceil() * 360.0;
    let total;
    if diff == 0.0 {
        total = rotation[i];
    } else {
        let (last_total, last_diff) = if first_frame {
            (0.0, diff)
        } else {
            (rotation[i], rotation[i + 1])
        };
        let loops = last_total - last_total % 360.0;
        let mut t = diff + loops;
        let current = diff >= 0.0;
        let mut dir = last_total >= 0.0;
        if last_diff.abs() <= 90.0 && sign(last_diff) != sign(diff) {
            if (last_total - loops).abs() > 180.0 {
                t += 360.0 * sign(last_total);
                dir = current;
            } else if loops != 0.0 {
                t -= 360.0 * sign(last_total);
            } else {
                dir = current;
            }
        }
        if dir != current {
            t += 360.0 * sign(last_total);
        }
        rotation[i] = t;
        total = t;
    }
    rotation[i + 1] = diff;
    pose.rotation = r1 + total * alpha;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no animation named `{0}`")]
pub struct AnimationNotFound(pub String);
