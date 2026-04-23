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

//! Setup-pose event definition. Runtime `Event` instances produced during
//! animation apply inherit these defaults and may override them per-keyframe.

use crate::data::EventId;

/// Named event declared on the skeleton. Animations fire runtime events that
/// reference one of these by index.
#[derive(Debug, Clone, PartialEq)]
pub struct EventData {
    pub index: EventId,
    pub name: String,
    pub int_value: i32,
    pub float_value: f32,
    pub string_value: String,
    pub audio_path: String,
    pub volume: f32,
    pub balance: f32,
}

impl EventData {
    #[must_use]
    pub fn new(index: EventId, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            int_value: 0,
            float_value: 0.0,
            string_value: String::new(),
            audio_path: String::new(),
            volume: 1.0,
            balance: 0.0,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Literal default comparisons only.
mod tests {
    use super::*;

    #[test]
    fn new_event_defaults() {
        let e = EventData::new(EventId(0), "footstep");
        assert_eq!(e.volume, 1.0);
        assert_eq!(e.balance, 0.0);
    }
}
