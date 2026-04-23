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

//! Curve-timeline evaluation primitives.
//!
//! Ports `spine::Animation::search`, `spine::CurveTimeline::getBezierValue`,
//! `spine::CurveTimeline1::getCurveValue`, `spine::CurveTimeline2::getCurveValue`,
//! and the `getRelativeValue` / `getAbsoluteValue` / `getScaleValue` helpers
//! on `CurveTimeline1`. All operate on raw `[f32]` slices so callers can
//! pass either field from [`CurveFrames`][crate::data::CurveFrames] or local
//! scratch buffers.

// Curve evaluation uses spine-cpp's short variable names (x, y, s, i, j, n)
// verbatim; renaming loses the diff-ability against the reference.
#![allow(clippy::many_single_char_names)]
// `alpha == 1.0` and `blend == MixBlend::Setup` are literal tag checks from
// spine-cpp — they're not the "imprecise equality" case float_cmp flags.
#![allow(clippy::float_cmp)]

use crate::animation::{
    BEZIER_SIZE, CURVE_BEZIER, CURVE_LINEAR, CURVE_STEPPED, MixBlend, MixDirection,
};

/// Find the largest `i` in `step, 2*step, 3*step, …` with `frames[i] <= target`.
///
/// Returns `frames.len() - step` when every frame after index 0 is still
/// less than or equal to `target` (i.e. we're past the last keyframe). Ports
/// `spine::Animation::search(Vector<float>&, float, int)`.
#[must_use]
pub fn search(frames: &[f32], target: f32, step: usize) -> usize {
    let n = frames.len();
    let mut i = step;
    while i < n {
        if frames[i] > target {
            return i - step;
        }
        i += step;
    }
    n - step
}

/// Bezier-segment interpolation for a curve-timeline value.
///
/// `frames` is the timeline's frame data (interleaved times + values),
/// `curves` is the per-frame type codes followed by bezier samples.
/// `frame_index` is the leftmost frame in the segment (step-aligned),
/// `value_offset` is the offset into `frames[frame_index..]` of the value
/// column we're reading. `frame_entries` is the stride (2 for
/// `CurveTimeline1`, 3 for `CurveTimeline2`, more for the colour timelines).
/// `i` is the absolute offset into `curves` of the bezier segment's first
/// x-sample (i.e. `frames.len()_over_stride + bezier_seg * BEZIER_SIZE`
/// — caller decodes it from `curves[frame] - CURVE_BEZIER`).
///
/// Ports `spine::CurveTimeline::getBezierValue`.
#[must_use]
#[allow(clippy::too_many_arguments)] // mirrors spine-cpp's eight-parameter signature
pub fn bezier_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    frame_index: usize,
    value_offset: usize,
    i: usize,
    frame_entries: usize,
) -> f32 {
    // First bezier sample's x is > time → we're in the segment between
    // `frames[frame_index]` and the first sample, so linear-interpolate.
    if curves[i] > time {
        let x = frames[frame_index];
        let y = frames[frame_index + value_offset];
        return y + (time - x) / (curves[i] - x) * (curves[i + 1] - y);
    }

    // Walk the 9 (x, y) bezier samples looking for the first whose x >= time.
    let n = i + BEZIER_SIZE;
    let mut j = i + 2;
    while j < n {
        if curves[j] >= time {
            let x = curves[j - 2];
            let y = curves[j - 1];
            return y + (time - x) / (curves[j] - x) * (curves[j + 1] - y);
        }
        j += 2;
    }

    // Past the last sample: interpolate between the last sample and the
    // next frame's value. spine-cpp advances `frame_index` by frame_entries
    // here, then reads frames[frame_index], frames[frame_index + value_offset].
    let next_frame = frame_index + frame_entries;
    let x = curves[n - 2];
    let y = curves[n - 1];
    y + (time - x) / (frames[next_frame] - x) * (frames[next_frame + value_offset] - y)
}

/// Sample a single-value curve-timeline (stride 2) at `time`.
///
/// Expects `frames = [t0, v0, t1, v1, …]` and `curves = [type_0, type_1,
/// …, type_(N-1), bezier_samples…]`. Ports
/// `spine::CurveTimeline1::getCurveValue`.
#[must_use]
pub fn curve_value1(frames: &[f32], curves: &[f32], time: f32) -> f32 {
    const ENTRIES: usize = 2;
    const VALUE: usize = 1;

    // Find the frame index i (always a multiple of ENTRIES) whose time is
    // the largest <= `time`. spine-cpp inlines this as a linear scan
    // starting at `ii = 2`; port it literally (isize to carry the "past
    // last frame" default across the signed comparison) to keep the
    // search behaviour identical.
    let mut i: isize = frames.len() as isize - ENTRIES as isize;
    let mut ii = ENTRIES;
    while ii as isize <= i {
        if frames[ii] > time {
            i = ii as isize - ENTRIES as isize;
            break;
        }
        ii += ENTRIES;
    }
    let i = i as usize;

    let curve_type = curves[i / ENTRIES] as i32;
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            let value = frames[i + VALUE];
            value
                + (time - before) / (frames[i + ENTRIES] - before)
                    * (frames[i + ENTRIES + VALUE] - value)
        }
        CURVE_STEPPED => frames[i + VALUE],
        _ => bezier_value(
            frames,
            curves,
            time,
            i,
            VALUE,
            (curve_type - CURVE_BEZIER) as usize,
            ENTRIES,
        ),
    }
}

/// Sample a two-value curve-timeline (stride 3, e.g. `Translate`, `Scale`,
/// `Shear`) at `time`. Returns `(value1, value2)`.
///
/// Ports the inline evaluation in `TranslateTimeline::apply` etc.
#[must_use]
pub fn curve_value2(frames: &[f32], curves: &[f32], time: f32) -> (f32, f32) {
    const ENTRIES: usize = 3;
    const VALUE1: usize = 1;
    const VALUE2: usize = 2;

    let i = search(frames, time, ENTRIES);
    let curve_type = curves[i / ENTRIES] as i32;
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            let mut x = frames[i + VALUE1];
            let mut y = frames[i + VALUE2];
            let t = (time - before) / (frames[i + ENTRIES] - before);
            x += (frames[i + ENTRIES + VALUE1] - x) * t;
            y += (frames[i + ENTRIES + VALUE2] - y) * t;
            (x, y)
        }
        CURVE_STEPPED => (frames[i + VALUE1], frames[i + VALUE2]),
        _ => {
            let bezier_i = (curve_type - CURVE_BEZIER) as usize;
            let x = bezier_value(frames, curves, time, i, VALUE1, bezier_i, ENTRIES);
            let y = bezier_value(
                frames,
                curves,
                time,
                i,
                VALUE2,
                bezier_i + BEZIER_SIZE,
                ENTRIES,
            );
            (x, y)
        }
    }
}

/// Blend a `CurveTimeline1` sample into a "relative" target (translate,
/// rotate, shear) according to `blend` and `alpha`. Ports
/// `CurveTimeline1::getRelativeValue`.
///
/// `current` is the target's current runtime value; `setup` is its
/// setup-pose value.
#[must_use]
pub fn relative_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    blend: MixBlend,
    current: f32,
    setup: f32,
) -> f32 {
    if time < frames[0] {
        return match blend {
            MixBlend::Setup => setup,
            MixBlend::First => current + (setup - current) * alpha,
            MixBlend::Replace | MixBlend::Add => current,
        };
    }
    let value = curve_value1(frames, curves, time);
    match blend {
        MixBlend::Setup => setup + value * alpha,
        MixBlend::First | MixBlend::Replace => current + (value + setup - current) * alpha,
        MixBlend::Add => current + value * alpha,
    }
}

/// Blend a `CurveTimeline1` sample into an "absolute" target (like alpha) —
/// setup-pose baseline is overwritten by the timeline, not added to.
/// Ports `CurveTimeline1::getAbsoluteValue`.
#[must_use]
pub fn absolute_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    blend: MixBlend,
    current: f32,
    setup: f32,
) -> f32 {
    if time < frames[0] {
        return match blend {
            MixBlend::Setup => setup,
            MixBlend::First => current + (setup - current) * alpha,
            MixBlend::Replace | MixBlend::Add => current,
        };
    }
    let value = curve_value1(frames, curves, time);
    if blend == MixBlend::Setup {
        return setup + (value - setup) * alpha;
    }
    current + (value - current) * alpha
}

/// Scale-specific blend (multiplicative against setup). Ports
/// `CurveTimeline1::getScaleValue`.
///
/// Different from [`absolute_value`] because the timeline's stored value
/// is a *scale* multiplier; the reflected sign convention depends on
/// whether the animation is mixing in or out, and on whether setup vs
/// current is the baseline.
#[must_use]
#[allow(clippy::too_many_arguments)] // matches spine-cpp's getScaleValue signature
pub fn scale_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    blend: MixBlend,
    direction: MixDirection,
    current: f32,
    setup: f32,
) -> f32 {
    if time < frames[0] {
        return match blend {
            MixBlend::Setup => setup,
            MixBlend::First => current + (setup - current) * alpha,
            MixBlend::Replace | MixBlend::Add => current,
        };
    }
    let value = curve_value1(frames, curves, time) * setup;
    if alpha == 1.0 {
        return match blend {
            MixBlend::Add => current + value - setup,
            _ => value,
        };
    }
    // Signs below follow spine-cpp verbatim; the comment there says "Mixing
    // out uses sign of setup or current pose, else use sign of key."
    if direction == MixDirection::Out {
        match blend {
            MixBlend::Setup => setup + (value.abs() * setup.signum() - setup) * alpha,
            MixBlend::First | MixBlend::Replace => {
                current + (value.abs() * current.signum() - current) * alpha
            }
            MixBlend::Add => current + (value - setup) * alpha,
        }
    } else {
        match blend {
            MixBlend::Setup => {
                let s = setup.abs() * value.signum();
                s + (value - s) * alpha
            }
            MixBlend::First | MixBlend::Replace => {
                let s = current.abs() * value.signum();
                s + (value - s) * alpha
            }
            MixBlend::Add => current + (value - setup) * alpha,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // testing exact algebra on small inputs
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn search_finds_largest_le() {
        // frames = [0.0, v, 1.0, v, 2.5, v] — step=2, values in odd slots.
        let frames = [0.0_f32, 10.0, 1.0, 20.0, 2.5, 30.0];
        assert_eq!(search(&frames, -0.5, 2), 0);
        assert_eq!(search(&frames, 0.0, 2), 0);
        assert_eq!(search(&frames, 0.5, 2), 0);
        assert_eq!(search(&frames, 1.0, 2), 2);
        assert_eq!(search(&frames, 2.0, 2), 2);
        assert_eq!(search(&frames, 2.5, 2), 4);
        assert_eq!(search(&frames, 100.0, 2), 4);
    }

    /// Two-frame linear ramp from 0 at t=0 to 10 at t=1.
    fn linear_ramp_1() -> (Vec<f32>, Vec<f32>) {
        // Frame layout: [t0, v0, t1, v1] = [0, 0, 1, 10].
        // Curves: two frames → [type_0, type_1] = [LINEAR, LINEAR]. Note:
        // spine-cpp's CurveTimeline ctor sets curves[frameCount - 1] = STEPPED,
        // but for apply() purposes, curves[last_frame] is never read (search
        // returns a non-last index unless time exceeds the last frame).
        (
            vec![0.0, 0.0, 1.0, 10.0],
            vec![CURVE_LINEAR as f32, CURVE_STEPPED as f32],
        )
    }

    #[test]
    fn curve_value1_linear_interpolation() {
        let (f, c) = linear_ramp_1();
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.0), 0.0);
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.25), 2.5);
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.5), 5.0);
        assert_abs_diff_eq!(curve_value1(&f, &c, 1.0), 10.0);
    }

    #[test]
    fn curve_value1_stepped_returns_left_value() {
        let frames = vec![0.0, 1.0, 1.0, 42.0];
        let curves = vec![CURVE_STEPPED as f32, CURVE_STEPPED as f32];
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.0), 1.0);
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.5), 1.0);
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.999), 1.0);
    }

    #[test]
    fn curve_value2_linear_interpolation_on_translate_layout() {
        // TranslateTimeline stride 3: [t0, x0, y0, t1, x1, y1].
        let frames = vec![0.0_f32, 1.0, 2.0, 1.0, 11.0, 22.0];
        let curves = vec![CURVE_LINEAR as f32, CURVE_STEPPED as f32];
        let (x0, y0) = curve_value2(&frames, &curves, 0.0);
        let (xm, ym) = curve_value2(&frames, &curves, 0.5);
        let (x1, y1) = curve_value2(&frames, &curves, 1.0);
        assert_abs_diff_eq!(x0, 1.0);
        assert_abs_diff_eq!(y0, 2.0);
        assert_abs_diff_eq!(xm, 6.0);
        assert_abs_diff_eq!(ym, 12.0);
        assert_abs_diff_eq!(x1, 11.0);
        assert_abs_diff_eq!(y1, 22.0);
    }

    #[test]
    fn relative_value_respects_mix_blend() {
        let (f, c) = linear_ramp_1();
        // Time 0.5 → curve value 5.
        let current = 7.0;
        let setup = 3.0;
        let alpha = 0.5;

        // Setup: setup + value * alpha = 3 + 5 * 0.5 = 5.5.
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, alpha, MixBlend::Setup, current, setup),
            5.5
        );
        // Add: current + value * alpha = 7 + 5 * 0.5 = 9.5.
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, alpha, MixBlend::Add, current, setup),
            9.5
        );
        // Replace: current + (value + setup - current) * alpha
        //        = 7 + (5 + 3 - 7) * 0.5 = 7.5.
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, alpha, MixBlend::Replace, current, setup),
            7.5
        );
    }

    #[test]
    fn relative_value_before_first_frame() {
        let (f, c) = linear_ramp_1();
        // Frames start at t=0, so negative time is "before first frame".
        assert_abs_diff_eq!(
            relative_value(&f, &c, -1.0, 0.5, MixBlend::Setup, 7.0, 3.0),
            3.0
        );
        assert_abs_diff_eq!(
            relative_value(&f, &c, -1.0, 0.5, MixBlend::First, 7.0, 3.0),
            7.0 + (3.0 - 7.0) * 0.5
        );
        assert_abs_diff_eq!(
            relative_value(&f, &c, -1.0, 0.5, MixBlend::Replace, 7.0, 3.0),
            7.0
        );
    }

    #[test]
    fn absolute_value_setup_lerps_to_timeline_value() {
        let (f, c) = linear_ramp_1();
        // Timeline value at t=0.5 is 5, setup 3, current 7, alpha 0.5.
        // Setup: setup + (value - setup) * alpha = 3 + (5 - 3) * 0.5 = 4.
        assert_abs_diff_eq!(
            absolute_value(&f, &c, 0.5, 0.5, MixBlend::Setup, 7.0, 3.0),
            4.0
        );
        // Add/Replace: current + (value - current) * alpha = 7 + (5 - 7) * 0.5 = 6.
        assert_abs_diff_eq!(
            absolute_value(&f, &c, 0.5, 0.5, MixBlend::Replace, 7.0, 3.0),
            6.0
        );
    }
}
